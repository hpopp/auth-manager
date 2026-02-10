use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use thiserror::Error;

use crate::config::Config;
use crate::storage::models::NodeState;
use crate::storage::Database;

#[derive(Debug, Error)]
pub enum ClusterError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("No quorum available")]
    NoQuorum,
    #[error("Not the leader")]
    NotLeader,
}

/// Role of this node in the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Candidate,
    Follower,
    Leader,
}

/// Status of a peer node
#[derive(Debug, Clone)]
pub struct PeerState {
    pub address: String,
    pub last_seen: std::time::Instant,
    pub sequence: u64,
    pub status: String,
}

/// In-memory cluster state
#[derive(Debug)]
pub struct ClusterState {
    pub current_term: u64,
    pub last_applied_sequence: u64,
    pub last_heartbeat: std::time::Instant,
    /// Direct address of the leader (set from heartbeats)
    pub leader_address: Option<String>,
    pub leader_id: Option<String>,
    pub peer_states: HashMap<String, PeerState>,
    pub role: Role,
    pub voted_for: Option<String>,
    pub votes_received: usize,
}

impl ClusterState {
    /// Create a new cluster state from config and persisted state
    ///
    /// Peers start empty — they are populated by the discovery task
    /// (DNS or static) before cluster tasks begin.
    pub fn new(config: &Config, db: &Database) -> Result<Self, ClusterError> {
        // Load persisted state or create new
        let node_state = db.get_node_state()?.unwrap_or_else(|| {
            let state = NodeState::new(config.node.id.clone());
            if let Err(e) = db.put_node_state(&state) {
                tracing::error!(error = %e, "Failed to persist initial node state");
            }
            state
        });

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
            leader_address: None,
            peer_states: HashMap::new(),
            votes_received: 0,
            last_heartbeat: std::time::Instant::now(),
        })
    }

    /// Start a new election
    pub fn start_election(&mut self, node_id: &str) {
        self.current_term += 1;
        self.role = Role::Candidate;
        self.voted_for = Some(node_id.to_string());
        self.votes_received = 1; // Vote for self
        self.leader_id = None;
        tracing::info!(term = self.current_term, "Starting election");

        debug_assert_eq!(self.role, Role::Candidate);
        debug_assert_eq!(self.votes_received, 1);
        debug_assert!(self.leader_id.is_none());
    }

    /// Become leader
    pub fn become_leader(&mut self, node_id: &str) {
        self.role = Role::Leader;
        self.leader_id = Some(node_id.to_string());
        tracing::info!(term = self.current_term, "Became leader");

        debug_assert_eq!(self.role, Role::Leader);
        debug_assert!(self.leader_id.is_some());
    }

    /// Become follower
    pub fn become_follower(&mut self, term: u64, leader_id: Option<String>) {
        self.role = Role::Follower;
        self.current_term = term;
        self.leader_id = leader_id;
        self.leader_address = None; // Will be set by next heartbeat
        self.votes_received = 0;
        self.voted_for = None;

        debug_assert_eq!(self.role, Role::Follower);
        debug_assert_eq!(self.current_term, term);
        debug_assert_eq!(self.votes_received, 0);
        debug_assert!(self.voted_for.is_none());
    }

    /// Record a vote received
    pub fn record_vote(&mut self) {
        debug_assert_eq!(
            self.role,
            Role::Candidate,
            "only candidates should receive votes"
        );
        self.votes_received += 1;
        debug_assert!(
            self.votes_received <= self.cluster_size(),
            "votes cannot exceed cluster size"
        );
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
    ///
    /// Prefers the directly-advertised leader address (from heartbeats),
    /// falls back to looking up leader_id in peer_states.
    pub fn get_leader_address(&self) -> Option<String> {
        if let Some(ref addr) = self.leader_address {
            return Some(addr.clone());
        }
        let leader_id = self.leader_id.as_ref()?;
        self.peer_states.get(leader_id).map(|p| p.address.clone())
    }

    /// Current cluster size (discovered peers + self)
    pub fn cluster_size(&self) -> usize {
        self.peer_states.len() + 1
    }

    /// Required quorum size for writes (majority)
    pub fn quorum_size(&self) -> usize {
        self.cluster_size() / 2 + 1
    }

    /// Update the set of known peers from discovery results
    ///
    /// Adds newly discovered peers and removes peers that are no longer
    /// reported by discovery (e.g., removed from DNS).
    pub fn update_discovered_peers(&mut self, addrs: Vec<SocketAddr>) {
        let discovered: HashSet<String> = addrs.iter().map(|a| a.to_string()).collect();

        // Add newly discovered peers
        for addr_str in &discovered {
            if !self.peer_states.contains_key(addr_str) {
                self.peer_states.insert(
                    addr_str.clone(),
                    PeerState {
                        address: addr_str.clone(),
                        status: "discovered".to_string(),
                        sequence: 0,
                        last_seen: std::time::Instant::now(),
                    },
                );
                tracing::info!(peer = %addr_str, "Discovered new peer");
            }
        }

        // Remove peers that are no longer in discovery results
        let removed: Vec<String> = self
            .peer_states
            .keys()
            .filter(|k| !discovered.contains(*k))
            .cloned()
            .collect();

        for peer_id in &removed {
            self.peer_states.remove(peer_id);
            tracing::info!(peer = %peer_id, "Peer removed from discovery");
        }
    }
}
