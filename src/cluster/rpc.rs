//! Shared request/response types for internal cluster communication

use serde::{Deserialize, Serialize};

use crate::storage::models::WriteOp;

// ============================================================================
// Heartbeat
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct HeartbeatRequest {
    /// Leader's advertise address (for follower forwarding)
    #[serde(default)]
    pub leader_address: Option<String>,
    pub leader_id: String,
    pub sequence: u64,
    pub term: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HeartbeatResponse {
    pub sequence: u64,
    pub success: bool,
    pub term: u64,
}

// ============================================================================
// Replication
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct ReplicateRequest {
    pub leader_id: String,
    pub operation: WriteOp,
    pub sequence: u64,
    pub term: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReplicateResponse {
    pub sequence: u64,
    pub success: bool,
}

// ============================================================================
// Voting
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct VoteRequest {
    pub candidate_id: String,
    pub last_sequence: u64,
    pub term: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}
