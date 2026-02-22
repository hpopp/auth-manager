use redb::ReadableTable;

use super::db::{expiry_key, Database, DatabaseError};
use super::models::ApiKey;
use super::tables::*;

impl Database {
    // ========================================================================
    // API key operations
    // ========================================================================

    /// Delete an API key
    pub fn delete_api_key(&self, key_hash: &str) -> Result<bool, DatabaseError> {
        let write_txn = self.begin_write()?;

        // First, get the API key for index cleanup
        let api_key: Option<ApiKey> = {
            let table = write_txn.open_table(API_KEYS)?;
            let result = table.get(key_hash)?;
            match result {
                Some(data) => Some(rmp_serde::from_slice(data.value())?),
                None => None,
            }
        };

        let deleted = match api_key {
            Some(api_key) => {
                // Remove from api_keys table
                {
                    let mut table = write_txn.open_table(API_KEYS)?;
                    table.remove(key_hash)?;
                }

                // Update subject_api_keys index
                let key_hashes: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                    let result = index_table.get(api_key.subject_id.as_str())?;
                    match result {
                        Some(data) => Some(rmp_serde::from_slice(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut hashes) = key_hashes {
                    hashes.retain(|h| h != key_hash);
                    let mut index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                    if hashes.is_empty() {
                        index_table.remove(api_key.subject_id.as_str())?;
                    } else {
                        let new_index_data = rmp_serde::to_vec_named(&hashes)?;
                        index_table
                            .insert(api_key.subject_id.as_str(), new_index_data.as_slice())?;
                    }
                }

                // Remove from API key ID index
                {
                    let mut id_table = write_txn.open_table(API_KEY_IDS)?;
                    id_table.remove(api_key.id.as_str())?;
                }

                // Remove from expiration index (if the key has an expiry)
                if let Some(ref expires_at) = api_key.expires_at {
                    let mut expiry_table = write_txn.open_table(API_KEY_EXPIRY)?;
                    let ek = expiry_key(expires_at, key_hash);
                    expiry_table.remove(ek.as_str())?;
                }

                true
            }
            None => false,
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Delete expired API keys using the expiration index (no full table scan).
    pub fn delete_expired_api_keys(&self) -> Result<usize, DatabaseError> {
        let now = chrono::Utc::now();
        let now_ms = now.timestamp_millis();

        // Phase 1: read the expiration index to collect expired entries
        let expired: Vec<(String, String)> = {
            let read_txn = self.begin_read()?;
            let table = read_txn.open_table(API_KEY_EXPIRY)?;
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

        // Phase 2: delete expired API keys and clean up all indexes
        let write_txn = self.begin_write()?;

        for (expiry_key_val, key_hash) in &expired {
            let api_key: Option<ApiKey> = {
                let table = write_txn.open_table(API_KEYS)?;
                let result = table.get(key_hash.as_str())?;
                match result {
                    Some(data) => Some(rmp_serde::from_slice(data.value())?),
                    None => None,
                }
            };

            if let Some(api_key) = api_key {
                {
                    let mut table = write_txn.open_table(API_KEYS)?;
                    table.remove(key_hash.as_str())?;
                }

                // Clean up subject_api_keys index
                let hashes: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                    let result = index_table.get(api_key.subject_id.as_str())?;
                    match result {
                        Some(data) => Some(rmp_serde::from_slice(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut h) = hashes {
                    h.retain(|v| v != key_hash);
                    let mut index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                    if h.is_empty() {
                        index_table.remove(api_key.subject_id.as_str())?;
                    } else {
                        let new_index_data = rmp_serde::to_vec_named(&h)?;
                        index_table
                            .insert(api_key.subject_id.as_str(), new_index_data.as_slice())?;
                    }
                }

                {
                    let mut id_table = write_txn.open_table(API_KEY_IDS)?;
                    id_table.remove(api_key.id.as_str())?;
                }
            }

            // Remove from expiration index
            {
                let mut expiry_table = write_txn.open_table(API_KEY_EXPIRY)?;
                expiry_table.remove(expiry_key_val.as_str())?;
            }
        }

        write_txn.commit()?;
        Ok(expired.len())
    }

    /// Get all API keys (for expiration cleanup)
    pub fn get_all_api_keys(&self) -> Result<Vec<ApiKey>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(API_KEYS)?;

        let mut keys = Vec::new();
        for result in table.iter()? {
            let (_, value) = result?;
            let api_key: ApiKey = rmp_serde::from_slice(value.value())?;
            keys.push(api_key);
        }

        Ok(keys)
    }

    /// Get an API key by hash
    pub fn get_api_key(&self, key_hash: &str) -> Result<Option<ApiKey>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(API_KEYS)?;

        match table.get(key_hash)? {
            Some(data) => {
                let api_key: ApiKey = rmp_serde::from_slice(data.value())?;
                Ok(Some(api_key))
            }
            None => Ok(None),
        }
    }

    /// Get an API key by its UUID (resolves ID -> key_hash -> api_key)
    pub fn get_api_key_by_id(&self, id: &str) -> Result<Option<ApiKey>, DatabaseError> {
        let key_hash = match self.get_api_key_hash_by_id(id)? {
            Some(h) => h,
            None => return Ok(None),
        };
        self.get_api_key(&key_hash)
    }

    /// Resolve an API key UUID to its key_hash
    pub fn get_api_key_hash_by_id(&self, id: &str) -> Result<Option<String>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(API_KEY_IDS)?;

        match table.get(id)? {
            Some(data) => Ok(Some(data.value().to_string())),
            None => Ok(None),
        }
    }

    /// Get all API keys for a resource
    pub fn get_api_keys_by_subject(&self, subject_id: &str) -> Result<Vec<ApiKey>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let index_table = read_txn.open_table(SUBJECT_API_KEYS)?;
        let keys_table = read_txn.open_table(API_KEYS)?;

        let key_hashes: Vec<String> = match index_table.get(subject_id)? {
            Some(data) => rmp_serde::from_slice(data.value())?,
            None => return Ok(Vec::new()),
        };

        let mut api_keys = Vec::new();
        for key_hash in key_hashes {
            if let Some(data) = keys_table.get(key_hash.as_str())? {
                let api_key: ApiKey = rmp_serde::from_slice(data.value())?;
                api_keys.push(api_key);
            }
        }

        Ok(api_keys)
    }

    /// Store an API key
    pub fn put_api_key(&self, api_key: &ApiKey) -> Result<(), DatabaseError> {
        debug_assert!(
            !api_key.key_hash.is_empty(),
            "API key hash must not be empty"
        );
        debug_assert!(!api_key.id.is_empty(), "API key id must not be empty");
        debug_assert!(
            !api_key.subject_id.is_empty(),
            "API key subject_id must not be empty"
        );

        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(API_KEYS)?;
            let data = rmp_serde::to_vec_named(api_key)?;
            table.insert(api_key.key_hash.as_str(), data.as_slice())?;

            // Update resource_api_keys index
            let mut index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
            let mut key_hashes: Vec<String> = index_table
                .get(api_key.subject_id.as_str())?
                .map(|v| rmp_serde::from_slice(v.value()))
                .transpose()?
                .unwrap_or_default();

            if !key_hashes.contains(&api_key.key_hash) {
                key_hashes.push(api_key.key_hash.clone());
                let index_data = rmp_serde::to_vec_named(&key_hashes)?;
                index_table.insert(api_key.subject_id.as_str(), index_data.as_slice())?;
            }

            // Update API key ID index (UUID -> key_hash)
            let mut id_table = write_txn.open_table(API_KEY_IDS)?;
            id_table.insert(api_key.id.as_str(), api_key.key_hash.as_str())?;

            // Update expiration index (only for keys with an expiry)
            if let Some(ref expires_at) = api_key.expires_at {
                let mut expiry_table = write_txn.open_table(API_KEY_EXPIRY)?;
                let ek = expiry_key(expires_at, &api_key.key_hash);
                expiry_table.insert(ek.as_str(), api_key.key_hash.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update last_used_at for an API key (local-only, no replication)
    pub fn touch_api_key(&self, key_hash: &str) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        let existing = {
            let table = write_txn.open_table(API_KEYS)?;
            let result = match table.get(key_hash)? {
                Some(data) => Some(rmp_serde::from_slice::<ApiKey>(data.value())?),
                None => None,
            };
            result
        };
        if let Some(mut api_key) = existing {
            api_key.last_used_at = Some(chrono::Utc::now());
            let serialized = rmp_serde::to_vec_named(&api_key)?;
            let mut table = write_txn.open_table(API_KEYS)?;
            table.insert(key_hash, serialized.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update an API key's mutable fields
    pub fn update_api_key(
        &self,
        key_hash: &str,
        name: Option<&str>,
        description: Option<Option<&str>>,
        scopes: Option<&[String]>,
    ) -> Result<bool, DatabaseError> {
        let write_txn = self.begin_write()?;

        // Read the existing key first
        let existing = {
            let table = write_txn.open_table(API_KEYS)?;
            let result = match table.get(key_hash)? {
                Some(data) => {
                    let api_key: ApiKey = rmp_serde::from_slice(data.value())?;
                    Some(api_key)
                }
                None => None,
            };
            result
        };

        let updated = match existing {
            Some(mut api_key) => {
                if let Some(n) = name {
                    api_key.name = n.to_string();
                }
                if let Some(d) = description {
                    api_key.description = d.map(|s| s.to_string());
                }
                if let Some(s) = scopes {
                    api_key.scopes = s.to_vec();
                }
                api_key.updated_at = Some(chrono::Utc::now());

                let serialized = rmp_serde::to_vec_named(&api_key)?;
                let mut table = write_txn.open_table(API_KEYS)?;
                table.insert(key_hash, serialized.as_slice())?;
                true
            }
            None => false,
        };

        write_txn.commit()?;
        Ok(updated)
    }
}
