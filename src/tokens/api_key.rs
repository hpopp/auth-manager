use chrono::{Duration, Utc};
use thiserror::Error;

use crate::storage::models::ApiKey;
use crate::storage::Database;

use super::generator::{generate_api_key, hash_key};

#[derive(Debug, Error)]
pub enum ApiKeyError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("API key not found")]
    NotFound,
    #[error("API key expired")]
    Expired,
}

/// Create a new API key
/// Returns the plaintext key (only shown once) and the stored record
pub fn create(
    db: &Database,
    name: &str,
    expires_in_days: Option<u64>,
) -> Result<(String, ApiKey), ApiKeyError> {
    let key = generate_api_key();
    let key_hash = hash_key(&key);
    let now = Utc::now();

    let api_key = ApiKey {
        id: uuid::Uuid::new_v4().to_string(),
        key_hash: key_hash.clone(),
        name: name.to_string(),
        created_at: now,
        expires_at: expires_in_days.map(|days| now + Duration::days(days as i64)),
    };

    db.put_api_key(&api_key)?;
    tracing::debug!(key_id = %api_key.id, name = %name, "Created API key");

    Ok((key, api_key))
}

/// Validate an API key, returning the record if valid
pub fn validate(db: &Database, key: &str) -> Result<Option<ApiKey>, ApiKeyError> {
    let key_hash = hash_key(key);
    
    match db.get_api_key(&key_hash)? {
        Some(api_key) => {
            if let Some(expires_at) = api_key.expires_at {
                if expires_at < Utc::now() {
                    // Key is expired - delete it and return None
                    let _ = db.delete_api_key(&key_hash);
                    tracing::debug!(key_id = %api_key.id, "API key expired");
                    return Ok(None);
                }
            }
            Ok(Some(api_key))
        }
        None => Ok(None),
    }
}

/// Revoke (delete) an API key
pub fn revoke(db: &Database, key: &str) -> Result<bool, ApiKeyError> {
    let key_hash = hash_key(key);
    let deleted = db.delete_api_key(&key_hash)?;
    if deleted {
        tracing::debug!(key_hash = %key_hash, "Revoked API key");
    }
    Ok(deleted)
}

/// Clean up expired API keys (called by background task)
pub fn cleanup_expired(db: &Database) -> Result<usize, ApiKeyError> {
    let keys = db.get_all_api_keys()?;
    let now = Utc::now();
    let mut cleaned = 0;

    for api_key in keys {
        if let Some(expires_at) = api_key.expires_at {
            if expires_at < now {
                if db.delete_api_key(&api_key.key_hash)? {
                    cleaned += 1;
                }
            }
        }
    }

    if cleaned > 0 {
        tracing::info!(count = cleaned, "Cleaned up expired API keys");
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
    fn test_create_and_validate_api_key() {
        let (db, _temp) = setup_db();
        
        let (key, api_key) = create(&db, "Test Key", None).unwrap();
        assert!(key.starts_with("am_"));
        assert_eq!(api_key.name, "Test Key");
        assert!(api_key.expires_at.is_none());

        let validated = validate(&db, &key).unwrap();
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().id, api_key.id);
    }

    #[test]
    fn test_create_api_key_with_expiration() {
        let (db, _temp) = setup_db();
        
        let (_, api_key) = create(&db, "Expiring Key", Some(30)).unwrap();
        assert!(api_key.expires_at.is_some());
    }

    #[test]
    fn test_revoke_api_key() {
        let (db, _temp) = setup_db();
        
        let (key, _) = create(&db, "Test Key", None).unwrap();
        
        assert!(revoke(&db, &key).unwrap());
        assert!(validate(&db, &key).unwrap().is_none());
    }

    #[test]
    fn test_invalid_key_returns_none() {
        let (db, _temp) = setup_db();
        
        let result = validate(&db, "am_invalid_key_that_does_not_exist").unwrap();
        assert!(result.is_none());
    }
}
