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
    resource_id: &str,
    device_info: DeviceInfo,
    ttl_seconds: u64,
) -> Result<SessionToken, SessionError> {
    let now = Utc::now();
    let session = SessionToken {
        id: generate_token(),
        resource_id: resource_id.to_string(),
        created_at: now,
        expires_at: now + Duration::seconds(ttl_seconds as i64),
        device_info,
    };

    db.put_session(&session)?;
    tracing::debug!(token_id = %session.id, resource_id = %resource_id, "Created session token");
    
    Ok(session)
}

/// Validate a session token, returning it if valid
pub fn validate(db: &Database, token_id: &str) -> Result<Option<SessionToken>, SessionError> {
    match db.get_session(token_id)? {
        Some(session) => {
            if session.expires_at < Utc::now() {
                // Token is expired - delete it and return None
                let _ = db.delete_session(token_id);
                tracing::debug!(token_id = %token_id, "Session token expired");
                Ok(None)
            } else {
                Ok(Some(session))
            }
        }
        None => Ok(None),
    }
}

/// Revoke (delete) a session token
pub fn revoke(db: &Database, token_id: &str) -> Result<bool, SessionError> {
    let deleted = db.delete_session(token_id)?;
    if deleted {
        tracing::debug!(token_id = %token_id, "Revoked session token");
    }
    Ok(deleted)
}

/// List all sessions for a resource
pub fn list_by_resource(db: &Database, resource_id: &str) -> Result<Vec<SessionToken>, SessionError> {
    let sessions = db.get_sessions_by_resource(resource_id)?;
    let now = Utc::now();
    
    // Filter out expired sessions
    Ok(sessions
        .into_iter()
        .filter(|s| s.expires_at > now)
        .collect())
}

/// Clean up expired sessions (called by background task)
pub fn cleanup_expired(db: &Database) -> Result<usize, SessionError> {
    let sessions = db.get_all_sessions()?;
    let now = Utc::now();
    let mut cleaned = 0;

    for session in sessions {
        if session.expires_at < now {
            if db.delete_session(&session.id)? {
                cleaned += 1;
            }
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
    use tempfile::TempDir;

    fn setup_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        (db, temp_dir)
    }

    #[test]
    fn test_create_and_validate_session() {
        let (db, _temp) = setup_db();
        
        let session = create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        assert!(!session.id.is_empty());
        assert_eq!(session.resource_id, "user123");

        let validated = validate(&db, &session.id).unwrap();
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().id, session.id);
    }

    #[test]
    fn test_revoke_session() {
        let (db, _temp) = setup_db();
        
        let session = create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        
        assert!(revoke(&db, &session.id).unwrap());
        assert!(validate(&db, &session.id).unwrap().is_none());
    }

    #[test]
    fn test_list_by_resource() {
        let (db, _temp) = setup_db();
        
        create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        create(&db, "user123", DeviceInfo::default(), 3600).unwrap();
        create(&db, "user456", DeviceInfo::default(), 3600).unwrap();

        let sessions = list_by_resource(&db, "user123").unwrap();
        assert_eq!(sessions.len(), 2);

        let sessions = list_by_resource(&db, "user456").unwrap();
        assert_eq!(sessions.len(), 1);
    }
}
