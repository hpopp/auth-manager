//! Shared request/response types for internal cluster communication

use serde::{Deserialize, Serialize};

use crate::storage::models::{ApiKey, ReplicatedWrite, SessionToken, WriteOp};

// ============================================================================
// Envelope — tagged enum for the TCP wire protocol (MessagePack encoded)
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub enum ClusterMessage {
    HeartbeatRequest(HeartbeatRequest),
    HeartbeatResponse(HeartbeatResponse),
    ReplicateRequest(Box<ReplicateRequest>),
    ReplicateResponse(ReplicateResponse),
    VoteRequest(VoteRequest),
    VoteResponse(VoteResponse),
    SyncRequest(SyncRequest),
    SyncResponse(Box<SyncResponse>),
}

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

// ============================================================================
// Sync / Catchup
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct SyncRequest {
    pub from_sequence: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyncResponse {
    pub leader_sequence: u64,
    pub log_entries: Vec<ReplicatedWrite>,
    pub snapshot: Option<Snapshot>,
}

/// Full state snapshot sent to lagging followers
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Snapshot {
    pub api_keys: Vec<ApiKey>,
    pub sequence: u64,
    pub sessions: Vec<SessionToken>,
}
