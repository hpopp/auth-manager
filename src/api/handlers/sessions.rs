use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::replication_error;
use crate::api::response::{ApiError, JSend};
use crate::cluster::replicate_write;
use crate::device::parse_user_agent;
use crate::storage::models::{SessionToken, WriteOp};
use crate::tokens::{generator::generate_token, session};
use crate::AppState;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateSessionRequest {
    pub subject_id: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub expires_at: String,
    pub id: String,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub created_at: String,
    pub device_info: DeviceInfoResponse,
    pub expires_at: String,
    pub id: String,
    pub subject_id: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifySessionRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceInfoResponse {
    pub browser: Option<String>,
    pub browser_version: Option<String>,
    pub kind: String,
    pub os: Option<String>,
    pub os_version: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<JSend<CreateSessionResponse>>, ApiError> {
    let device_info = req
        .user_agent
        .as_deref()
        .map(parse_user_agent)
        .unwrap_or_default();

    let ttl = req
        .ttl_seconds
        .unwrap_or(state.config.tokens.session_ttl_seconds);

    let now = Utc::now();
    let session = SessionToken {
        created_at: now,
        device_info,
        expires_at: now + chrono::Duration::seconds(ttl as i64),
        id: uuid::Uuid::new_v4().to_string(),
        subject_id: req.subject_id.clone(),
        token: generate_token(),
    };

    let operation = WriteOp::CreateSession(session.clone());
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    state
        .db
        .put_session(&session)
        .map_err(|e| ApiError::internal(format!("Failed to store session: {}", e)))?;

    tracing::debug!(id = %session.id, subject_id = %req.subject_id, "Created session token");

    Ok(JSend::success(CreateSessionResponse {
        expires_at: session.expires_at.to_rfc3339(),
        id: session.id,
        token: session.token,
    }))
}

pub async fn validate_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifySessionRequest>,
) -> Result<Json<JSend<SessionResponse>>, ApiError> {
    let token = req.token;
    match session::validate(&state.db, &token) {
        Ok(Some(session)) => Ok(JSend::success(session_to_response(&session))),
        Ok(None) => Err(ApiError::not_found("Session not found or expired")),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

pub async fn revoke_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<JSend<()>>, ApiError> {
    // Resolve UUID id to the internal token value
    let token = state
        .db
        .get_session_token_by_id(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    let operation = WriteOp::RevokeSession {
        token_id: token.clone(),
    };
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    state
        .db
        .delete_session(&token)
        .map_err(|e| ApiError::internal(format!("Failed to delete session: {}", e)))?;

    tracing::debug!(id = %id, "Revoked session token");
    Ok(JSend::success(()))
}

pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Path(subject_id): Path<String>,
) -> Result<Json<JSend<Vec<SessionResponse>>>, ApiError> {
    match session::list_by_subject(&state.db, &subject_id) {
        Ok(sessions) => Ok(JSend::success(
            sessions.iter().map(session_to_response).collect(),
        )),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn session_to_response(session: &SessionToken) -> SessionResponse {
    SessionResponse {
        created_at: session.created_at.to_rfc3339(),
        device_info: DeviceInfoResponse {
            browser: session.device_info.browser.clone(),
            browser_version: session.device_info.browser_version.clone(),
            kind: format!("{:?}", session.device_info.kind),
            os: session.device_info.os.clone(),
            os_version: session.device_info.os_version.clone(),
        },
        expires_at: session.expires_at.to_rfc3339(),
        id: session.id.clone(),
        subject_id: session.subject_id.clone(),
    }
}
