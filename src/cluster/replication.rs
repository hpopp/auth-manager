//! Quorum-based replication for write operations

use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, warn};

use super::rpc::{ClusterMessage, ReplicateRequest};
use crate::storage::models::WriteOp;
use crate::AppState;

#[derive(Debug, Error)]
pub enum ReplicationError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("Failed to reach quorum")]
    NoQuorum,
    #[error("Not the leader")]
    NotLeader,
}

/// Replicate a write operation to the cluster
/// Returns only after quorum is reached
pub async fn replicate_write(
    state: Arc<AppState>,
    operation: WriteOp,
) -> Result<u64, ReplicationError> {
    // No replication needed when single-node.
    if state.config.is_single_node() {
        let entry = state.db.append_next_replication_log(operation)?;
        return Ok(entry.sequence);
    }

    super::ensure_leader(&state).await?;

    // Append to local log atomically (read latest sequence + write in one transaction)
    let log_entry = state.db.append_next_replication_log(operation.clone())?;
    let sequence = log_entry.sequence;

    let info = gather_cluster_info(&state).await;

    debug_assert!(
        info.quorum_size > 0 && info.quorum_size <= info.peers.len() + 1,
        "quorum size must be between 1 and cluster size"
    );

    let handles = fan_out_replicate(&state, &info, sequence, &operation);
    await_quorum(&state, handles, info.quorum_size, sequence).await
}

/// Snapshot of cluster state needed for replication fan-out.
struct ClusterSnapshot {
    peers: Vec<(String, String)>,
    leader_id: String,
    term: u64,
    quorum_size: usize,
}

/// Grab a read lock and snapshot the cluster info needed for replication.
async fn gather_cluster_info(state: &AppState) -> ClusterSnapshot {
    let cluster = state.cluster.read().await;
    ClusterSnapshot {
        peers: cluster
            .peer_states
            .iter()
            .map(|(id, p)| (id.clone(), p.address.clone()))
            .collect(),
        leader_id: state.config.node.id.clone(),
        term: cluster.current_term,
        quorum_size: cluster.quorum_size(),
    }
}

/// Spawn a replication task for each peer. Returns join handles that
/// resolve to `Some(peer_id)` on successful ack, `None` otherwise.
fn fan_out_replicate(
    state: &AppState,
    info: &ClusterSnapshot,
    sequence: u64,
    operation: &WriteOp,
) -> Vec<tokio::task::JoinHandle<Option<String>>> {
    info.peers
        .iter()
        .map(|(peer_id, peer_addr)| {
            let op = operation.clone();
            let lid = info.leader_id.clone();
            let term = info.term;
            let peer_id = peer_id.clone();
            let peer_addr = peer_addr.clone();
            let transport = Arc::clone(&state.transport);

            tokio::spawn(async move {
                match send_replicate(&transport, &peer_addr, &lid, term, sequence, op).await {
                    Ok(true) => Some(peer_id),
                    Ok(false) => None,
                    Err(e) => {
                        debug!(peer = %peer_id, error = %e, "Replication failed");
                        None
                    }
                }
            })
        })
        .collect()
}

/// Wait for enough acks to reach quorum. Updates the cluster sequence
/// once quorum is achieved.
async fn await_quorum(
    state: &AppState,
    handles: Vec<tokio::task::JoinHandle<Option<String>>>,
    quorum_size: usize,
    sequence: u64,
) -> Result<u64, ReplicationError> {
    let mut acks = 1; // Count self

    for handle in handles {
        if let Ok(Some(peer_id)) = handle.await {
            acks += 1;
            debug!(peer = %peer_id, acks, quorum = quorum_size, "Replication ack received");

            if acks >= quorum_size {
                let mut cluster = state.cluster.write().await;
                cluster.last_applied_sequence = sequence;
                return Ok(sequence);
            }
        }
    }

    if acks >= quorum_size {
        Ok(sequence)
    } else {
        warn!(acks, quorum = quorum_size, "Failed to reach quorum");
        Err(ReplicationError::NoQuorum)
    }
}

/// Send a replication request to a peer over TCP
async fn send_replicate(
    transport: &super::ClusterTransport,
    peer_addr: &str,
    leader_id: &str,
    term: u64,
    sequence: u64,
    operation: WriteOp,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let request = ReplicateRequest {
        leader_id: leader_id.to_string(),
        term,
        sequence,
        operation,
    };

    let response = transport
        .send(
            peer_addr,
            ClusterMessage::ReplicateRequest(Box::new(request)),
        )
        .await?;

    match response {
        ClusterMessage::ReplicateResponse(resp) => Ok(resp.success),
        other => {
            warn!(peer = %peer_addr, "Unexpected replication response: {other:?}");
            Ok(false)
        }
    }
}
