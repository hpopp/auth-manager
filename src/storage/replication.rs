use redb::ReadableTable;

use super::db::{Database, DatabaseError};
use super::models::{ReplicatedWrite, WriteOp};
use super::tables::*;

impl Database {
    // ========================================================================
    // Replication log operations
    // ========================================================================

    /// Append a write to the replication log
    pub fn append_replication_log(&self, write: &ReplicatedWrite) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(REPLICATION_LOG)?;
            let data = rmp_serde::to_vec(write)?;
            table.insert(write.sequence, data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Atomically read the latest sequence, increment it, and append the
    /// new entry — all in a single write transaction. This prevents the
    /// TOCTOU race where two concurrent callers read the same sequence
    /// and silently overwrite each other's log entries.
    pub fn append_next_replication_log(
        &self,
        operation: WriteOp,
    ) -> Result<ReplicatedWrite, DatabaseError> {
        let write_txn = self.begin_write()?;
        let write = {
            let mut table = write_txn.open_table(REPLICATION_LOG)?;

            let previous_sequence = match table.last()? {
                Some((key, _)) => key.value(),
                None => 0,
            };
            let sequence = previous_sequence + 1;

            let entry = ReplicatedWrite {
                sequence,
                operation,
                timestamp: chrono::Utc::now(),
            };
            let data = rmp_serde::to_vec(&entry)?;
            table.insert(sequence, data.as_slice())?;
            entry
        };
        write_txn.commit()?;
        Ok(write)
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
            let entry: ReplicatedWrite = rmp_serde::from_slice(value.value())?;
            entries.push(entry);
        }

        Ok(entries)
    }
}
