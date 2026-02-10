//! Leader forwarding middleware
//!
//! Transparently proxies write requests to the leader node when
//! the current node is a follower. Applied only to write routes.

use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    middleware::Next,
};
use std::sync::Arc;

use crate::cluster::Role;
use crate::AppState;

/// Middleware that forwards requests to the leader if this node is not the leader.
///
/// When the node is the leader (or running in single-node mode), the request
/// passes through to the handler unchanged. When the node is a follower, the
/// entire request is proxied to the leader and the leader's raw response is
/// returned to the client.
pub async fn leader_forward(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    // Single-node mode: always handle locally
    if state.config.is_single_node() {
        return next.run(request).await;
    }

    // Check if we're the leader
    let leader_addr = {
        let cluster = state.cluster.read().await;
        if cluster.role == Role::Leader {
            None
        } else {
            cluster.get_leader_address()
        }
    };

    let leader_addr = match leader_addr {
        None => return next.run(request).await,
        Some(addr) => addr,
    };

    // Forward the request to the leader
    proxy_to_leader(&state, &leader_addr, request).await
}

/// Proxy an HTTP request to the leader node and return the raw response.
async fn proxy_to_leader(
    state: &Arc<AppState>,
    leader_addr: &str,
    request: Request<Body>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();

    let path = parts.uri.path();
    let url = format!("http://{leader_addr}{path}");

    let client = &state.http_client;
    let mut builder = match parts.method {
        axum::http::Method::POST => client.post(&url),
        axum::http::Method::DELETE => client.delete(&url),
        axum::http::Method::PUT => client.put(&url),
        axum::http::Method::PATCH => client.patch(&url),
        _ => client.get(&url),
    };

    // Forward content-type header
    if let Some(content_type) = parts.headers.get(axum::http::header::CONTENT_TYPE) {
        builder = builder.header(axum::http::header::CONTENT_TYPE, content_type);
    }

    // Forward the body
    let body_bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("Failed to read request body: {e}"),
            );
        }
    };

    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes);
    }

    // Send to leader
    let response = match builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("Failed to forward request to leader: {e}"),
            );
        }
    };

    // Convert reqwest response to axum response
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut axum_response = Response::builder().status(status);

    // Forward response headers
    for (key, value) in response.headers() {
        axum_response = axum_response.header(key, value);
    }

    match response.bytes().await {
        Ok(bytes) => axum_response.body(Body::from(bytes)).unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Response build error".to_string(),
            )
        }),
        Err(e) => error_response(
            StatusCode::BAD_GATEWAY,
            format!("Failed to read leader response: {e}"),
        ),
    }
}

/// Build a JSend error response for middleware failures.
fn error_response(status: StatusCode, message: String) -> Response<Body> {
    let body = serde_json::json!({
        "status": "error",
        "message": message,
    });

    Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap_or_default()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}
