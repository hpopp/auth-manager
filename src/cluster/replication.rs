//! Quorum-based replication for write operations

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, warn};

use super::node::Role;
use crate::storage::models::{ReplicatedWrite, WriteOp};
use crate::storage::Database;
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
    // Single-node mode: no replication needed
    if state.config.is_single_node() {
        let sequence = append_to_log(&state.db, operation)?;
        return Ok(sequence);
    }

    // Check if we're the leader
    {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            return Err(ReplicationError::NotLeader);
        }
    }

    // Append to local log
    let sequence = append_to_log(&state.db, operation.clone())?;

    // Gather cluster info in a single lock acquisition
    let (peers, leader_id, term, quorum_size) = {
        let cluster = state.cluster.read().await;
        let peers: Vec<_> = cluster
            .peer_states
            .iter()
            .map(|(id, p)| (id.clone(), p.address.clone()))
            .collect();
        let leader_id = state.config.node.id.clone();
        let term = cluster.current_term;
        let quorum_size = cluster.quorum_size();
        (peers, leader_id, term, quorum_size)
    };

    let mut acks = 1; // Count self

    // Send to all peers in parallel
    let mut handles = Vec::new();
    for (peer_id, peer_addr) in peers {
        let op = operation.clone();
        let seq = sequence;
        let lid = leader_id.clone();
        let client = state.http_client.clone();

        handles.push(tokio::spawn(async move {
            match send_replicate(&client, &peer_addr, &lid, term, seq, op).await {
                Ok(true) => Some(peer_id),
                Ok(false) => None,
                Err(e) => {
                    debug!(peer = %peer_id, error = %e, "Replication failed");
                    None
                }
            }
        }));
    }

    // Wait for quorum
    for handle in handles {
        if let Ok(Some(peer_id)) = handle.await {
            acks += 1;
            debug!(peer = %peer_id, acks, quorum = quorum_size, "Replication ack received");

            if acks >= quorum_size {
                // Update sequence in cluster state
                let mut cluster = state.cluster.write().await;
                cluster.last_applied_sequence = sequence;
                return Ok(sequence);
            }
        }
    }

    // Check if we reached quorum
    if acks >= quorum_size {
        Ok(sequence)
    } else {
        warn!(acks, quorum = quorum_size, "Failed to reach quorum");
        Err(ReplicationError::NoQuorum)
    }
}

/// Append an operation to the local replication log
fn append_to_log(db: &Database, operation: WriteOp) -> Result<u64, ReplicationError> {
    let sequence = db.get_latest_sequence()? + 1;

    let write = ReplicatedWrite {
        sequence,
        operation,
        timestamp: Utc::now(),
    };

    db.append_replication_log(&write)?;
    Ok(sequence)
}

/// Request body for replication RPC
#[derive(Debug, Serialize)]
struct ReplicateRequest {
    leader_id: String,
    operation: WriteOp,
    sequence: u64,
    term: u64,
}

/// Response from replication RPC
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReplicateResponse {
    sequence: u64,
    success: bool,
}

/// Send a replication request to a peer
async fn send_replicate(
    client: &reqwest::Client,
    peer_addr: &str,
    leader_id: &str,
    term: u64,
    sequence: u64,
    operation: WriteOp,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("http://{}/_internal/replicate", peer_addr);

    let request = ReplicateRequest {
        leader_id: leader_id.to_string(),
        term,
        sequence,
        operation,
    };

    let response = client.post(&url).json(&request).send().await?;

    if response.status().is_success() {
        let resp: ReplicateResponse = response.json().await?;
        Ok(resp.success)
    } else {
        warn!(peer = %peer_addr, status = %response.status(), "Replication request failed");
        Ok(false)
    }
}
