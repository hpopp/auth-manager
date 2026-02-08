//! Heartbeat mechanism for leader-follower communication

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::discovery;
use super::node::Role;
use crate::AppState;

/// Start the heartbeat task
/// Leaders send heartbeats, followers receive them
pub fn start_heartbeat_task(state: Arc<AppState>) -> JoinHandle<()> {
    let heartbeat_interval = Duration::from_millis(state.config.cluster.heartbeat_interval_ms);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(heartbeat_interval);

        loop {
            interval.tick().await;

            let cluster = state.cluster.read().await;

            if cluster.role == Role::Leader {
                let peers: Vec<_> = cluster
                    .peer_states
                    .iter()
                    .map(|(id, p)| (id.clone(), p.address.clone()))
                    .collect();
                let term = cluster.current_term;
                let sequence = cluster.last_applied_sequence;
                let leader_id = state.config.node.id.clone();
                let leader_address =
                    discovery::compute_advertise_address(&state.config.node.bind_address);
                drop(cluster);

                // Send heartbeats to all peers
                for (peer_id, peer_addr) in peers {
                    let state = Arc::clone(&state);
                    let lid = leader_id.clone();
                    let la = leader_address.clone();

                    tokio::spawn(async move {
                        match send_heartbeat(&peer_addr, &lid, &la, term, sequence).await {
                            Ok((peer_term, peer_sequence)) => {
                                let mut cluster = state.cluster.write().await;

                                // If peer has higher term, step down
                                if peer_term > cluster.current_term {
                                    cluster.become_follower(peer_term, None);
                                    warn!(peer_term, "Peer has higher term, stepping down");
                                } else {
                                    cluster.update_peer(&peer_id, "synced", peer_sequence);
                                }
                            }
                            Err(e) => {
                                debug!(peer = %peer_id, error = %e, "Failed to send heartbeat");
                                let mut cluster = state.cluster.write().await;
                                cluster.update_peer(&peer_id, "unreachable", 0);
                            }
                        }
                    });
                }
            }
        }
    })
}

/// Request body for heartbeat RPC
#[derive(Debug, Serialize)]
struct HeartbeatRequest {
    leader_address: String,
    leader_id: String,
    sequence: u64,
    term: u64,
}

/// Response from heartbeat RPC
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HeartbeatResponse {
    sequence: u64,
    success: bool,
    term: u64,
}

/// Send a heartbeat to a peer
async fn send_heartbeat(
    peer_addr: &str,
    leader_id: &str,
    leader_address: &str,
    term: u64,
    sequence: u64,
) -> Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    let url = format!("http://{}/_internal/heartbeat", peer_addr);

    let request = HeartbeatRequest {
        leader_id: leader_id.to_string(),
        leader_address: leader_address.to_string(),
        term,
        sequence,
    };

    let response = client.post(&url).json(&request).send().await?;

    if response.status().is_success() {
        let resp: HeartbeatResponse = response.json().await?;
        Ok((resp.term, resp.sequence))
    } else {
        Err(format!("Heartbeat failed with status: {}", response.status()).into())
    }
}
