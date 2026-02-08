use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

use super::handlers;
use super::middleware::leader_forward;
use crate::AppState;

pub fn create_router(state: Arc<AppState>) -> Router {
    // Write routes -- follower nodes proxy these to the leader
    let write_routes = Router::new()
        .route("/admin/purge", delete(handlers::admin_purge))
        .route("/api-keys", post(handlers::create_api_key))
        .route("/api-keys/:id", delete(handlers::revoke_api_key))
        .route("/sessions", post(handlers::create_session))
        .route("/sessions/:id", delete(handlers::revoke_session))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            leader_forward,
        ));

    // Read routes -- any node can serve these
    let read_routes = Router::new()
        .route(
            "/api-keys/resource/:resource_id",
            get(handlers::list_api_keys),
        )
        .route("/api-keys/verify", post(handlers::validate_api_key))
        .route(
            "/sessions/resource/:resource_id",
            get(handlers::list_sessions),
        )
        .route("/sessions/verify", post(handlers::validate_session))
        .route(
            "/user-agents/parse",
            post(handlers::parse_user_agent_handler),
        );

    // Internal routes -- cluster communication, never forwarded
    let internal_routes = Router::new()
        .route("/_internal/cluster/status", get(handlers::cluster_status))
        .route("/_internal/health", get(handlers::health))
        .route("/_internal/heartbeat", post(handlers::internal_heartbeat))
        .route("/_internal/replicate", post(handlers::internal_replicate))
        .route("/_internal/vote", post(handlers::internal_vote));

    Router::new()
        .merge(write_routes)
        .merge(read_routes)
        .merge(internal_routes)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
