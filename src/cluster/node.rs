use std::collections::HashMap;
use thiserror::Error;

use crate::config::Config;
use crate::storage::models::NodeState;
use crate::storage::Database;

#[derive(Debug, Error)]
pub enum ClusterError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("Not the leader")]
    NotLeader,
    #[error("No quorum available")]
    NoQuorum,
}

/// Role of this node in the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Leader,
    Follower,
    Candidate,
}

/// Status of a peer node
#[derive(Debug, Clone)]
pub struct PeerState {
    pub address: String,
    pub status: String,
    pub sequence: u64,
    pub last_seen: std::time::Instant,
}

/// In-memory cluster state
#[derive(Debug)]
pub struct ClusterState {
    pub role: Role,
    pub current_term: u64,
    pub voted_for: Option<String>,
    pub last_applied_sequence: u64,
    pub leader_id: Option<String>,
    pub peer_states: HashMap<String, PeerState>,
    pub votes_received: usize,
    pub last_heartbeat: std::time::Instant,
}

impl ClusterState {
    /// Create a new cluster state from config and persisted state
    pub fn new(config: &Config, db: &Database) -> Result<Self, ClusterError> {
        // Load persisted state or create new
        let node_state = db.get_node_state()?.unwrap_or_else(|| {
            let state = NodeState::new(config.node.id.clone());
            let _ = db.put_node_state(&state);
            state
        });

        // Initialize peer states
        let mut peer_states = HashMap::new();
        for peer_addr in &config.cluster.peers {
            // Extract peer ID from address (or use address as ID)
            let peer_id = peer_addr.split(':').next().unwrap_or(peer_addr).to_string();
            peer_states.insert(
                peer_id,
                PeerState {
                    address: peer_addr.clone(),
                    status: "unknown".to_string(),
                    sequence: 0,
                    last_seen: std::time::Instant::now(),
                },
            );
        }

        // Start as follower if in cluster mode, leader if single-node
        let role = if config.is_single_node() {
            Role::Leader
        } else {
            Role::Follower
        };

        Ok(Self {
            role,
            current_term: node_state.current_term,
            voted_for: node_state.voted_for,
            last_applied_sequence: node_state.last_applied_sequence,
            leader_id: None,
            peer_states,
            votes_received: 0,
            last_heartbeat: std::time::Instant::now(),
        })
    }

    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        self.role == Role::Leader
    }

    /// Start a new election
    pub fn start_election(&mut self, node_id: &str) {
        self.current_term += 1;
        self.role = Role::Candidate;
        self.voted_for = Some(node_id.to_string());
        self.votes_received = 1; // Vote for self
        self.leader_id = None;
        tracing::info!(term = self.current_term, "Starting election");
    }

    /// Become leader
    pub fn become_leader(&mut self, node_id: &str) {
        self.role = Role::Leader;
        self.leader_id = Some(node_id.to_string());
        tracing::info!(term = self.current_term, "Became leader");
    }

    /// Become follower
    pub fn become_follower(&mut self, term: u64, leader_id: Option<String>) {
        self.role = Role::Follower;
        self.current_term = term;
        self.leader_id = leader_id;
        self.votes_received = 0;
        self.voted_for = None;
    }

    /// Record a vote received
    pub fn record_vote(&mut self) {
        self.votes_received += 1;
    }

    /// Check if we have enough votes to become leader
    pub fn has_majority(&self, cluster_size: usize) -> bool {
        self.votes_received > cluster_size / 2
    }

    /// Update heartbeat timestamp
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = std::time::Instant::now();
    }

    /// Check if heartbeat has timed out
    pub fn heartbeat_timeout(&self, timeout_ms: u64) -> bool {
        self.last_heartbeat.elapsed().as_millis() > timeout_ms as u128
    }

    /// Update a peer's state
    pub fn update_peer(&mut self, peer_id: &str, status: &str, sequence: u64) {
        if let Some(peer) = self.peer_states.get_mut(peer_id) {
            peer.status = status.to_string();
            peer.sequence = sequence;
            peer.last_seen = std::time::Instant::now();
        }
    }

    /// Persist the current state to the database
    pub fn persist(&self, db: &Database, node_id: &str) -> Result<(), ClusterError> {
        let state = NodeState {
            node_id: node_id.to_string(),
            current_term: self.current_term,
            voted_for: self.voted_for.clone(),
            last_applied_sequence: self.last_applied_sequence,
        };
        db.put_node_state(&state)?;
        Ok(())
    }

    /// Get the leader's address if known
    pub fn get_leader_address(&self) -> Option<String> {
        let leader_id = self.leader_id.as_ref()?;
        self.peer_states.get(leader_id).map(|p| p.address.clone())
    }
}
