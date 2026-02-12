use redb::{Database as RedbDatabase, ReadTransaction, ReadableTable, WriteTransaction};
use std::path::Path;
use thiserror::Error;

use super::tables::*;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Commit error: {0}")]
    Commit(Box<redb::CommitError>),
    #[error("Database error: {0}")]
    Redb(Box<redb::Error>),
    #[error("Database error: {0}")]
    RedbDatabase(Box<redb::DatabaseError>),
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] rmp_serde::decode::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] rmp_serde::encode::Error),
    #[error("Storage error: {0}")]
    Storage(Box<redb::StorageError>),
    #[error("Table error: {0}")]
    Table(Box<redb::TableError>),
    #[error("Transaction error: {0}")]
    Transaction(Box<redb::TransactionError>),
}

impl From<redb::CommitError> for DatabaseError {
    fn from(e: redb::CommitError) -> Self {
        DatabaseError::Commit(Box::new(e))
    }
}

impl From<redb::DatabaseError> for DatabaseError {
    fn from(e: redb::DatabaseError) -> Self {
        DatabaseError::RedbDatabase(Box::new(e))
    }
}

impl From<redb::Error> for DatabaseError {
    fn from(e: redb::Error) -> Self {
        DatabaseError::Redb(Box::new(e))
    }
}

impl From<redb::StorageError> for DatabaseError {
    fn from(e: redb::StorageError) -> Self {
        DatabaseError::Storage(Box::new(e))
    }
}

impl From<redb::TableError> for DatabaseError {
    fn from(e: redb::TableError) -> Self {
        DatabaseError::Table(Box::new(e))
    }
}

impl From<redb::TransactionError> for DatabaseError {
    fn from(e: redb::TransactionError) -> Self {
        DatabaseError::Transaction(Box::new(e))
    }
}

pub struct Database {
    db: RedbDatabase,
}

/// Statistics from a purge operation
#[derive(Debug, Default)]
pub struct PurgeStats {
    pub api_keys: u64,
    pub replication_entries: u64,
    pub sessions: u64,
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
            let _ = write_txn.open_table(API_KEY_IDS)?;
            let _ = write_txn.open_table(API_KEYS)?;
            let _ = write_txn.open_table(NODE_META)?;
            let _ = write_txn.open_table(REPLICATION_LOG)?;
            let _ = write_txn.open_table(SESSION_IDS)?;
            let _ = write_txn.open_table(SESSIONS)?;
            let _ = write_txn.open_table(SUBJECT_API_KEYS)?;
            let _ = write_txn.open_table(SUBJECT_SESSIONS)?;
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
    // Admin operations
    // ========================================================================

    /// Purge all data (sessions, API keys, replication log) - for testing only
    pub fn purge_all(&self) -> Result<PurgeStats, DatabaseError> {
        let write_txn = self.begin_write()?;
        let mut stats = PurgeStats::default();

        // Clear API key ID index
        {
            let table = write_txn.open_table(API_KEY_IDS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(API_KEY_IDS)?;
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

        // Clear session ID index
        {
            let table = write_txn.open_table(SESSION_IDS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(SESSION_IDS)?;
            for key in keys {
                table.remove(key.as_str())?;
            }
        }

        // Clear sessions
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

        // Clear subject API keys index
        {
            let table = write_txn.open_table(SUBJECT_API_KEYS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(SUBJECT_API_KEYS)?;
            for key in keys {
                table.remove(key.as_str())?;
            }
        }

        // Clear subject sessions index
        {
            let table = write_txn.open_table(SUBJECT_SESSIONS)?;
            let keys: Vec<String> = table
                .iter()?
                .map(|r| r.map(|(k, _)| k.value().to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            drop(table);

            let mut table = write_txn.open_table(SUBJECT_SESSIONS)?;
            for key in keys {
                table.remove(key.as_str())?;
            }
        }

        write_txn.commit()?;
        Ok(stats)
    }
}
