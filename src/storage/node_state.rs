use super::db::{Database, DatabaseError};
use super::models::NodeState;
use super::tables::*;

impl Database {
    // ========================================================================
    // Node state operations
    // ========================================================================

    /// Get the node state
    pub fn get_node_state(&self) -> Result<Option<NodeState>, DatabaseError> {
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(NODE_META)?;

        match table.get("state")? {
            Some(data) => {
                let state: NodeState = rmp_serde::from_slice(data.value())?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// Save the node state
    pub fn put_node_state(&self, state: &NodeState) -> Result<(), DatabaseError> {
        let write_txn = self.begin_write()?;
        {
            let mut table = write_txn.open_table(NODE_META)?;
            let data = rmp_serde::to_vec(state)?;
            table.insert("state", data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}
