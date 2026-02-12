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
    if let TermCheck::Stale {
        current_term,
        sequence,
    } = check_term(&state, req.term).await
    {
        return HeartbeatResponse {
            term: current_term,
            success: false,
            sequence,
        };
    }

    let (term, my_sequence, leader_sequence, leader_addr) = {
        let mut cluster = state.cluster.write().await;

        if req.term > cluster.current_term || cluster.role != Role::Follower {
            cluster.become_follower(req.term, Some(req.leader_id.clone()));
        }

        cluster.leader_id = Some(req.leader_id);
        if let Some(addr) = req.leader_address {
            cluster.leader_address = Some(addr);
        }
        cluster.update_heartbeat();

        (
            cluster.current_term,
            cluster.last_applied_sequence,
            req.sequence,
            cluster.leader_address.clone(),
        )
    };

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
    if let TermCheck::Stale { sequence, .. } = check_term(&state, req.term).await {
        return Ok(ReplicateResponse {
            success: false,
            sequence,
        });
    }

    {
        let mut cluster = state.cluster.write().await;

        if req.term > cluster.current_term {
            cluster.become_follower(req.term, Some(req.leader_id.clone()));
        }

        cluster.update_heartbeat();
    }

    // Serialize the apply + log-append + sequence-update path.
    // This prevents concurrent replication requests from interleaving,
    // which could write log entries out of order or cause
    // last_applied_sequence to go backwards.
    let _repl_guard = state.replication_lock.lock().await;

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
    if let TermCheck::Stale { current_term, .. } = check_term(&state, req.term).await {
        return VoteResponse {
            term: current_term,
            vote_granted: false,
        };
    }

    let mut cluster = state.cluster.write().await;

    if req.term > cluster.current_term {
        cluster.become_follower(req.term, None);
    }

    let log_up_to_date = req.last_sequence >= cluster.last_applied_sequence;
    let can_vote =
        cluster.voted_for.is_none() || cluster.voted_for.as_deref() == Some(&req.candidate_id);
    let grant = log_up_to_date && can_vote;

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
    super::ensure_leader(&state)
        .await
        .map_err(|e| e.to_string())?;

    catchup::handle_sync_request(state, req.from_sequence)
        .await
        .map_err(|e| format!("Sync failed: {e}"))
}

// ============================================================================
// Helpers
// ============================================================================

/// Result of a term validation check.
enum TermCheck {
    /// The remote term is stale — reject the request.
    Stale { current_term: u64, sequence: u64 },
    /// The term is valid — proceed with the request.
    Ok,
}

/// Pure read-only term check. Returns `Stale` if the incoming term
/// is behind ours, `Ok` otherwise. No side effects.
async fn check_term(state: &AppState, term: u64) -> TermCheck {
    let cluster = state.cluster.read().await;
    if term < cluster.current_term {
        TermCheck::Stale {
            current_term: cluster.current_term,
            sequence: cluster.last_applied_sequence,
        }
    } else {
        TermCheck::Ok
    }
}

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
