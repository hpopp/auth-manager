use chrono::Utc;
use thiserror::Error;

use crate::storage::models::SessionToken;
use crate::storage::Database;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("Session expired")]
    Expired,
    #[error("Session not found")]
    NotFound,
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

/// List sessions, optionally filtered by subject_id
pub fn list(db: &Database, subject_id: Option<&str>) -> Result<Vec<SessionToken>, SessionError> {
    let sessions = match subject_id {
        Some(id) => db.get_sessions_by_subject(id)?,
        None => db.get_all_sessions()?,
    };
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
    use crate::testutil::{make_session, setup_db};

    #[test]
    fn test_store_and_validate_session() {
        let (db, _temp) = setup_db();

        let session = make_session("s1", "user123");
        db.put_session(&session).unwrap();

        let validated = validate(&db, &session.token).unwrap();
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().id, session.id);
    }

    #[test]
    fn test_revoke_session() {
        let (db, _temp) = setup_db();

        let session = make_session("s1", "user123");
        db.put_session(&session).unwrap();

        assert!(db.delete_session(&session.token).unwrap());
        assert!(validate(&db, &session.token).unwrap().is_none());
    }

    #[test]
    fn test_list_by_subject() {
        let (db, _temp) = setup_db();

        for (id, subject) in [("s1", "user123"), ("s2", "user123"), ("s3", "user456")] {
            db.put_session(&make_session(id, subject)).unwrap();
        }

        assert_eq!(list(&db, Some("user123")).unwrap().len(), 2);
        assert_eq!(list(&db, Some("user456")).unwrap().len(), 1);
    }

    #[test]
    fn test_list_all() {
        let (db, _temp) = setup_db();

        for (id, subject) in [("s1", "user123"), ("s2", "user123"), ("s3", "user456")] {
            db.put_session(&make_session(id, subject)).unwrap();
        }

        assert_eq!(list(&db, None).unwrap().len(), 3);
    }

    #[test]
    fn test_list_pagination() {
        let (db, _temp) = setup_db();

        for i in 0..5 {
            db.put_session(&make_session(&format!("s{i}"), "user123"))
                .unwrap();
        }

        let all = list(&db, Some("user123")).unwrap();
        assert_eq!(all.len(), 5);

        assert_eq!(all.iter().skip(0).take(2).count(), 2);
        assert_eq!(all.iter().skip(2).take(2).count(), 2);
        assert_eq!(all.iter().skip(4).take(2).count(), 1);
        assert_eq!(all.iter().skip(10).take(2).count(), 0);
    }
}
