# auth-manager

A tiny, fault-tolerant distributed token management service written in Rust.

## Features

- **Session Tokens** - Generate and validate opaque session tokens with automatic expiration
- **API Keys** - Create and manage API keys with optional expiration
- **Device Tracking** - Parse User-Agent strings to identify device type, OS, and browser
- **Embedded Storage** - Uses [redb](https://github.com/cberner/redb) for ACID-compliant, crash-safe persistence
- **Cluster Support** - Leader-follower replication with automatic leader election
- **Configurable Quorum** - Supports 1, 3, 5, 7+ node clusters
- **No External Dependencies** - No PostgreSQL, Redis, or other services required

## Quick Start

### Build

```bash
cargo build --release
```

### Run (Single Node)

```bash
# Uses default configuration
cargo run --release

# Or with environment variables
AUTH_MANAGER_NODE_ID=node-1 \
AUTH_MANAGER_BIND_ADDRESS=0.0.0.0:8080 \
AUTH_MANAGER_DATA_DIR=./data \
cargo run --release
```

### Run (Cluster)

```bash
# Node 1
AUTH_MANAGER_NODE_ID=node-1 \
AUTH_MANAGER_BIND_ADDRESS=0.0.0.0:8080 \
AUTH_MANAGER_PEERS=node-2:8080,node-3:8080 \
cargo run --release

# Node 2
AUTH_MANAGER_NODE_ID=node-2 \
AUTH_MANAGER_BIND_ADDRESS=0.0.0.0:8080 \
AUTH_MANAGER_PEERS=node-1:8080,node-3:8080 \
cargo run --release

# Node 3
AUTH_MANAGER_NODE_ID=node-3 \
AUTH_MANAGER_BIND_ADDRESS=0.0.0.0:8080 \
AUTH_MANAGER_PEERS=node-1:8080,node-2:8080 \
cargo run --release
```

## Configuration

Configuration can be provided via a TOML file or environment variables.

### Config File

Create a `config.toml` (see `config.example.toml`):

```toml
[node]
id = "node-1"
bind_address = "0.0.0.0:8080"
data_dir = "./data"

[cluster]
peers = ["node-2:8080", "node-3:8080"]
heartbeat_interval_ms = 100
election_timeout_ms = 500

[tokens]
session_ttl_seconds = 86400      # 24 hours
cleanup_interval_seconds = 60
```

### Environment Variables

| Variable                    | Description                    | Default               |
| --------------------------- | ------------------------------ | --------------------- |
| `AUTH_MANAGER_CONFIG`       | Path to config file            | `config.toml`         |
| `AUTH_MANAGER_NODE_ID`      | Unique node identifier         | Random UUID           |
| `AUTH_MANAGER_BIND_ADDRESS` | HTTP server bind address       | `0.0.0.0:8080`        |
| `AUTH_MANAGER_DATA_DIR`     | Data directory path            | `./data`              |
| `AUTH_MANAGER_PEERS`        | Comma-separated peer addresses | (empty = single-node) |

## API Reference

### Session Tokens

#### Create Session

```bash
curl -X POST http://localhost:8080/sessions \
  -H "Content-Type: application/json" \
  -d '{
    "resource_id": "user-123",
    "user_agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)...",
    "ttl_seconds": 3600
  }'
```

Response:

```json
{
  "token": "a1b2c3d4e5f6...",
  "expires_at": "2024-01-15T12:00:00Z"
}
```

#### Validate Session

```bash
curl http://localhost:8080/sessions/{token}
```

Response:

```json
{
  "token": "a1b2c3d4e5f6...",
  "resource_id": "user-123",
  "created_at": "2024-01-15T11:00:00Z",
  "expires_at": "2024-01-15T12:00:00Z",
  "device_info": {
    "kind": "Desktop",
    "os": "Mac OS X",
    "os_version": "10.15",
    "browser": "Chrome",
    "browser_version": "120"
  }
}
```

#### Revoke Session

```bash
curl -X DELETE http://localhost:8080/sessions/{token}
```

#### List Sessions by Resource

```bash
curl http://localhost:8080/sessions/resource/{resource_id}
```

### API Keys

#### Create API Key

```bash
curl -X POST http://localhost:8080/api-keys \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My API Key",
    "expires_in_days": 365
  }'
```

Response:

```json
{
  "key": "am_a1b2c3d4e5f6...",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "My API Key",
  "expires_at": "2025-01-15T12:00:00Z"
}
```

> **Important:** The `key` value is only returned once at creation time. Store it securely.

#### Validate API Key

```bash
curl http://localhost:8080/api-keys/{key}
```

#### Revoke API Key

```bash
curl -X DELETE http://localhost:8080/api-keys/{key}
```

### Health & Cluster Status

#### Health Check

```bash
curl http://localhost:8080/health
```

Response:

```json
{
  "status": "healthy",
  "node_id": "node-1"
}
```

#### Cluster Status

```bash
curl http://localhost:8080/cluster/status
```

Response:

```json
{
  "node_id": "node-1",
  "role": "Leader",
  "term": 5,
  "sequence": 12847,
  "cluster_size": 3,
  "quorum": 2,
  "peers": [
    { "id": "node-2", "status": "synced", "sequence": 12847 },
    { "id": "node-3", "status": "synced", "sequence": 12847 }
  ]
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Load Balancer                        │
└─────────────────────────────────────────────────────────┘
         │              │              │
         ▼              ▼              ▼
    ┌─────────┐    ┌─────────┐    ┌─────────┐
    │ Node 1  │    │ Node 2  │    │ Node 3  │
    │ (Leader)│◄──►│(Follower)│◄──►│(Follower)│
    │         │    │         │    │         │
    │ [redb]  │    │ [redb]  │    │ [redb]  │
    └─────────┘    └─────────┘    └─────────┘
```

### Consistency Model

- **Writes** (create/revoke) go to the leader and are replicated to a quorum before confirming
- **Reads** (validate) can be served by any node from local storage
- **Revocations** wait for all nodes to confirm (security-critical)

### Quorum Calculation

| Cluster Size | Quorum | Fault Tolerance  |
| ------------ | ------ | ---------------- |
| 1            | 1      | 0 (single-node)  |
| 3            | 2      | 1 node can fail  |
| 5            | 3      | 2 nodes can fail |
| 7            | 4      | 3 nodes can fail |

## Development

### Run Tests

```bash
cargo test
```

### Run with Debug Logging

```bash
RUST_LOG=auth_manager=debug cargo run
```

## Resource Usage

- **Binary size:** ~15MB (release build)
- **Memory:** ~20-50MB per node
- **Storage:** Single `auth-manager.redb` file in data directory

## License

MIT
