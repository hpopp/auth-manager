use axum::{
    routing::{delete, get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use crate::AppState;
use super::handlers;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Session endpoints (specific routes before parameterized ones)
        .route("/sessions", post(handlers::create_session))
        .route("/sessions/resource/:resource_id", get(handlers::list_sessions))
        .route("/sessions/:token", get(handlers::validate_session))
        .route("/sessions/:token", delete(handlers::revoke_session))
        // API key endpoints
        .route("/api-keys", post(handlers::create_api_key))
        .route("/api-keys/:key", get(handlers::validate_api_key))
        .route("/api-keys/:key", delete(handlers::revoke_api_key))
        // Health and cluster endpoints
        .route("/health", get(handlers::health))
        .route("/cluster/status", get(handlers::cluster_status))
        // Admin endpoints (for testing)
        .route("/admin/purge", delete(handlers::admin_purge))
        // Internal cluster endpoints
        .route("/internal/replicate", post(handlers::internal_replicate))
        .route("/internal/heartbeat", post(handlers::internal_heartbeat))
        .route("/internal/vote", post(handlers::internal_vote))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
