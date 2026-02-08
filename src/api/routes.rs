use axum::{
    routing::{delete, get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use super::handlers;
use crate::AppState;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/_internal/cluster/status", get(handlers::cluster_status))
        .route("/_internal/health", get(handlers::health))
        .route("/_internal/heartbeat", post(handlers::internal_heartbeat))
        .route("/_internal/replicate", post(handlers::internal_replicate))
        .route("/_internal/vote", post(handlers::internal_vote))
        .route("/admin/purge", delete(handlers::admin_purge))
        .route("/api-keys", post(handlers::create_api_key))
        .route(
            "/api-keys/resource/:resource_id",
            get(handlers::list_api_keys),
        )
        .route("/api-keys/:key", get(handlers::validate_api_key))
        .route("/api-keys/:key", delete(handlers::revoke_api_key))
        .route("/sessions", post(handlers::create_session))
        .route(
            "/sessions/resource/:resource_id",
            get(handlers::list_sessions),
        )
        .route("/sessions/:token", get(handlers::validate_session))
        .route("/sessions/:token", delete(handlers::revoke_session))
        .route(
            "/user-agents/parse",
            post(handlers::parse_user_agent_handler),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
