use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::{replication_error, ListParams};
use crate::api::response::{ApiError, AppJson, AppQuery, JSend, JSendPaginated, Pagination};
use crate::cluster::replicate_write;
use crate::device::parse_user_agent;
use crate::storage::models::{SessionToken, WriteOp};
use crate::tokens::{generator::generate_hex, session};
use crate::AppState;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
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
    pub ip_address: Option<String>,
    pub last_used_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
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
    AppJson(req): AppJson<CreateSessionRequest>,
) -> Result<Json<JSend<CreateSessionResponse>>, ApiError> {
    validate_create_session(&req)?;

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
        ip_address: req.ip_address.clone(),
        last_used_at: None,
        metadata: req.metadata.clone(),
        subject_id: req.subject_id.clone(),
        token: generate_hex(32),
    };

    let operation = WriteOp::CreateSession(session.clone());
    replicate_write(Arc::clone(&state), operation)
        .await
        .map_err(replication_error)?;

    state
        .db
        .put_session(&session)
        .map_err(|e| ApiError::internal(format!("Failed to store session: {e}")))?;

    tracing::debug!(id = %session.id, subject_id = %req.subject_id, "Created session token");

    Ok(JSend::success(CreateSessionResponse {
        expires_at: session.expires_at.to_rfc3339(),
        id: session.id,
        token: session.token,
    }))
}

pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<JSend<SessionResponse>>, ApiError> {
    let session = state
        .db
        .get_session_by_id(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    // Filter out expired sessions
    if session.expires_at < Utc::now() {
        return Err(ApiError::not_found("Session not found"));
    }

    Ok(JSend::success(session_to_response(&session)))
}

pub async fn validate_session(
    State(state): State<Arc<AppState>>,
    AppJson(req): AppJson<VerifySessionRequest>,
) -> Result<Json<JSend<SessionResponse>>, ApiError> {
    if req.token.trim().is_empty() {
        return Err(ApiError::bad_request("token is required"));
    }

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
        .map_err(|e| ApiError::internal(format!("Failed to delete session: {e}")))?;

    tracing::debug!(id = %id, "Revoked session token");
    Ok(JSend::success(()))
}

pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    AppQuery(params): AppQuery<ListParams>,
) -> Result<Json<JSendPaginated<SessionResponse>>, ApiError> {
    params.validate()?;

    match session::list(&state.db, params.subject_id.as_deref()) {
        Ok(sessions) => {
            let total = sessions.len() as u64;
            let items: Vec<SessionResponse> = sessions
                .iter()
                .skip(params.offset as usize)
                .take(params.limit as usize)
                .map(session_to_response)
                .collect();

            Ok(JSendPaginated::success(
                items,
                Pagination {
                    limit: params.limit,
                    offset: params.offset,
                    total,
                },
            ))
        }
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn validate_create_session(req: &CreateSessionRequest) -> Result<(), ApiError> {
    if req.subject_id.trim().is_empty() {
        return Err(ApiError::bad_request("subject_id is required"));
    }
    if let Some(ttl) = req.ttl_seconds {
        if ttl == 0 {
            return Err(ApiError::bad_request("ttl_seconds must be greater than 0"));
        }
    }
    Ok(())
}

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
        ip_address: session.ip_address.clone(),
        last_used_at: session.last_used_at.map(|t| t.to_rfc3339()),
        metadata: session.metadata.clone(),
        subject_id: session.subject_id.clone(),
    }
}
