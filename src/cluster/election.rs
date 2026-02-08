//! Leader election with term-based voting

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use super::discovery;
use super::node::Role;
use crate::AppState;

/// Start the election monitor task
/// This checks for heartbeat timeouts and triggers elections
pub fn start_election_monitor(state: Arc<AppState>) -> JoinHandle<()> {
    let election_timeout = state.config.cluster.election_timeout_ms;
    let check_interval = Duration::from_millis(election_timeout / 4);

    tokio::spawn(async move {
        // Random startup delay to stagger initial elections across nodes
        let startup_delay = rand::random::<u64>() % 2000 + 1000; // 1-3 seconds
        debug!("Election monitor starting in {}ms", startup_delay);
        tokio::time::sleep(Duration::from_millis(startup_delay)).await;

        let mut interval = tokio::time::interval(check_interval);

        // Add some randomization to prevent split votes
        let jitter = rand::random::<u64>() % (election_timeout / 2);
        let effective_timeout = election_timeout + jitter;

        // Reset heartbeat timer after startup delay
        {
            let mut cluster = state.cluster.write().await;
            cluster.update_heartbeat();
        }

        loop {
            interval.tick().await;

            let mut cluster = state.cluster.write().await;

            match cluster.role {
                Role::Leader => {
                    // Leaders don't need to monitor for elections
                    continue;
                }
                Role::Follower => {
                    // Check if heartbeat has timed out
                    if cluster.heartbeat_timeout(effective_timeout) {
                        info!("Heartbeat timeout, starting election");
                        cluster.start_election(&state.config.node.id);

                        // Request votes from peers
                        drop(cluster); // Release lock before making network calls
                        request_votes(Arc::clone(&state)).await;
                    }
                }
                Role::Candidate => {
                    // Check if we've won the election
                    let cluster_size = cluster.cluster_size();
                    if cluster.has_majority(cluster_size) {
                        cluster.become_leader(&state.config.node.id);
                        cluster.leader_address = Some(discovery::compute_advertise_address(
                            &state.config.node.bind_address,
                        ));

                        // Persist state
                        if let Err(e) = cluster.persist(&state.db, &state.config.node.id) {
                            warn!(error = %e, "Failed to persist cluster state");
                        }
                    } else if cluster.heartbeat_timeout(effective_timeout * 2) {
                        // Election timed out, start a new one
                        debug!("Election timed out, restarting");
                        cluster.start_election(&state.config.node.id);

                        drop(cluster);
                        request_votes(Arc::clone(&state)).await;
                    }
                }
            }
        }
    })
}

/// Request votes from all peers
async fn request_votes(state: Arc<AppState>) {
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
        let state = Arc::clone(&state);
        let node_id = state.config.node.id.clone();
        let client = state.http_client.clone();

        tokio::spawn(async move {
            match send_vote_request(&client, &peer_addr, &node_id, term, sequence).await {
                Ok((peer_term, granted)) => {
                    let mut cluster = state.cluster.write().await;

                    // If peer has higher term, step down
                    if peer_term > cluster.current_term {
                        cluster.become_follower(peer_term, None);
                        return;
                    }

                    if granted && cluster.role == Role::Candidate && cluster.current_term == term {
                        cluster.record_vote();
                        debug!(peer = %peer_id, "Vote granted");
                    }
                }
                Err(e) => {
                    debug!(peer = %peer_id, error = %e, "Failed to request vote");
                }
            }
        });
    }
}

/// Request body for vote RPC
#[derive(Debug, Serialize)]
struct VoteRequest {
    candidate_id: String,
    last_sequence: u64,
    term: u64,
}

/// Response from vote RPC
#[derive(Debug, Deserialize)]
struct VoteResponse {
    term: u64,
    vote_granted: bool,
}

/// Send a vote request to a peer
async fn send_vote_request(
    client: &reqwest::Client,
    peer_addr: &str,
    node_id: &str,
    term: u64,
    sequence: u64,
) -> Result<(u64, bool), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("http://{}/_internal/vote", peer_addr);

    let request = VoteRequest {
        candidate_id: node_id.to_string(),
        term,
        last_sequence: sequence,
    };

    let response = client.post(&url).json(&request).send().await?;

    if response.status().is_success() {
        let resp: VoteResponse = response.json().await?;
        Ok((resp.term, resp.vote_granted))
    } else {
        Err(format!("Vote request failed with status: {}", response.status()).into())
    }
}
