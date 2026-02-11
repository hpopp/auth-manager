//! Pure cluster message handlers — no Axum dependency.
//!
//! Each function takes `Arc<AppState>` + a request struct and returns
//! a response struct (or error string). The TCP server dispatches
//! incoming `ClusterMessage` variants to these handlers.

use chrono::Utc;
use std::sync::Arc;
use tracing::debug;

use super::catchup;
use super::rpc::{
    HeartbeatRequest, HeartbeatResponse, ReplicateRequest, ReplicateResponse, SyncRequest,
    SyncResponse, VoteRequest, VoteResponse,
};
use super::Role;
use crate::storage::models::{ReplicatedWrite, WriteOp};
use crate::AppState;

// ============================================================================
// Heartbeat
// ============================================================================

pub async fn handle_heartbeat(state: Arc<AppState>, req: HeartbeatRequest) -> HeartbeatResponse {
    let mut cluster = state.cluster.write().await;

    if req.term < cluster.current_term {
        return HeartbeatResponse {
            term: cluster.current_term,
            success: false,
            sequence: cluster.last_applied_sequence,
        };
    }

    if req.term > cluster.current_term || cluster.role != Role::Follower {
        cluster.become_follower(req.term, Some(req.leader_id.clone()));
    }

    cluster.leader_id = Some(req.leader_id);
    if let Some(addr) = req.leader_address {
        cluster.leader_address = Some(addr);
    }
    cluster.update_heartbeat();

    let my_sequence = cluster.last_applied_sequence;
    let leader_sequence = req.sequence;
    let term = cluster.current_term;
    let leader_addr = cluster.leader_address.clone();
    drop(cluster);

    // If we're behind the leader, trigger a background sync
    if leader_sequence > my_sequence {
        if let Some(addr) = leader_addr {
            let sync_state = Arc::clone(&state);
            tokio::spawn(async move {
                if let Err(e) = catchup::request_sync(sync_state, &addr).await {
                    match e {
                        catchup::CatchupError::AlreadySyncing => {}
                        _ => tracing::warn!(error = %e, "Background sync failed."),
                    }
                }
            });
        }
    }

    HeartbeatResponse {
        term,
        success: true,
        sequence: my_sequence,
    }
}

// ============================================================================
// Replication
// ============================================================================

pub async fn handle_replicate(
    state: Arc<AppState>,
    req: ReplicateRequest,
) -> Result<ReplicateResponse, String> {
    let mut cluster = state.cluster.write().await;

    if req.term < cluster.current_term {
        return Ok(ReplicateResponse {
            success: false,
            sequence: cluster.last_applied_sequence,
        });
    }

    if req.term > cluster.current_term {
        cluster.become_follower(req.term, Some(req.leader_id.clone()));
    }

    cluster.update_heartbeat();
    drop(cluster);

    match apply_write_op(&state.db, &req.operation) {
        Ok(()) => {
            let write = ReplicatedWrite {
                sequence: req.sequence,
                operation: req.operation,
                timestamp: Utc::now(),
            };

            if let Err(e) = state.db.append_replication_log(&write) {
                tracing::error!(error = %e, "Failed to append to replication log");
                return Err(format!("Failed to append to replication log: {e}"));
            }

            let mut cluster = state.cluster.write().await;
            cluster.last_applied_sequence = req.sequence;

            debug!(sequence = req.sequence, "Applied replicated write");

            Ok(ReplicateResponse {
                success: true,
                sequence: req.sequence,
            })
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to apply replicated write");
            Err(format!("Failed to apply write: {e}"))
        }
    }
}

// ============================================================================
// Vote
// ============================================================================

pub async fn handle_vote(state: Arc<AppState>, req: VoteRequest) -> VoteResponse {
    let mut cluster = state.cluster.write().await;

    if req.term < cluster.current_term {
        return VoteResponse {
            term: cluster.current_term,
            vote_granted: false,
        };
    }

    if req.term > cluster.current_term {
        cluster.become_follower(req.term, None);
    }

    let dominated_log = req.last_sequence >= cluster.last_applied_sequence;
    let grant = dominated_log
        && (cluster.voted_for.is_none() || cluster.voted_for.as_deref() == Some(&req.candidate_id));

    if grant {
        cluster.voted_for = Some(req.candidate_id.clone());
        cluster.update_heartbeat();

        if let Err(e) = cluster.persist(&state.db, &state.config.node.id) {
            tracing::warn!(error = %e, "Failed to persist vote");
        }

        debug!(candidate = %req.candidate_id, term = req.term, "Granted vote");
    }

    VoteResponse {
        term: cluster.current_term,
        vote_granted: grant,
    }
}

// ============================================================================
// Sync
// ============================================================================

pub async fn handle_sync(state: Arc<AppState>, req: SyncRequest) -> Result<SyncResponse, String> {
    // Only leader should serve sync requests
    {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            return Err("Not the leader.".to_string());
        }
    }

    catchup::handle_sync_request(state, req.from_sequence)
        .await
        .map_err(|e| format!("Sync failed: {e}"))
}

// ============================================================================
// Helpers
// ============================================================================

fn apply_write_op(
    db: &crate::storage::Database,
    op: &WriteOp,
) -> Result<(), crate::storage::DatabaseError> {
    match op {
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
    Ok(())
}
