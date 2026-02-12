//! TCP transport layer for inter-node cluster communication
//!
//! Each peer gets a pool of dedicated background tasks, each owning its own
//! TCP connection. Requests are distributed across the pool via round-robin,
//! allowing multiple in-flight requests per peer for higher throughput.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_util::bytes::{Bytes, BytesMut};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::debug;

use super::rpc::ClusterMessage;

/// Number of TCP connections per peer
const POOL_SIZE: usize = 4;

/// Error type for transport operations
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Connection failed: {0}")]
    Connect(std::io::Error),
    #[error("Serialization error: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("Deserialization error: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("Connection closed by peer")]
    ConnectionClosed,
    #[error("Peer task unavailable")]
    PeerUnavailable,
}

/// A framed TCP connection to a peer
type PeerConnection = Framed<TcpStream, LengthDelimitedCodec>;

/// A request sent to a peer's background task
type PeerRequest = (Vec<u8>, oneshot::Sender<Result<Vec<u8>, TransportError>>);

/// Handle to a peer's connection pool.
///
/// Each pool contains `POOL_SIZE` background tasks, each owning a separate TCP
/// connection. Requests are distributed round-robin across the pool.
struct PeerHandle {
    senders: Vec<mpsc::Sender<PeerRequest>>,
    next: AtomicUsize,
}

impl PeerHandle {
    /// Pick the next live sender in round-robin order.
    /// Returns `None` only if all connections in the pool are dead.
    fn pick_sender(&self) -> Option<mpsc::Sender<PeerRequest>> {
        let len = self.senders.len();
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        for i in 0..len {
            let idx = (start + i) % len;
            let tx = &self.senders[idx];
            if !tx.is_closed() {
                return Some(tx.clone());
            }
        }
        None
    }
}

/// Manages outbound TCP connections to cluster peers.
///
/// Each peer is served by a pool of `POOL_SIZE` tokio tasks, each owning its
/// own TCP connection. Callers never hold a lock during network I/O — they
/// enqueue a request and await a oneshot response.
pub struct ClusterTransport {
    /// Per-peer connection pool handles, keyed by resolved TCP address
    peers: RwLock<HashMap<String, PeerHandle>>,
    /// The cluster TCP port peers are listening on
    cluster_port: u16,
}

impl ClusterTransport {
    pub fn new(cluster_port: u16) -> Arc<Self> {
        Arc::new(Self {
            peers: RwLock::new(HashMap::new()),
            cluster_port,
        })
    }

    /// Send a message to a peer and wait for a response.
    ///
    /// Distributes requests round-robin across the peer's connection pool.
    pub async fn send(
        &self,
        peer_addr: &str,
        message: ClusterMessage,
    ) -> Result<ClusterMessage, TransportError> {
        let tcp_addr = self.resolve_tcp_addr(peer_addr);
        let payload = rmp_serde::to_vec(&message)?;

        let tx = self.get_or_spawn_peer(&tcp_addr).await;

        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send((payload, reply_tx))
            .await
            .map_err(|_| TransportError::PeerUnavailable)?;

        let response_bytes = reply_rx
            .await
            .map_err(|_| TransportError::PeerUnavailable)??;

        let msg: ClusterMessage = rmp_serde::from_slice(&response_bytes)?;
        Ok(msg)
    }

    /// Send a message without waiting for a response (fire-and-forget).
    pub async fn send_no_reply(
        &self,
        peer_addr: &str,
        message: ClusterMessage,
    ) -> Result<(), TransportError> {
        let tcp_addr = self.resolve_tcp_addr(peer_addr);
        let payload = rmp_serde::to_vec(&message)?;

        let tx = self.get_or_spawn_peer(&tcp_addr).await;

        // Send but drop the reply — the peer task will still process
        // the request and discard the oneshot error when it tries to reply.
        let (reply_tx, _reply_rx) = oneshot::channel();
        tx.send((payload, reply_tx))
            .await
            .map_err(|_| TransportError::PeerUnavailable)?;

        Ok(())
    }

    /// Remove a peer's connection pool (e.g., when peer is known unreachable).
    ///
    /// Drops all handles, which closes all channels, which causes every
    /// background task in the pool to exit and drop its TCP connection.
    pub async fn disconnect(&self, peer_addr: &str) {
        let tcp_addr = self.resolve_tcp_addr(peer_addr);
        self.peers.write().await.remove(&tcp_addr);
    }

