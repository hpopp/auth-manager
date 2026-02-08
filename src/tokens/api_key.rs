use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::storage::models::ApiKey;
use crate::storage::Database;

use super::generator::{generate_api_key, hash_key};

#[derive(Debug, Error)]
pub enum ApiKeyError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("API key expired")]
    Expired,
    #[error("API key not found")]
    NotFound,
}

/// Create a new API key
/// Returns the plaintext key (only shown once) and the stored record
pub fn create(
    db: &Database,
    name: &str,
    subject_id: &str,
    description: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    scopes: Vec<String>,
) -> Result<(String, ApiKey), ApiKeyError> {
    let key = generate_api_key();
    let key_hash = hash_key(&key);
    let now = Utc::now();

    let api_key = ApiKey {
        created_at: now,
        description,
        expires_at,
        id: uuid::Uuid::new_v4().to_string(),
        key_hash: key_hash.clone(),
        name: name.to_string(),
        subject_id: subject_id.to_string(),
        scopes,
    };

    db.put_api_key(&api_key)?;
    tracing::debug!(key_id = %api_key.id, subject_id = %subject_id, name = %name, "Created API key");

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

/// List all API keys for a resource
pub fn list_by_subject(db: &Database, subject_id: &str) -> Result<Vec<ApiKey>, ApiKeyError> {
    let keys = db.get_api_keys_by_subject(subject_id)?;
    let now = Utc::now();

    // Filter out expired keys
    Ok(keys
        .into_iter()
        .filter(|k| k.expires_at.map(|exp| exp > now).unwrap_or(true))
        .collect())
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

        let (key, api_key) = create(&db, "Test Key", "user-123", None, None, vec![]).unwrap();
        assert!(key.starts_with("am_"));
        assert_eq!(api_key.name, "Test Key");
        assert_eq!(api_key.subject_id, "user-123");
        assert!(api_key.expires_at.is_none());
        assert!(api_key.scopes.is_empty());

        let validated = validate(&db, &key).unwrap();
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().id, api_key.id);
    }

    #[test]
    fn test_create_api_key_with_expiration() {
        let (db, _temp) = setup_db();

        let expires_at = Utc::now() + chrono::Duration::days(30);
        let (_, api_key) = create(
            &db,
            "Expiring Key",
            "user-456",
            None,
            Some(expires_at),
            vec![],
        )
        .unwrap();
        assert!(api_key.expires_at.is_some());
    }

    #[test]
    fn test_revoke_api_key() {
        let (db, _temp) = setup_db();

        let (key, _) = create(&db, "Test Key", "user-789", None, None, vec![]).unwrap();

        assert!(revoke(&db, &key).unwrap());
        assert!(validate(&db, &key).unwrap().is_none());
    }

    #[test]
    fn test_list_by_subject() {
        let (db, _temp) = setup_db();

        create(&db, "Key 1", "user-123", None, None, vec![]).unwrap();
        create(&db, "Key 2", "user-123", None, None, vec![]).unwrap();
        create(&db, "Key 3", "user-456", None, None, vec![]).unwrap();

        let keys = list_by_subject(&db, "user-123").unwrap();
        assert_eq!(keys.len(), 2);

        let keys = list_by_subject(&db, "user-456").unwrap();
        assert_eq!(keys.len(), 1);

        let keys = list_by_subject(&db, "user-999").unwrap();
        assert_eq!(keys.len(), 0);
    }

    #[test]
    fn test_invalid_key_returns_none() {
        let (db, _temp) = setup_db();

        let result = validate(&db, "am_invalid_key_that_does_not_exist").unwrap();
        assert!(result.is_none());
    }
}
