//! Pluggable peer discovery strategies
//!
//! Inspired by Elixir's libcluster, this module provides multiple strategies
//! for discovering cluster peers:
//!
//! - **DNS**: Resolves a DNS name to find peers. Works with Docker Compose
//!   service names and Kubernetes headless services.
//! - **Static**: Uses a fixed list of peer addresses from configuration.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use tracing::{info, trace, warn};

/// Peer discovery strategy
pub enum Discovery {
    /// Resolve a DNS name to discover peers (Docker Compose + Kubernetes)
    Dns(DnsPoll),
    /// Use a fixed list of peer addresses (local dev, testing)
    Static(StaticList),
}

impl Discovery {
    /// Discover current cluster peers
    pub async fn discover_peers(&self) -> anyhow::Result<Vec<SocketAddr>> {
        match self {
            Discovery::Dns(d) => d.discover().await,
            Discovery::Static(d) => d.discover().await,
        }
    }
}

/// DNS-based peer discovery
///
/// Resolves a DNS name and returns all addresses, filtering out the local node.
/// This works identically with:
/// - Docker Compose service names (each replica gets a container IP)
/// - Kubernetes headless Services (each Pod gets a stable IP)
pub struct DnsPoll {
    dns_name: String,
    local_ip: Option<IpAddr>,
    port: u16,
}

impl DnsPoll {
    pub fn new(dns_name: String, port: u16) -> Self {
        let local_ip = detect_local_ip();
        if let Some(ip) = local_ip {
            info!(%ip, dns_name = %dns_name, "DNS discovery initialized");
        } else {
            warn!(dns_name = %dns_name, "DNS discovery initialized (could not detect local IP)");
        }
        Self { dns_name, port, local_ip }
    }

    async fn discover(&self) -> anyhow::Result<Vec<SocketAddr>> {
        let lookup = format!("{}:{}", self.dns_name, self.port);

        let addrs: Vec<SocketAddr> = tokio::net::lookup_host(&lookup)
            .await?
            .filter(|addr| {
                // Filter out our own address
                if let Some(local_ip) = self.local_ip {
                    addr.ip() != local_ip
                } else {
                    true
                }
            })
            .collect();

        trace!(
            dns = %self.dns_name,
            peers = addrs.len(),
            "DNS discovery completed"
        );

        Ok(addrs)
    }
}

/// Static peer list discovery
///
/// Returns a fixed list of peer addresses. Supports hostnames — they are
/// resolved via DNS at discovery time, so Docker Compose service names work.
pub struct StaticList {
    peer_addrs: Vec<String>,
}

impl StaticList {
    pub fn new(peer_addrs: Vec<String>) -> Self {
        Self { peer_addrs }
    }

    async fn discover(&self) -> anyhow::Result<Vec<SocketAddr>> {
        let mut peers = Vec::new();
        for addr_str in &self.peer_addrs {
            match tokio::net::lookup_host(addr_str.as_str()).await {
                Ok(addrs) => peers.extend(addrs),
                Err(e) => {
                    warn!(peer = %addr_str, error = %e, "Failed to resolve static peer");
                }
            }
        }
        Ok(peers)
    }
}

/// Detect the local IP address of this node
///
/// Used to filter out self from DNS discovery results.
/// Tries two methods:
/// 1. Resolve the HOSTNAME environment variable (set by Docker and Kubernetes)
/// 2. UDP socket routing table query (works in most network environments)
pub fn detect_local_ip() -> Option<IpAddr> {
    // Method 1: Resolve HOSTNAME env var (Docker/K8s set this automatically)
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        if let Ok(addrs) = (hostname.as_str(), 0u16).to_socket_addrs() {
            for addr in addrs {
                if !addr.ip().is_loopback() {
                    return Some(addr.ip());
                }
            }
        }
    }

    // Method 2: UDP socket trick (queries routing table, no data is actually sent)
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

/// Compute the advertise address for this node
///
/// This is the address other nodes should use to reach us (e.g., for leader forwarding).
/// Priority: AUTH_MANAGER_ADVERTISE_ADDRESS env var > auto-detected IP + bind port
pub fn compute_advertise_address(bind_address: &str) -> String {
    if let Ok(addr) = std::env::var("AUTH_MANAGER_ADVERTISE_ADDRESS") {
        return addr;
    }

    let port = bind_address
        .rsplit(':')
        .next()
        .unwrap_or("8080");

    if let Some(ip) = detect_local_ip() {
        format!("{}:{}", ip, port)
    } else {
        bind_address.to_string()
    }
}
