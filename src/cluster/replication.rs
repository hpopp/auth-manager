//! Quorum-based replication for write operations

use std::sync::Arc;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

use crate::storage::models::{ReplicatedWrite, WriteOp};
use crate::storage::Database;
use crate::AppState;
use super::node::Role;

#[derive(Debug, Error)]
pub enum ReplicationError {
    #[error("Not the leader")]
    NotLeader,
    #[error("Failed to reach quorum")]
    NoQuorum,
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
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
    
    // Replicate to peers
    let quorum_size = state.config.quorum_size();
    let mut acks = 1; // Count self
    
    let cluster = state.cluster.read().await;
    let peers: Vec<_> = cluster.peer_states.iter()
        .map(|(id, p)| (id.clone(), p.address.clone()))
        .collect();
    drop(cluster);
    
    // Get leader info for replication requests
    let cluster = state.cluster.read().await;
    let leader_id = state.config.node.id.clone();
    let term = cluster.current_term;
    drop(cluster);
    
    // Send to all peers in parallel
    let mut handles = Vec::new();
    for (peer_id, peer_addr) in peers {
        let op = operation.clone();
        let seq = sequence;
        let lid = leader_id.clone();
        
        handles.push(tokio::spawn(async move {
            match send_replicate(&peer_addr, &lid, term, seq, op).await {
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
    term: u64,
    sequence: u64,
    operation: WriteOp,
}

/// Response from replication RPC
#[derive(Debug, Deserialize)]
struct ReplicateResponse {
    success: bool,
    sequence: u64,
}

/// Send a replication request to a peer
async fn send_replicate(
    peer_addr: &str,
    leader_id: &str,
    term: u64,
    sequence: u64,
    operation: WriteOp,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    
    let url = format!("http://{}/internal/replicate", peer_addr);
    
    let request = ReplicateRequest {
        leader_id: leader_id.to_string(),
        term,
        sequence,
        operation,
    };
    
    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await?;
    
    if response.status().is_success() {
        let resp: ReplicateResponse = response.json().await?;
        Ok(resp.success)
    } else {
        warn!(peer = %peer_addr, status = %response.status(), "Replication request failed");
        Ok(false)
    }
}
