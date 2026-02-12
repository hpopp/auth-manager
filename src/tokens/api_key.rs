use chrono::Utc;
use thiserror::Error;

use crate::storage::models::ApiKey;
use crate::storage::Database;

use super::generator::hash_key;

#[derive(Debug, Error)]
pub enum ApiKeyError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("API key expired")]
    Expired,
    #[error("API key not found")]
    NotFound,
}

/// Validate an API key, returning the record if valid
pub fn validate(db: &Database, key: &str) -> Result<Option<ApiKey>, ApiKeyError> {
    let key_hash = hash_key(key);

    match db.get_api_key(&key_hash)? {
        Some(api_key) => {
            if api_key.is_expired_at(Utc::now()) {
                // Key is expired - delete it and return None
                if let Err(e) = db.delete_api_key(&key_hash) {
                    tracing::warn!(error = %e, key_id = %api_key.id, "Failed to delete expired API key");
                }
                tracing::debug!(key_id = %api_key.id, "API key expired");
                return Ok(None);
            }
            // Update last_used_at (local-only, best-effort)
            if let Err(e) = db.touch_api_key(&key_hash) {
                tracing::warn!(error = %e, key_id = %api_key.id, "Failed to update API key last_used_at");
            }
            Ok(Some(api_key))
        }
        None => Ok(None),
    }
}

/// List API keys, optionally filtered by subject_id
pub fn list(db: &Database, subject_id: Option<&str>) -> Result<Vec<ApiKey>, ApiKeyError> {
    let keys = match subject_id {
        Some(id) => db.get_api_keys_by_subject(id)?,
        None => db.get_all_api_keys()?,
    };
    let now = Utc::now();

    // Filter out expired keys
    Ok(keys.into_iter().filter(|k| !k.is_expired_at(now)).collect())
}

/// Clean up expired API keys (called by background task)
pub fn cleanup_expired(db: &Database) -> Result<usize, ApiKeyError> {
    let cleaned = db.delete_expired_api_keys()?;

    if cleaned > 0 {
        tracing::info!(count = cleaned, "Cleaned up expired API keys");
    }

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{make_api_key, setup_db};

    #[test]
    fn test_store_and_validate_api_key() {
        let (db, _temp) = setup_db();

        let api_key = make_api_key("k1", "user-123");
        db.put_api_key(&api_key).unwrap();

        let fetched = db.get_api_key(&api_key.key_hash).unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, api_key.id);
    }

    #[test]
    fn test_revoke_api_key() {
        let (db, _temp) = setup_db();

        let api_key = make_api_key("k1", "user-789");
        db.put_api_key(&api_key).unwrap();

        assert!(db.delete_api_key(&api_key.key_hash).unwrap());
        assert!(db.get_api_key(&api_key.key_hash).unwrap().is_none());
    }

    #[test]
    fn test_list_by_subject() {
        let (db, _temp) = setup_db();

        for (id, subject) in [("k1", "user-123"), ("k2", "user-123"), ("k3", "user-456")] {
            db.put_api_key(&make_api_key(id, subject)).unwrap();
        }

        assert_eq!(list(&db, Some("user-123")).unwrap().len(), 2);
        assert_eq!(list(&db, Some("user-456")).unwrap().len(), 1);
        assert_eq!(list(&db, Some("user-999")).unwrap().len(), 0);
    }

    #[test]
    fn test_list_all() {
        let (db, _temp) = setup_db();

        for (id, subject) in [("k1", "user-123"), ("k2", "user-123"), ("k3", "user-456")] {
            db.put_api_key(&make_api_key(id, subject)).unwrap();
        }

        assert_eq!(list(&db, None).unwrap().len(), 3);
    }

    #[test]
    fn test_invalid_key_returns_none() {
        let (db, _temp) = setup_db();

        let result = validate(&db, "invalid_key_that_does_not_exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_pagination() {
        let (db, _temp) = setup_db();

        for i in 0..5 {
            db.put_api_key(&make_api_key(&format!("k{i}"), "user-123"))
                .unwrap();
        }

        let all = list(&db, Some("user-123")).unwrap();
        assert_eq!(all.len(), 5);

        assert_eq!(all.iter().skip(0).take(2).count(), 2);
        assert_eq!(all.iter().skip(2).take(2).count(), 2);
        assert_eq!(all.iter().skip(4).take(2).count(), 1);
        assert_eq!(all.iter().skip(10).take(2).count(), 0);
    }
}
