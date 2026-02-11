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
            send_heartbeats(&state).await;
        }
    })
}

/// Run one heartbeat round: if we're the leader, send a heartbeat
/// to every peer in parallel.
async fn send_heartbeats(state: &Arc<AppState>) {
    let cluster = state.cluster.read().await;
    if cluster.role != Role::Leader {
        return;
    }

    let info = gather_heartbeat_info(state, &cluster);
    drop(cluster);

    for (peer_id, peer_addr) in &info.peers {
        let state = Arc::clone(state);
        let peer_id = peer_id.clone();
        let peer_addr = peer_addr.clone();
        let info = info.clone();

        tokio::spawn(async move {
            let result = send_heartbeat(
                &state.transport,
                &peer_addr,
                &info.leader_id,
                &info.leader_address,
                info.term,
                info.sequence,
            )
            .await;

            handle_heartbeat_response(&state, &peer_id, result).await;
        });
    }
}

/// Snapshot of leader state needed to send heartbeats.
#[derive(Clone)]
struct HeartbeatInfo {
    peers: Vec<(String, String)>,
    leader_id: String,
    leader_address: String,
    term: u64,
    sequence: u64,
}

/// Snapshot the cluster info needed for a heartbeat round.
fn gather_heartbeat_info(state: &AppState, cluster: &super::ClusterState) -> HeartbeatInfo {
    HeartbeatInfo {
        peers: cluster
            .peer_states
            .iter()
            .map(|(id, p)| (id.clone(), p.address.clone()))
            .collect(),
        leader_id: state.config.node.id.clone(),
        leader_address: discovery::compute_advertise_address(&state.config.node.bind_address),
        term: cluster.current_term,
        sequence: cluster.last_applied_sequence,
    }
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

/// Process the result of a heartbeat send. Steps down if the peer
/// has a higher term, otherwise updates the peer's status.
async fn handle_heartbeat_response(
    state: &AppState,
    peer_id: &str,
    result: Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>>,
) {
    match result {
        Ok((peer_term, peer_sequence)) => {
            let mut cluster = state.cluster.write().await;

            if peer_term > cluster.current_term {
                cluster.become_follower(peer_term, None);
                warn!(peer_term, "Peer has higher term, stepping down");
            } else {
                cluster.update_peer(peer_id, "synced", peer_sequence);
            }
        }
        Err(e) => {
            debug!(peer = %peer_id, error = %e, "Failed to send heartbeat");
            let mut cluster = state.cluster.write().await;
            cluster.update_peer(peer_id, "unreachable", 0);
        }
    }
}
