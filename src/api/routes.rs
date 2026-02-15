use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use super::handlers;
use crate::AppState;

pub fn create_router(state: Arc<AppState>) -> Router {
    let mut router = Router::new()
        // API keys
        .route("/api-keys", get(handlers::list_api_keys))
        .route("/api-keys", post(handlers::create_api_key))
        .route("/api-keys/verify", post(handlers::validate_api_key))
        .route("/api-keys/:id", delete(handlers::revoke_api_key))
        .route("/api-keys/:id", get(handlers::get_api_key))
        .route("/api-keys/:id", put(handlers::update_api_key))
        // Sessions
        .route("/sessions", get(handlers::list_sessions))
        .route("/sessions", post(handlers::create_session))
        .route("/sessions/verify", post(handlers::validate_session))
        .route("/sessions/:id", delete(handlers::revoke_session))
        .route("/sessions/:id", get(handlers::get_session))
        // Utilities
        .route(
            "/user-agents/parse",
            post(handlers::parse_user_agent_handler),
        )
        // Internal
        .route("/_internal/cluster/status", get(handlers::cluster_status))
        .route("/_internal/health", get(handlers::health));

    // Test-only routes -- dangerous operations gated behind TEST_MODE
    if state.config.test_mode {
        tracing::warn!("Test mode enabled — purge route is available.");
        router = router.route("/admin/purge", delete(handlers::admin_purge));
    }

    router.layer(TraceLayer::new_for_http()).with_state(state)
}
