use axum::{extract::State, Json};
use chrono::Utc;
use std::sync::Arc;

use crate::api::response::ApiError;
use crate::cluster::catchup;
use crate::cluster::rpc::{
    HeartbeatRequest, HeartbeatResponse, ReplicateRequest, ReplicateResponse, SyncRequest,
    SyncResponse, VoteRequest, VoteResponse,
};
use crate::cluster::Role;
use crate::storage::models::{ReplicatedWrite, WriteOp};
use crate::AppState;

// ============================================================================
// Replication handler (called by leader to replicate to followers)
// ============================================================================

pub async fn internal_replicate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReplicateRequest>,
) -> Result<Json<ReplicateResponse>, ApiError> {
    let mut cluster = state.cluster.write().await;

    if req.term < cluster.current_term {
        return Ok(Json(ReplicateResponse {
            success: false,
            sequence: cluster.last_applied_sequence,
        }));
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
                return Err(ApiError::internal(format!(
                    "Failed to append to replication log: {e}"
                )));
            }

            let mut cluster = state.cluster.write().await;
            cluster.last_applied_sequence = req.sequence;

            tracing::debug!(sequence = req.sequence, "Applied replicated write");

            Ok(Json(ReplicateResponse {
                success: true,
                sequence: req.sequence,
            }))
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to apply replicated write");
            Err(ApiError::internal(format!("Failed to apply write: {e}")))
        }
    }
}

// ============================================================================
// Heartbeat handler (called by leader to maintain leadership)
// ============================================================================

pub async fn internal_heartbeat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HeartbeatRequest>,
) -> Json<HeartbeatResponse> {
    let mut cluster = state.cluster.write().await;

    if req.term < cluster.current_term {
        return Json(HeartbeatResponse {
            term: cluster.current_term,
            success: false,
            sequence: cluster.last_applied_sequence,
        });
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
                        catchup::CatchupError::AlreadySyncing => {} // Expected, not an error
                        _ => tracing::warn!(error = %e, "Background sync failed."),
                    }
                }
            });
        }
    }

    Json(HeartbeatResponse {
        term,
        success: true,
        sequence: my_sequence,
    })
}

// ============================================================================
// Vote handler (called during leader election)
// ============================================================================

pub async fn internal_vote(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VoteRequest>,
) -> Json<VoteResponse> {
    let mut cluster = state.cluster.write().await;

    if req.term < cluster.current_term {
        return Json(VoteResponse {
            term: cluster.current_term,
            vote_granted: false,
        });
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

        tracing::debug!(candidate = %req.candidate_id, term = req.term, "Granted vote");
    }

    Json(VoteResponse {
        term: cluster.current_term,
        vote_granted: grant,
    })
}

// ============================================================================
// Sync handler (called by followers to catch up)
// ============================================================================

pub async fn internal_sync(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SyncRequest>,
) -> Result<Json<SyncResponse>, ApiError> {
    // Only leader should serve sync requests
    {
        let cluster = state.cluster.read().await;
        if cluster.role != Role::Leader {
            return Err(ApiError::unavailable("Not the leader."));
        }
    }

    let response = catchup::handle_sync_request(state, req.from_sequence)
        .await
        .map_err(|e| ApiError::internal(format!("Sync failed: {e}")))?;

    Ok(Json(response))
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
