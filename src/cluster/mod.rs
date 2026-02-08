mod catchup;
mod election;
mod heartbeat;
mod node;
mod replication;

pub use node::{ClusterState, PeerState, Role};
pub use replication::{replicate_write, ReplicationError};

use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::AppState;

/// Start cluster-related background tasks (heartbeat, election monitoring)
pub fn start_cluster_tasks(state: Arc<AppState>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let heartbeat_handle = heartbeat::start_heartbeat_task(Arc::clone(&state));
        let election_handle = election::start_election_monitor(Arc::clone(&state));
        
        // Wait for either task to complete (which shouldn't happen normally)
        tokio::select! {
            _ = heartbeat_handle => {
                tracing::error!("Heartbeat task ended unexpectedly");
            }
            _ = election_handle => {
                tracing::error!("Election monitor ended unexpectedly");
            }
        }
    })
}
