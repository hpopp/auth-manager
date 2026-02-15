use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::response::{ApiError, AppJson, JSend};
use crate::device::parse_user_agent;
use crate::AppState;

use super::sessions::DeviceInfoResponse;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub node_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ClusterStatusResponse {
    pub cluster_size: usize,
    pub node_id: String,
    pub peers: Vec<PeerStatus>,
    pub quorum: usize,
    pub role: String,
    pub sequence: u64,
    pub term: u64,
}

#[derive(Debug, Serialize)]
pub struct PeerStatus {
    pub id: String,
    pub sequence: u64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct PurgeResponse {
    pub api_keys_deleted: u64,
    pub sessions_deleted: u64,
}

#[derive(Debug, Deserialize)]
pub struct ParseUserAgentRequest {
    pub user_agent: String,
}

// ============================================================================
// Handlers
// ============================================================================

pub async fn health(State(state): State<Arc<AppState>>) -> Json<JSend<HealthResponse>> {
    JSend::success(HealthResponse {
        node_id: state.config.node.id.clone(),
        status: "healthy".to_string(),
    })
}

pub async fn cluster_status(
    State(state): State<Arc<AppState>>,
) -> Json<JSend<ClusterStatusResponse>> {
    let info = state.node.cluster_info().await;
    let cluster_size = info.peers.len() + 1;

    JSend::success(ClusterStatusResponse {
        cluster_size,
        node_id: info.node_id,
        peers: info
            .peers
            .iter()
            .map(|p| PeerStatus {
                id: p.id.clone(),
                sequence: p.sequence,
                status: p.status.clone(),
            })
            .collect(),
        quorum: cluster_size / 2 + 1,
        role: format!("{:?}", info.role),
        sequence: info.sequence,
        term: info.term,
    })
}

pub async fn admin_purge(
    State(state): State<Arc<AppState>>,
) -> Result<Json<JSend<PurgeResponse>>, ApiError> {
    match state.db.purge_all() {
        Ok(stats) => {
            tracing::warn!(
                sessions = stats.sessions,
                api_keys = stats.api_keys,
                "Purged all data"
            );
            Ok(JSend::success(PurgeResponse {
                api_keys_deleted: stats.api_keys,
                sessions_deleted: stats.sessions,
            }))
        }
        Err(e) => Err(ApiError::internal(format!("Failed to purge data: {e}"))),
    }
}

pub async fn parse_user_agent_handler(
    AppJson(req): AppJson<ParseUserAgentRequest>,
) -> Result<Json<JSend<DeviceInfoResponse>>, ApiError> {
    if req.user_agent.trim().is_empty() {
        return Err(ApiError::bad_request("user_agent is required"));
    }

    let device_info = parse_user_agent(&req.user_agent);
    Ok(JSend::success(DeviceInfoResponse {
        browser: device_info.browser,
        browser_version: device_info.browser_version,
        kind: format!("{:?}", device_info.kind),
        os: device_info.os,
        os_version: device_info.os_version,
    }))
}
