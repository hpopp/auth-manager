use redb::ReadableTable;

use super::db::{expiry_key, Database, DatabaseError};
use super::models::SessionToken;
use super::tables::*;

impl Database {
    // ========================================================================
    // Session operations
    // ========================================================================

    /// Store a session token
    pub fn put_session(&self, session: &SessionToken) -> Result<(), DatabaseError> {
        debug_assert!(!session.token.is_empty(), "session token must not be empty");
        debug_assert!(!session.id.is_empty(), "session id must not be empty");
        debug_assert!(
            !session.subject_id.is_empty(),
            "session subject_id must not be empty"
        );

        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS)?;
            let data = rmp_serde::to_vec_named(session)?;
            table.insert(session.token.as_str(), data.as_slice())?;

            // Update resource_sessions index (keyed by token)
            let mut index_table = write_txn.open_table(SUBJECT_SESSIONS)?;
            let mut tokens: Vec<String> = index_table
                .get(session.subject_id.as_str())?
                .map(|v| rmp_serde::from_slice(v.value()))
                .transpose()?
                .unwrap_or_default();

            if !tokens.contains(&session.token) {
                tokens.push(session.token.clone());
                let index_data = rmp_serde::to_vec_named(&tokens)?;
                index_table.insert(session.subject_id.as_str(), index_data.as_slice())?;
            }

            // Update session ID index (UUID -> token)
            let mut id_table = write_txn.open_table(SESSION_IDS)?;
            id_table.insert(session.id.as_str(), session.token.as_str())?;

            // Update expiration index
            let mut expiry_table = write_txn.open_table(SESSION_EXPIRY)?;
            let ek = expiry_key(&session.expires_at, &session.token);
            expiry_table.insert(ek.as_str(), session.token.as_str())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get a session by its secret token value
    pub fn get_session(&self, token: &str) -> Result<Option<SessionToken>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(SESSIONS)?;

