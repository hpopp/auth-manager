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
            heartbeat_peer(&state, &peer_id, &peer_addr, &info).await;
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

/// Send a heartbeat to a single peer and process the response.
/// Steps down if the peer has a higher term, otherwise updates
/// the peer's status.
async fn heartbeat_peer(state: &AppState, peer_id: &str, peer_addr: &str, info: &HeartbeatInfo) {
    let request = HeartbeatRequest {
        leader_address: Some(info.leader_address.clone()),
        leader_id: info.leader_id.clone(),
        sequence: info.sequence,
        term: info.term,
    };

    let response = state
        .transport
        .send(peer_addr, ClusterMessage::HeartbeatRequest(request))
        .await;

    match response {
        Ok(ClusterMessage::HeartbeatResponse(resp)) => {
            let mut cluster = state.cluster.write().await;

            if resp.term > cluster.current_term {
                cluster.become_follower(resp.term, None);
                warn!(peer_term = resp.term, "Peer has higher term, stepping down");
            } else {
                cluster.update_peer(peer_id, "synced", resp.sequence);
            }
        }
        Ok(other) => {
            warn!(peer = %peer_id, "Unexpected heartbeat response: {other:?}");
        }
        Err(e) => {
            debug!(peer = %peer_id, error = %e, "Failed to send heartbeat");
            let mut cluster = state.cluster.write().await;
            cluster.update_peer(peer_id, "unreachable", 0);
        }
    }
}
