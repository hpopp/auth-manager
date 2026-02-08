//! Follower catch-up mechanism for syncing new/lagging nodes
#![allow(dead_code)]

use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info};

use crate::storage::models::{ApiKey, ReplicatedWrite, SessionToken, WriteOp};
use crate::storage::Database;
use crate::AppState;

#[derive(Debug, Error)]
pub enum CatchupError {
    #[error("Database error: {0}")]
    Database(#[from] crate::storage::DatabaseError),
    #[error("Network error: {0}")]
    Network(String),
}

/// Sync response from leader
#[derive(Debug, Clone)]
pub struct SyncResponse {
    pub leader_sequence: u64,
    pub log_entries: Vec<ReplicatedWrite>,
    pub snapshot: Option<Snapshot>,
}

/// Full state snapshot
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub api_keys: Vec<ApiKey>,
    pub sequence: u64,
    pub sessions: Vec<SessionToken>,
}

/// Request sync from the leader
pub async fn request_sync(state: Arc<AppState>, leader_addr: &str) -> Result<(), CatchupError> {
    let my_sequence = {
        let cluster = state.cluster.read().await;
        cluster.last_applied_sequence
    };

    info!(my_sequence, leader = %leader_addr, "Requesting sync from leader");

    // Request sync from leader
    let response = send_sync_request(leader_addr, my_sequence).await?;

    // Apply snapshot if provided
    if let Some(snapshot) = response.snapshot {
        apply_snapshot(&state.db, &snapshot)?;
        info!(sequence = snapshot.sequence, "Applied snapshot");
    }

    // Apply log entries
    for entry in response.log_entries {
        apply_log_entry(&state.db, &entry)?;
    }

    // Update our sequence
    {
        let mut cluster = state.cluster.write().await;
        cluster.last_applied_sequence = response.leader_sequence;
    }

    info!(sequence = response.leader_sequence, "Sync complete");
    Ok(())
}

/// Send a sync request to the leader
async fn send_sync_request(
    _leader_addr: &str,
    _from_sequence: u64,
) -> Result<SyncResponse, CatchupError> {
    // TODO: Implement actual RPC call to leader
    // For now, return empty response

    // In a real implementation, this would:
    // 1. Connect to leader_addr
    // 2. Send SyncRequest with from_sequence
    // 3. Receive snapshot (if far behind) + log entries

    Ok(SyncResponse {
        snapshot: None,
        log_entries: Vec::new(),
        leader_sequence: 0,
    })
}

/// Apply a snapshot to the local database
fn apply_snapshot(db: &Database, snapshot: &Snapshot) -> Result<(), CatchupError> {
    // Apply sessions
    for session in &snapshot.sessions {
        db.put_session(session)?;
    }

    // Apply API keys
    for api_key in &snapshot.api_keys {
        db.put_api_key(api_key)?;
    }

    debug!(
        sessions = snapshot.sessions.len(),
        api_keys = snapshot.api_keys.len(),
        "Snapshot applied"
    );

    Ok(())
}

/// Apply a single log entry to the local database
fn apply_log_entry(db: &Database, entry: &ReplicatedWrite) -> Result<(), CatchupError> {
    match &entry.operation {
        WriteOp::CreateSession(session) => {
            db.put_session(session)?;
        }
        WriteOp::RevokeSession { token_id } => {
            db.delete_session(token_id)?;
        }
        WriteOp::CreateApiKey(api_key) => {
            db.put_api_key(api_key)?;
        }
        WriteOp::RevokeApiKey { key_id } => {
            db.delete_api_key(key_id)?;
        }
        WriteOp::UpdateApiKey {
            key_hash,
            description,
            name,
            scopes,
        } => {
            db.update_api_key(
                key_hash,
                name.as_deref(),
                description.as_ref().map(|d| d.as_deref()),
                scopes.as_deref(),
            )?;
        }
    }

    // Record in replication log
    db.append_replication_log(entry)?;

    Ok(())
}

/// Handle a sync request from a follower (leader only)
pub async fn handle_sync_request(
    state: Arc<AppState>,
    from_sequence: u64,
) -> Result<SyncResponse, CatchupError> {
    let leader_sequence = state.db.get_latest_sequence()?;

    // If follower is far behind, send a snapshot
    let snapshot = if from_sequence == 0 || (leader_sequence - from_sequence) > 10000 {
        Some(create_snapshot(&state.db)?)
    } else {
        None
    };

    // Get log entries from follower's sequence
    let start_sequence = snapshot
        .as_ref()
        .map(|s| s.sequence + 1)
        .unwrap_or(from_sequence + 1);

    let log_entries = state.db.get_replication_log_from(start_sequence)?;

    Ok(SyncResponse {
        snapshot,
        log_entries,
        leader_sequence,
    })
}

/// Create a snapshot of the current state
fn create_snapshot(db: &Database) -> Result<Snapshot, CatchupError> {
    let sequence = db.get_latest_sequence()?;
    let sessions = db.get_all_sessions()?;
    let api_keys = db.get_all_api_keys()?;

    Ok(Snapshot {
        sequence,
        sessions,
        api_keys,
    })
}