        match table.get(token)? {
            Some(data) => {
                let session: SessionToken = rmp_serde::from_slice(data.value())?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    /// Delete a session by its secret token value
    pub fn delete_session(&self, token: &str) -> Result<bool, DatabaseError> {
        let write_txn = self.begin_write()?;

        // First, get the session for index cleanup
        let session: Option<SessionToken> = {
            let table = write_txn.open_table(SESSIONS)?;
            let result = table.get(token)?;
            match result {
                Some(data) => Some(rmp_serde::from_slice(data.value())?),
                None => None,
            }
        };

        let deleted = match session {
            Some(session) => {
                // Remove from sessions table
                {
                    let mut table = write_txn.open_table(SESSIONS)?;
                    table.remove(token)?;
                }

                // Update subject_sessions index
                let tokens: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(SUBJECT_SESSIONS)?;
                    let result = index_table.get(session.subject_id.as_str())?;
                    match result {
                        Some(data) => Some(rmp_serde::from_slice(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut t) = tokens {
                    t.retain(|v| v != token);
                    let mut index_table = write_txn.open_table(SUBJECT_SESSIONS)?;
                    if t.is_empty() {
                        index_table.remove(session.subject_id.as_str())?;
                    } else {
                        let new_index_data = rmp_serde::to_vec_named(&t)?;
                        index_table
                            .insert(session.subject_id.as_str(), new_index_data.as_slice())?;
                    }
                }

                // Remove from session ID index
                {
                    let mut id_table = write_txn.open_table(SESSION_IDS)?;
                    id_table.remove(session.id.as_str())?;
                }

                // Remove from expiration index
                {
                    let mut expiry_table = write_txn.open_table(SESSION_EXPIRY)?;
                    let ek = expiry_key(&session.expires_at, token);
                    expiry_table.remove(ek.as_str())?;
                }

                true
            }
            None => false,
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Delete expired sessions using the expiration index (no full table scan).
    pub fn delete_expired_sessions(&self) -> Result<usize, DatabaseError> {
        let now = chrono::Utc::now();
        let now_ms = now.timestamp_millis();

        // Phase 1: read the expiration index to collect expired entries
        let expired: Vec<(String, String)> = {
            let read_txn = self.begin_read()?;
            let table = read_txn.open_table(SESSION_EXPIRY)?;
            let mut result = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let key_str = key.value().to_string();
                match super::db::expiry_key_ms(&key_str) {
                    Some(ms) if ms <= now_ms => {
                        result.push((key_str, value.value().to_string()));
                    }
                    _ => break,
                }
            }
            result
        };

        if expired.is_empty() {
            return Ok(0);
        }

        // Phase 2: delete expired sessions and clean up all indexes
        let write_txn = self.begin_write()?;

        for (expiry_key_val, token) in &expired {
            // Look up the session to get subject_id and id for index cleanup
            let session: Option<SessionToken> = {
                let table = write_txn.open_table(SESSIONS)?;
                let result = table.get(token.as_str())?;
                match result {
                    Some(data) => Some(rmp_serde::from_slice(data.value())?),
                    None => None,
                }
            };

            if let Some(session) = session {
                {
                    let mut table = write_txn.open_table(SESSIONS)?;
                    table.remove(token.as_str())?;
                }

                // Clean up subject_sessions index
                let tokens: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(SUBJECT_SESSIONS)?;
                    let result = index_table.get(session.subject_id.as_str())?;
                    match result {
                        Some(data) => Some(rmp_serde::from_slice(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut t) = tokens {
                    t.retain(|v| v != token);
                    let mut index_table = write_txn.open_table(SUBJECT_SESSIONS)?;
                    if t.is_empty() {
                        index_table.remove(session.subject_id.as_str())?;
                    } else {
                        let new_index_data = rmp_serde::to_vec_named(&t)?;
                        index_table
                            .insert(session.subject_id.as_str(), new_index_data.as_slice())?;
                    }
                }

                {
                    let mut id_table = write_txn.open_table(SESSION_IDS)?;
                    id_table.remove(session.id.as_str())?;
                }
            }

            // Remove from expiration index
            {
                let mut expiry_table = write_txn.open_table(SESSION_EXPIRY)?;
                expiry_table.remove(expiry_key_val.as_str())?;
            }
        }

        write_txn.commit()?;
        Ok(expired.len())
    }

    /// Get all sessions (for expiration cleanup)
    pub fn get_all_sessions(&self) -> Result<Vec<SessionToken>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(SESSIONS)?;

        let mut sessions = Vec::new();
        for result in table.iter()? {
            let (_, value) = result?;
            let session: SessionToken = rmp_serde::from_slice(value.value())?;
            sessions.push(session);
        }

        Ok(sessions)
    }

    /// Get a session by its UUID (resolves ID -> token -> session)
    pub fn get_session_by_id(&self, id: &str) -> Result<Option<SessionToken>, DatabaseError> {
        let token = match self.get_session_token_by_id(id)? {
            Some(t) => t,
            None => return Ok(None),
        };
        self.get_session(&token)
    }

    /// Get all sessions for a resource
    pub fn get_sessions_by_subject(
        &self,
        subject_id: &str,
    ) -> Result<Vec<SessionToken>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let index_table = read_txn.open_table(SUBJECT_SESSIONS)?;
        let sessions_table = read_txn.open_table(SESSIONS)?;

        let token_ids: Vec<String> = match index_table.get(subject_id)? {
            Some(data) => rmp_serde::from_slice(data.value())?,
            None => return Ok(Vec::new()),
        };

        let mut sessions = Vec::new();
        for token_id in token_ids {
            if let Some(data) = sessions_table.get(token_id.as_str())? {
                let session: SessionToken = rmp_serde::from_slice(data.value())?;
                sessions.push(session);
            }
        }

        Ok(sessions)
    }

    /// Resolve a session UUID to its secret token value
    pub fn get_session_token_by_id(&self, id: &str) -> Result<Option<String>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(SESSION_IDS)?;

        match table.get(id)? {
            Some(data) => Ok(Some(data.value().to_string())),
            None => Ok(None),
        }
    }

    /// Update last_used_at for a session (local-only, no replication)
    pub fn touch_session(&self, token: &str) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        let existing = {
            let table = write_txn.open_table(SESSIONS)?;
            let result = match table.get(token)? {
                Some(data) => Some(rmp_serde::from_slice::<SessionToken>(data.value())?),
                None => None,
            };
            result
        };
        if let Some(mut session) = existing {
            session.last_used_at = Some(chrono::Utc::now());
            let serialized = rmp_serde::to_vec_named(&session)?;
            let mut table = write_txn.open_table(SESSIONS)?;
            table.insert(token, serialized.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}
