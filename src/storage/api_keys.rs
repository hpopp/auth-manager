use redb::ReadableTable;

use super::db::{Database, DatabaseError};
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
        let api_key_info: Option<(String, String)> = {
            let table = write_txn.open_table(API_KEYS)?;
            let result = table.get(key_hash)?;
            match result {
                Some(data) => {
                    let api_key: ApiKey = rmp_serde::from_slice(data.value())?;
                    Some((api_key.subject_id, api_key.id))
                }
                None => None,
            }
        };

        let deleted = match api_key_info {
            Some((subject_id, api_key_id)) => {
                // Remove from api_keys table
                {
                    let mut table = write_txn.open_table(API_KEYS)?;
                    table.remove(key_hash)?;
                }

                // Update resource_api_keys index
                let key_hashes: Option<Vec<String>> = {
                    let index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                    let result = index_table.get(subject_id.as_str())?;
                    match result {
                        Some(data) => Some(rmp_serde::from_slice(data.value())?),
                        None => None,
                    }
                };

                if let Some(mut hashes) = key_hashes {
                    hashes.retain(|h| h != key_hash);
                    let mut index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                    if hashes.is_empty() {
                        index_table.remove(subject_id.as_str())?;
                    } else {
                        let new_index_data = rmp_serde::to_vec(&hashes)?;
                        index_table.insert(subject_id.as_str(), new_index_data.as_slice())?;
                    }
                }

                // Remove from API key ID index
                {
                    let mut id_table = write_txn.open_table(API_KEY_IDS)?;
                    id_table.remove(api_key_id.as_str())?;
                }

                true
            }
            None => false,
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Scan all API keys, delete expired ones in a single write transaction.
    pub fn delete_expired_api_keys(&self) -> Result<usize, DatabaseError> {
        let now = chrono::Utc::now();

        // Phase 1: read-only scan to collect expired key metadata
        let expired: Vec<(String, String, String)> = {
            let read_txn = self.begin_read()?;
            let table = read_txn.open_table(API_KEYS)?;
            let mut result = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let api_key: ApiKey = rmp_serde::from_slice(value.value())?;
                if api_key.is_expired_at(now) {
                    result.push((api_key.key_hash, api_key.subject_id, api_key.id));
                }
            }
            result
        };

        if expired.is_empty() {
            return Ok(0);
        }

        // Phase 2: single write transaction to delete all expired entries
        let write_txn = self.begin_write()?;

        for (key_hash, subject_id, api_key_id) in &expired {
            {
                let mut table = write_txn.open_table(API_KEYS)?;
                table.remove(key_hash.as_str())?;
            }

            let hashes_in_index: Option<Vec<String>> = {
                let index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                let result = index_table.get(subject_id.as_str())?;
                match result {
                    Some(data) => Some(rmp_serde::from_slice(data.value())?),
                    None => None,
                }
            };

            if let Some(mut hashes) = hashes_in_index {
                hashes.retain(|h| h != key_hash);
                let mut index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
                if hashes.is_empty() {
                    index_table.remove(subject_id.as_str())?;
                } else {
                    let new_index_data = rmp_serde::to_vec(&hashes)?;
                    index_table.insert(subject_id.as_str(), new_index_data.as_slice())?;
                }
            }

            {
                let mut id_table = write_txn.open_table(API_KEY_IDS)?;
                id_table.remove(api_key_id.as_str())?;
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
            let data = rmp_serde::to_vec(api_key)?;
            table.insert(api_key.key_hash.as_str(), data.as_slice())?;

            // Update resource_api_keys index
            let mut index_table = write_txn.open_table(SUBJECT_API_KEYS)?;
            let mut key_hashes: Vec<String> = index_table
                .get(api_key.subject_id.as_str())?
                .map(|v| rmp_serde::from_slice(v.value()).unwrap_or_default())
                .unwrap_or_default();

            if !key_hashes.contains(&api_key.key_hash) {
                key_hashes.push(api_key.key_hash.clone());
                let index_data = rmp_serde::to_vec(&key_hashes)?;
                index_table.insert(api_key.subject_id.as_str(), index_data.as_slice())?;
            }

            // Update API key ID index (UUID -> key_hash)
            let mut id_table = write_txn.open_table(API_KEY_IDS)?;
            id_table.insert(api_key.id.as_str(), api_key.key_hash.as_str())?;
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
            let serialized = rmp_serde::to_vec(&api_key)?;
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

                let serialized = rmp_serde::to_vec(&api_key)?;
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
