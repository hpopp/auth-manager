//! Leader election with term-based voting

use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use super::discovery;
use super::node::Role;
use super::rpc::{ClusterMessage, VoteRequest};
use crate::AppState;

/// Start the election monitor task.
/// This checks for heartbeat timeouts and triggers elections.
pub fn start_election_monitor(state: Arc<AppState>) -> JoinHandle<()> {
    let election_timeout = state.config.cluster.election_timeout_ms;
    let check_interval = Duration::from_millis(election_timeout / 3);

    tokio::spawn(async move {
        // Random startup delay to stagger initial elections across nodes
        let startup_delay = rand::random::<u64>() % 350 + 150; // 150-500ms
        debug!("Election monitor starting in {}ms", startup_delay);
        tokio::time::sleep(Duration::from_millis(startup_delay)).await;

        // Add some randomization to prevent split votes
        let jitter = rand::random::<u64>() % (election_timeout / 2);
        let effective_timeout = election_timeout + jitter;

        // Reset heartbeat timer after startup delay
        {
            let mut cluster = state.cluster.write().await;
            cluster.update_heartbeat();
        }

        let mut interval = tokio::time::interval(check_interval);
        loop {
            interval.tick().await;
            check_election(&state, effective_timeout).await;
        }
    })
}

/// Run one election check: inspect the current role and act accordingly.
async fn check_election(state: &Arc<AppState>, timeout: u64) {
    let mut cluster = state.cluster.write().await;

    match cluster.role {
        Role::Leader => {
            // Nothing to do.
        }
        Role::Follower => {
            if cluster.heartbeat_timeout(timeout) {
                info!("Heartbeat timeout, starting election");
                cluster.start_election(&state.config.node.id);
                drop(cluster);
                request_votes(state).await;
            }
        }
        Role::Candidate => {
            let cluster_size = cluster.cluster_size();
            if cluster.has_majority(cluster_size) {
                cluster.become_leader(&state.config.node.id);
                cluster.leader_address = Some(discovery::compute_advertise_address(
                    &state.config.node.bind_address,
                ));

                if let Err(e) = cluster.persist(&state.db, &state.config.node.id) {
                    warn!(error = %e, "Failed to persist cluster state");
                }
            } else if cluster.heartbeat_timeout(timeout * 2) {
                debug!("Election timed out, restarting");
                cluster.start_election(&state.config.node.id);
                drop(cluster);
                request_votes(state).await;
            }
        }
    }
}

/// Request votes from all peers in parallel.
async fn request_votes(state: &Arc<AppState>) {
    let cluster = state.cluster.read().await;
    let term = cluster.current_term;
    let sequence = cluster.last_applied_sequence;
    let peers: Vec<_> = cluster
        .peer_states
        .iter()
        .map(|(id, p)| (id.clone(), p.address.clone()))
        .collect();
    drop(cluster);

    for (peer_id, peer_addr) in peers {
        let state = Arc::clone(state);
        let node_id = state.config.node.id.clone();

        tokio::spawn(async move {
            request_vote(&state, &peer_id, &peer_addr, &node_id, term, sequence).await;
        });
    }
}

/// Send a vote request to a single peer and process the response.
/// Steps down if the peer has a higher term, otherwise records
/// the vote if still a candidate.
async fn request_vote(
    state: &AppState,
    peer_id: &str,
    peer_addr: &str,
    node_id: &str,
    term: u64,
    sequence: u64,
) {
    let request = VoteRequest {
        candidate_id: node_id.to_string(),
        term,
        last_sequence: sequence,
    };

    let response = state
        .transport
        .send(peer_addr, ClusterMessage::VoteRequest(request))
        .await;

    match response {
        Ok(ClusterMessage::VoteResponse(resp)) => {
            let mut cluster = state.cluster.write().await;

            if resp.term > cluster.current_term {
                cluster.become_follower(resp.term, None);
                return;
            }

            if resp.vote_granted && cluster.role == Role::Candidate && cluster.current_term == term
            {
                cluster.record_vote();
                debug!(peer = %peer_id, "Vote granted");
            }
        }
        Ok(other) => {
            warn!(peer = %peer_id, "Unexpected vote response: {other:?}");
        }
        Err(e) => {
            debug!(peer = %peer_id, error = %e, "Failed to request vote");
        }
    }
}
