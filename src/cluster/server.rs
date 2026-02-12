//! TCP server for inter-node cluster communication
//!
//! Accepts incoming TCP connections from peer nodes and dispatches
//! `ClusterMessage` enums to the appropriate handler.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::bytes::Bytes;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info, warn};

use super::handlers;
use super::rpc::ClusterMessage;
use crate::AppState;

/// Start the TCP cluster server on the configured `cluster_port`.
///
/// Returns a `JoinHandle` that runs for the lifetime of the process.
pub fn start_cluster_server(state: Arc<AppState>) -> JoinHandle<()> {
    let port = state.config.cluster.cluster_port;

    tokio::spawn(async move {
        // Bind to all interfaces on the cluster port
        let addr = format!("0.0.0.0:{port}");
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => {
                info!(addr = %addr, "Cluster TCP server listening");
                l
            }
            Err(e) => {
                error!(error = %e, addr = %addr, "Failed to bind cluster TCP server");
                return;
            }
        };

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    debug!(peer = %peer_addr, "Accepted cluster connection");
                    let state = Arc::clone(&state);

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(state, stream).await {
                            debug!(peer = %peer_addr, error = %e, "Cluster connection closed");
                        }
                    });
                }
                Err(e) => {
                    warn!(error = %e, "Failed to accept cluster connection");
                }
            }
        }
    })
}

/// Handle a single peer connection — read messages, dispatch, reply.
async fn handle_connection(
    state: Arc<AppState>,
    stream: tokio::net::TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    stream.set_nodelay(true)?;

    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(64 * 1024 * 1024)
        .new_codec();

    let mut framed = Framed::new(stream, codec);

    while let Some(frame) = framed.next().await {
        let frame = frame?;

        let msg: ClusterMessage = rmp_serde::from_slice(&frame)?;

        let response = dispatch(Arc::clone(&state), msg).await;

        let payload = rmp_serde::to_vec(&response)?;
        framed.send(Bytes::from(payload)).await?;
    }

    Ok(())
}

/// Route a `ClusterMessage` to the correct handler and wrap the result
/// back into a `ClusterMessage` response variant.
async fn dispatch(state: Arc<AppState>, msg: ClusterMessage) -> ClusterMessage {
    match msg {
        ClusterMessage::HeartbeatRequest(req) => {
            let resp = handlers::handle_heartbeat(state, req).await;
            ClusterMessage::HeartbeatResponse(resp)
        }
        ClusterMessage::ReplicateRequest(req) => {
            let resp = match handlers::handle_replicate(state, *req).await {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "Replicate handler error");
                    super::rpc::ReplicateResponse {
                        success: false,
                        sequence: 0,
                    }
                }
            };
            ClusterMessage::ReplicateResponse(resp)
        }
        ClusterMessage::VoteRequest(req) => {
            let resp = handlers::handle_vote(state, req).await;
            ClusterMessage::VoteResponse(resp)
        }
        ClusterMessage::SyncRequest(req) => {
            let resp = match handlers::handle_sync(state, req).await {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "Sync handler error");
                    super::rpc::SyncResponse {
                        leader_sequence: 0,
                        log_entries: vec![],
                        snapshot: None,
                    }
                }
            };
            ClusterMessage::SyncResponse(Box::new(resp))
        }
        // Response variants should never arrive on the server side
        other => {
            warn!(?other, "Unexpected message type received by server");
            // Return a benign heartbeat response as a noop
            ClusterMessage::HeartbeatResponse(super::rpc::HeartbeatResponse {
                term: 0,
                success: false,
                sequence: 0,
            })
        }
    }
}