    /// Get an existing peer's sender or spawn a new connection pool.
    async fn get_or_spawn_peer(&self, tcp_addr: &str) -> mpsc::Sender<PeerRequest> {
        // Fast path: peer pool already exists (read lock only)
        {
            let peers = self.peers.read().await;
            if let Some(handle) = peers.get(tcp_addr) {
                if let Some(tx) = handle.pick_sender() {
                    return tx;
                }
            }
        }

        // Slow path: need to spawn a new pool (write lock)
        let mut peers = self.peers.write().await;

        // Double-check after acquiring write lock
        if let Some(handle) = peers.get(tcp_addr) {
            if let Some(tx) = handle.pick_sender() {
                return tx;
            }
        }

        let handle = spawn_peer_pool(tcp_addr.to_string());
        let tx = handle
            .pick_sender()
            .expect("freshly spawned pool must have live senders");
        peers.insert(tcp_addr.to_string(), handle);
        tx
    }

    /// Convert a peer's HTTP address (host:8080) to its cluster TCP address (host:cluster_port)
    fn resolve_tcp_addr(&self, peer_addr: &str) -> String {
        if let Some(host) = peer_addr.rsplit_once(':').map(|(h, _)| h) {
            format!("{host}:{}", self.cluster_port)
        } else {
            format!("{peer_addr}:{}", self.cluster_port)
        }
    }
}

impl std::fmt::Debug for ClusterTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClusterTransport")
            .field("cluster_port", &self.cluster_port)
            .finish()
    }
}

// ============================================================================
// Per-peer connection pool
// ============================================================================

/// Spawn a pool of `POOL_SIZE` background tasks for a peer.
///
/// Each task owns its own TCP connection and processes requests sequentially
/// from its own mpsc channel. The pool distributes requests round-robin
/// across the tasks, giving up to `POOL_SIZE` concurrent in-flight requests
/// per peer.
fn spawn_peer_pool(addr: String) -> PeerHandle {
    let mut senders = Vec::with_capacity(POOL_SIZE);

    for conn_idx in 0..POOL_SIZE {
        let (tx, mut rx) = mpsc::channel::<PeerRequest>(64);
        let addr = addr.clone();

        tokio::spawn(async move {
            let mut conn: Option<PeerConnection> = None;

            while let Some((payload, reply_tx)) = rx.recv().await {
                let result = peer_send_recv(&mut conn, &addr, &payload).await;

                // Convert BytesMut to Vec<u8> for the reply
                let result = result.map(|bytes| bytes.to_vec());

                // If the caller dropped the oneshot, that's fine — just discard
                let _ = reply_tx.send(result);
            }

            debug!(peer = %addr, conn_idx, "Peer pool task exiting (channel closed)");
        });

        senders.push(tx);
    }

    PeerHandle {
        senders,
        next: AtomicUsize::new(0),
    }
}

/// Send a payload on the peer connection and read the response.
///
/// Handles lazy connection and automatic reconnection on failure.
async fn peer_send_recv(
    conn: &mut Option<PeerConnection>,
    addr: &str,
    payload: &[u8],
) -> Result<BytesMut, TransportError> {
    // Ensure we have a connection
    if conn.is_none() {
        *conn = Some(connect(addr).await?);
    }

    // Try to send/recv, reconnect once on failure
    match send_and_recv(conn.as_mut().unwrap(), payload).await {
        Ok(resp) => Ok(resp),
        Err(_) => {
            debug!(peer = %addr, "Connection lost, reconnecting");
            *conn = None;
            *conn = Some(connect(addr).await?);
            send_and_recv(conn.as_mut().unwrap(), payload).await
        }
    }
}

/// Create a new framed TCP connection to a peer
async fn connect(addr: &str) -> Result<PeerConnection, TransportError> {
    debug!(peer = %addr, "Connecting to peer");
    let stream = TcpStream::connect(addr)
        .await
        .map_err(TransportError::Connect)?;
    stream.set_nodelay(true).map_err(TransportError::Connect)?;

    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(64 * 1024 * 1024) // 64 MB max (snapshots can be large)
        .new_codec();

    Ok(Framed::new(stream, codec))
}

/// Send a payload and read the response on a framed connection
async fn send_and_recv(
    conn: &mut PeerConnection,
    payload: &[u8],
) -> Result<BytesMut, TransportError> {
    use futures_util::{SinkExt, StreamExt};

    conn.send(Bytes::copy_from_slice(payload))
        .await
        .map_err(|_| TransportError::ConnectionClosed)?;

    match conn.next().await {
        Some(Ok(frame)) => Ok(frame),
        Some(Err(_)) => Err(TransportError::ConnectionClosed),
        None => Err(TransportError::ConnectionClosed),
    }
}
