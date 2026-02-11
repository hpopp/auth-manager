pub(crate) mod catchup;
pub mod discovery;
mod election;
pub(crate) mod handlers;
mod heartbeat;
mod node;
mod replication;
pub mod rpc;
pub mod server;
pub mod transport;

pub use discovery::Discovery;
pub use node::{ClusterState, PeerState, Role};
pub use replication::{replicate_write, ReplicationError};
pub use transport::ClusterTransport;

use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

use crate::AppState;

/// Guard that checks the current node is the leader.
pub(crate) async fn ensure_leader(state: &AppState) -> Result<(), ReplicationError> {
    let cluster = state.cluster.read().await;
    if cluster.role != Role::Leader {
        return Err(ReplicationError::NotLeader);
    }
    Ok(())
}

/// Start cluster-related background tasks (heartbeat, election, discovery)
pub fn start_cluster_tasks(state: Arc<AppState>, discovery: Option<Discovery>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let heartbeat_handle = heartbeat::start_heartbeat_task(Arc::clone(&state));
        let election_handle = election::start_election_monitor(Arc::clone(&state));

        if let Some(disc) = discovery {
            let discovery_handle = start_discovery_task(Arc::clone(&state), disc);

            tokio::select! {
                _ = heartbeat_handle => {
                    tracing::error!("Heartbeat task ended unexpectedly");
                }
                _ = election_handle => {
                    tracing::error!("Election monitor ended unexpectedly");
                }
                _ = discovery_handle => {
                    tracing::error!("Discovery task ended unexpectedly");
                }
            }
        } else {
            tokio::select! {
                _ = heartbeat_handle => {
                    tracing::error!("Heartbeat task ended unexpectedly");
                }
                _ = election_handle => {
                    tracing::error!("Election monitor ended unexpectedly");
                }
            }
        }
    })
}

/// Background task that periodically discovers peers via the configured strategy
fn start_discovery_task(state: Arc<AppState>, discovery: Discovery) -> JoinHandle<()> {
    let poll_interval = Duration::from_secs(state.config.cluster.discovery.poll_interval_seconds);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);

        loop {
            interval.tick().await;

            match discovery.discover_peers().await {
                Ok(peers) => {
                    let mut cluster = state.cluster.write().await;
                    cluster.update_discovered_peers(peers);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Peer discovery failed");
                }
            }
        }
    })
}
