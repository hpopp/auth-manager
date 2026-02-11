//! Heartbeat mechanism for leader-follower communication

use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::discovery;
use super::node::Role;
use super::rpc::{ClusterMessage, HeartbeatRequest};
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
                        match send_heartbeat(
                            &state.transport,
                            &peer_addr,
                            &lid,
                            &la,
                            term,
                            sequence,
                        )
                        .await
                        {
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

/// Send a heartbeat to a peer over TCP
async fn send_heartbeat(
    transport: &super::ClusterTransport,
    peer_addr: &str,
    leader_id: &str,
    leader_address: &str,
    term: u64,
    sequence: u64,
) -> Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>> {
    let request = HeartbeatRequest {
        leader_address: Some(leader_address.to_string()),
        leader_id: leader_id.to_string(),
        sequence,
        term,
    };

    let response = transport
        .send(peer_addr, ClusterMessage::HeartbeatRequest(request))
        .await?;

    match response {
        ClusterMessage::HeartbeatResponse(resp) => Ok((resp.term, resp.sequence)),
        other => Err(format!("Unexpected response: {other:?}").into()),
    }
}
