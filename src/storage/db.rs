use redb::{Database as RedbDatabase, ReadTransaction, ReadableTable, WriteTransaction};
use std::path::Path;
use thiserror::Error;

use super::models::{ApiKey, NodeState, ReplicatedWrite, SessionToken};
use super::tables::*;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Redb(#[from] redb::Error),
    #[error("Database error: {0}")]
    RedbDatabase(#[from] redb::DatabaseError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("Table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("Transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),
}

pub struct Database {
    db: RedbDatabase,
}

impl Database {
    /// Open or create a database at the given path
    pub fn open<P: AsRef<Path>>(data_dir: P) -> Result<Self, DatabaseError> {
        std::fs::create_dir_all(data_dir.as_ref())?;
        let db_path = data_dir.as_ref().join("auth-manager.redb");
        let db = RedbDatabase::create(db_path)?;

        // Initialize tables
        let write_txn = db.begin_write()?;
        {
            // Create tables if they don't exist
            let _ = write_txn.open_table(SESSIONS)?;
            let _ = write_txn.open_table(API_KEYS)?;
            let _ = write_txn.open_table(REPLICATION_LOG)?;
            let _ = write_txn.open_table(NODE_META)?;
            let _ = write_txn.open_table(RESOURCE_API_KEYS)?;
            let _ = write_txn.open_table(RESOURCE_SESSIONS)?;
        }
        write_txn.commit()?;

        Ok(Self { db })
    }

    /// Begin a read transaction
    pub fn begin_read(&self) -> Result<ReadTransaction, DatabaseError> {
        Ok(self.db.begin_read()?)
    }

    /// Begin a write transaction
    pub fn begin_write(&self) -> Result<WriteTransaction, DatabaseError> {
        Ok(self.db.begin_write()?)
    }

    // ========================================================================
    // Session operations
    // ========================================================================

    /// Store a session token
    pub fn put_session(&self, session: &SessionToken) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS)?;
            let data = bincode::serialize(session)?;
            table.insert(session.id.as_str(), data.as_slice())?;

            // Update resource_sessions index
            let mut index_table = write_txn.open_table(RESOURCE_SESSIONS)?;
            let mut token_ids: Vec<String> = index_table
                .get(session.resource_id.as_str())?
                .map(|v| bincode::deserialize(v.value()).unwrap_or_default())
                .unwrap_or_default();

            if !token_ids.contains(&session.id) {
                token_ids.push(session.id.clone());
                let index_data = bincode::serialize(&token_ids)?;
                index_table.insert(session.resource_id.as_str(), index_data.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get a session token by ID
    pub fn get_session(&self, token_id: &str) -> Result<Option<SessionToken>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(SESSIONS)?;

        match table.get(token_id)? {
            Some(data) => {
                let session: SessionToken = bincode::deserialize(data.value())?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    /// Delete a session token
    pub fn delete_session(&self, token_id: &str) -> Result<bool, DatabaseError> {
        let write_txn = self.begin_write()?;

        // First, get the session's resource_id for index cleanup
        let resource_id: Option<String> = {
            let table = write_txn.open_table(SESSIONS)?;
            let result = table.get(token_id)?;
            match result {
                Some(data) => {
                    let session: SessionToken = bincode::deserialize(data.value())?;
                    Some(session.resource_id)
                }
                None => None,
            }
        };

        let deleted = match resource_id {
            Some(rid) => {
                // Remove from sessions table
                {
                    let mut table = write_txn.open_table(SESSIONS)?;
                    table.remove(token_id)?;
                }

                // Update resource_sessions index
                let token_ids: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(RESOURCE_SESSIONS)?;
                    let result = index_table.get(rid.as_str())?;
                    match result {
                        Some(data) => Some(bincode::deserialize(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut ids) = token_ids {
                    ids.retain(|id| id != token_id);
                    let mut index_table = write_txn.open_table(RESOURCE_SESSIONS)?;
                    if ids.is_empty() {
                        index_table.remove(rid.as_str())?;
                    } else {
                        let new_index_data = bincode::serialize(&ids)?;
                        index_table.insert(rid.as_str(), new_index_data.as_slice())?;
                    }
                }
                true
            }
            None => false,
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Get all sessions for a resource
    pub fn get_sessions_by_resource(
        &self,
        resource_id: &str,
    ) -> Result<Vec<SessionToken>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let index_table = read_txn.open_table(RESOURCE_SESSIONS)?;
        let sessions_table = read_txn.open_table(SESSIONS)?;

        let token_ids: Vec<String> = match index_table.get(resource_id)? {
            Some(data) => bincode::deserialize(data.value())?,
            None => return Ok(Vec::new()),
        };

        let mut sessions = Vec::new();
        for token_id in token_ids {
            if let Some(data) = sessions_table.get(token_id.as_str())? {
                let session: SessionToken = bincode::deserialize(data.value())?;
                sessions.push(session);
            }
        }

        Ok(sessions)
    }

    /// Get all sessions (for expiration cleanup)
    pub fn get_all_sessions(&self) -> Result<Vec<SessionToken>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(SESSIONS)?;

        let mut sessions = Vec::new();
        for result in table.iter()? {
            let (_, value) = result?;
            let session: SessionToken = bincode::deserialize(value.value())?;
            sessions.push(session);
        }

        Ok(sessions)
    }

    // ========================================================================
    // API key operations
    // ========================================================================

    /// Store an API key
    pub fn put_api_key(&self, api_key: &ApiKey) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(API_KEYS)?;
            let data = bincode::serialize(api_key)?;
            table.insert(api_key.key_hash.as_str(), data.as_slice())?;

            // Update resource_api_keys index
            let mut index_table = write_txn.open_table(RESOURCE_API_KEYS)?;
            let mut key_hashes: Vec<String> = index_table
                .get(api_key.resource_id.as_str())?
                .map(|v| bincode::deserialize(v.value()).unwrap_or_default())
                .unwrap_or_default();

            if !key_hashes.contains(&api_key.key_hash) {
                key_hashes.push(api_key.key_hash.clone());
                let index_data = bincode::serialize(&key_hashes)?;
                index_table.insert(api_key.resource_id.as_str(), index_data.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get an API key by hash
    pub fn get_api_key(&self, key_hash: &str) -> Result<Option<ApiKey>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(API_KEYS)?;

        match table.get(key_hash)? {
            Some(data) => {
                let api_key: ApiKey = bincode::deserialize(data.value())?;
                Ok(Some(api_key))
            }
            None => Ok(None),
        }
    }

    /// Delete an API key
    pub fn delete_api_key(&self, key_hash: &str) -> Result<bool, DatabaseError> {
        let write_txn = self.begin_write()?;

        // First, get the API key's resource_id for index cleanup
        let resource_id: Option<String> = {
            let table = write_txn.open_table(API_KEYS)?;
            let result = table.get(key_hash)?;
            match result {
                Some(data) => {
                    let api_key: ApiKey = bincode::deserialize(data.value())?;
                    Some(api_key.resource_id)
                }
                None => None,
            }
        };

        let deleted = match resource_id {
            Some(rid) => {
                // Remove from api_keys table
                {
                    let mut table = write_txn.open_table(API_KEYS)?;
                    table.remove(key_hash)?;
                }

                // Update resource_api_keys index
                let key_hashes: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(RESOURCE_API_KEYS)?;
                    let result = index_table.get(rid.as_str())?;
                    match result {
                        Some(data) => Some(bincode::deserialize(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut hashes) = key_hashes {
                    hashes.retain(|h| h != key_hash);
                    let mut index_table = write_txn.open_table(RESOURCE_API_KEYS)?;
                    if hashes.is_empty() {
                        index_table.remove(rid.as_str())?;
                    } else {
                        let new_index_data = bincode::serialize(&hashes)?;
                        index_table.insert(rid.as_str(), new_index_data.as_slice())?;
                    }
                }
                true
            }
            None => false,
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Get all API keys for a resource
    pub fn get_api_keys_by_resource(
        &self,
        resource_id: &str,
    ) -> Result<Vec<ApiKey>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let index_table = read_txn.open_table(RESOURCE_API_KEYS)?;
        let keys_table = read_txn.open_table(API_KEYS)?;

        let key_hashes: Vec<String> = match index_table.get(resource_id)? {
            Some(data) => bincode::deserialize(data.value())?,
            None => return Ok(Vec::new()),
        };

        let mut api_keys = Vec::new();
        for key_hash in key_hashes {
            if let Some(data) = keys_table.get(key_hash.as_str())? {
                let api_key: ApiKey = bincode::deserialize(data.value())?;
                api_keys.push(api_key);
            }
        }

        Ok(api_keys)
    }

    /// Get all API keys (for expiration cleanup)
    pub fn get_all_api_keys(&self) -> Result<Vec<ApiKey>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(API_KEYS)?;

        let mut keys = Vec::new();
        for result in table.iter()? {
            let (_, value) = result?;
            let api_key: ApiKey = bincode::deserialize(value.value())?;
            keys.push(api_key);
        }

        Ok(keys)
    }

    // ========================================================================
    // Node state operations
    // ========================================================================

    /// Get the node state
    pub fn get_node_state(&self) -> Result<Option<NodeState>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(NODE_META)?;

        match table.get("state")? {
            Some(data) => {
                let state: NodeState = bincode::deserialize(data.value())?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// Save the node state
    pub fn put_node_state(&self, state: &NodeState) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(NODE_META)?;
            let data = bincode::serialize(state)?;
            table.insert("state", data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    // ========================================================================
    // Replication log operations
    // ========================================================================

    /// Append a write to the replication log
    pub fn append_replication_log(&self, write: &ReplicatedWrite) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(REPLICATION_LOG)?;
            let data = bincode::serialize(write)?;
            table.insert(write.sequence, data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get replication log entries starting from a sequence number
    pub fn get_replication_log_from(
        &self,
        from_sequence: u64,
    ) -> Result<Vec<ReplicatedWrite>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(REPLICATION_LOG)?;

        let mut entries = Vec::new();
        for result in table.range(from_sequence..)? {
            let (_, value) = result?;
            let entry: ReplicatedWrite = bincode::deserialize(value.value())?;
            entries.push(entry);
        }

        Ok(entries)
    }

    /// Get the latest sequence number
    pub fn get_latest_sequence(&self) -> Result<u64, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(REPLICATION_LOG)?;

        let result = table.last()?;
        match result {
            Some((key, _)) => {
                let seq = key.value();
                Ok(seq)
            }
            None => Ok(0),
        }
    }

    /// Truncate old replication log entries
    pub fn truncate_replication_log(&self, keep_from: u64) -> Result<u64, DatabaseError> {
        let write_txn = self.begin_write()?;
        let mut deleted = 0u64;
        {
            let mut table = write_txn.open_table(REPLICATION_LOG)?;
            let keys_to_delete: Vec<u64> = table
                .range(..keep_from)?
                .map(|r| r.map(|(k, _)| k.value()))
                .collect::<Result<Vec<_>, _>>()?;

            for key in keys_to_delete {
                table.remove(key)?;
                deleted += 1;
            }
        }
        write_txn.commit()?;
        Ok(deleted)
    }

    // ========================================================================
    // Admin operations
    // ========================================================================

    /// Purge all data (sessions, API keys, replication log) - for testing only
    pub fn purge_all(&self) -> Result<PurgeStats, DatabaseError> {
        let write_txn = self.begin_write()?;
        let mut stats = PurgeStats::default();

        // Clear sessions - collect keys first, then remove
        {
            let table = write_txn.open_table(SESSIONS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(SESSIONS)?;
            for key in keys {
                table.remove(key.as_str())?;
                stats.sessions += 1;
            }
        }

        // Clear resource_sessions index
        {
            let table = write_txn.open_table(RESOURCE_SESSIONS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(RESOURCE_SESSIONS)?;
            for key in keys {
                table.remove(key.as_str())?;
            }
        }

        // Clear API keys
        {
            let table = write_txn.open_table(API_KEYS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(API_KEYS)?;
            for key in keys {
                table.remove(key.as_str())?;
                stats.api_keys += 1;
            }
        }

        // Clear replication log
        {
            let table = write_txn.open_table(REPLICATION_LOG)?;
            let keys: Vec<u64> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(REPLICATION_LOG)?;
            for key in keys {
                table.remove(key)?;
                stats.replication_entries += 1;
            }
        }

        write_txn.commit()?;
        Ok(stats)
    }
}

/// Statistics from a purge operation
#[derive(Debug, Default)]
pub struct PurgeStats {
    pub sessions: u64,
    pub api_keys: u64,
    pub replication_entries: u64,
}
