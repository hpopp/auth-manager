use chrono::{Duration, Utc};
use thiserror::Error;

use crate::storage::models::{DeviceInfo, SessionToken};
use crate::storage::Database;

use super::generator::generate_token;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("Session expired")]
    Expired,
    #[error("Session not found")]
    NotFound,
}

/// Create a new session token
pub fn create(
    db: &Database,
    subject_id: &str,
    device_info: DeviceInfo,
    ttl_seconds: u64,
) -> Result<SessionToken, SessionError> {
    let now = Utc::now();
    let session = SessionToken {
        created_at: now,
        device_info,
        expires_at: now + Duration::seconds(ttl_seconds as i64),
        id: uuid::Uuid::new_v4().to_string(),
        ip_address: None,
        last_used_at: None,
        subject_id: subject_id.to_string(),
        token: generate_token(),
    };

    db.put_session(&session)?;
    tracing::debug!(id = %session.id, subject_id = %subject_id, "Created session token");

    Ok(session)
}

/// Validate a session token, returning it if valid
pub fn validate(db: &Database, token: &str) -> Result<Option<SessionToken>, SessionError> {
    match db.get_session(token)? {
        Some(session) => {
            if session.is_expired_at(Utc::now()) {
                // Token is expired - delete it and return None
                if let Err(e) = db.delete_session(token) {
                    tracing::warn!(error = %e, id = %session.id, "Failed to delete expired session");
                }
                tracing::debug!(id = %session.id, "Session token expired");
                Ok(None)
            } else {
                // Update last_used_at (local-only, best-effort)
                if let Err(e) = db.touch_session(token) {
                    tracing::warn!(error = %e, id = %session.id, "Failed to update session last_used_at");
                }
                Ok(Some(session))
            }
        }
        None => Ok(None),
    }
}

/// Revoke (delete) a session token by its secret token value
pub fn revoke(db: &Database, token: &str) -> Result<bool, SessionError> {
    let deleted = db.delete_session(token)?;
    if deleted {
        tracing::debug!(token = %token, "Revoked session token");
    }
    Ok(deleted)
}

/// List all sessions for a resource
pub fn list_by_subject(db: &Database, subject_id: &str) -> Result<Vec<SessionToken>, SessionError> {
    let sessions = db.get_sessions_by_subject(subject_id)?;
    let now = Utc::now();

    // Filter out expired sessions
    Ok(sessions
        .into_iter()
        .filter(|s| !s.is_expired_at(now))
        .collect())
}

/// Clean up expired sessions (called by background task)
pub fn cleanup_expired(db: &Database) -> Result<usize, SessionError> {
    let sessions = db.get_all_sessions()?;
    let now = Utc::now();
    let mut cleaned = 0;

    for session in sessions {
        if session.is_expired_at(now) && db.delete_session(&session.token)? {
            cleaned += 1;
        }
    }

    if cleaned > 0 {
        tracing::info!(count = cleaned, "Cleaned up expired sessions");
    }

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::setup_db;

    #[test]
    fn test_create_and_validate_session() {
        let (db, _temp) = setup_db();

        let session = create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        assert!(!session.id.is_empty());
        assert!(!session.token.is_empty());
        assert_ne!(session.id, session.token);
        assert_eq!(session.subject_id, "user123");

        let validated = validate(&db, &session.token).unwrap();
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().id, session.id);
    }

    #[test]
    fn test_revoke_session() {
        let (db, _temp) = setup_db();

        let session = create(&db, "user123", DeviceInfo::default(), 3600).unwrap();

        assert!(revoke(&db, &session.token).unwrap());
        assert!(validate(&db, &session.token).unwrap().is_none());
    }

    #[test]
    fn test_list_by_subject() {
        let (db, _temp) = setup_db();

        create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        create(&db, "user456", DeviceInfo::default(), 3600).unwrap();

        let sessions = list_by_subject(&db, "user123").unwrap();
        assert_eq!(sessions.len(), 2);

        let sessions = list_by_subject(&db, "user456").unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn test_list_by_subject_pagination() {
        let (db, _temp) = setup_db();

        // Create 5 sessions
        for _ in 0..5 {
            create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        }

        let all = list_by_subject(&db, "user123").unwrap();
        assert_eq!(all.len(), 5);

        // Simulate pagination: limit=2, offset=0
        let page: Vec<_> = all.iter().skip(0).take(2).collect();
        assert_eq!(page.len(), 2);

        // limit=2, offset=2
        let page: Vec<_> = all.iter().skip(2).take(2).collect();
        assert_eq!(page.len(), 2);

        // limit=2, offset=4 (last page, partial)
        let page: Vec<_> = all.iter().skip(4).take(2).collect();
        assert_eq!(page.len(), 1);

        // offset beyond total
        let page: Vec<_> = all.iter().skip(10).take(2).collect();
        assert_eq!(page.len(), 0);
    }
}
