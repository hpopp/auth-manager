[![CI](https://github.com/hpopp/auth-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/hpopp/auth-manager/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.5.1-orange.svg)](https://github.com/hpopp/auth-manager/commits/main)
[![Last Updated](https://img.shields.io/github/last-commit/hpopp/auth-manager.svg)](https://github.com/hpopp/auth-manager/commits/main)

# Auth Manager

A lightweight, clusterable authentication and API key management service.

## Project Dependencies

- Rust 1.93+
- Docker (optional, for clustered deployment)

## Getting Started

1. Clone the repository.

```shell
git clone https://github.com/hpopp/auth-manager && cd auth-manager
```

2. Build the project.

```shell
cargo build --release
```

3. Run the service.

```shell
cargo run --release
```

The service starts on `http://localhost:8080` with sensible defaults. No external database required.

## Contributing

### Testing

Unit and integration tests can be run with `cargo test`.

### Formatting

This project uses `cargo fmt` for formatting.

## Deployment

Deployments require the following environment variables to be set in containers:

| Key                       | Description                                        | Required? | Default             |
| ------------------------- | -------------------------------------------------- | --------- | ------------------- |
| `BIND_ADDRESS`            | HTTP server bind address.                          |           | `0.0.0.0:8080`      |
| `DATA_DIR`                | Data directory for embedded database.              |           | `./data`            |
| `DISCOVERY_DNS_NAME`      | DNS name for peer discovery. Enables DNS strategy. |           |                     |
| `DISCOVERY_POLL_INTERVAL` | Discovery poll interval in seconds.                |           | `5`                 |
| `LOG_FORMAT`              | Log output format: `gcp`, `json`, or `text`.       |           | `text`              |
| `NODE_ID`                 | Unique node identifier.                            |           | Random UUID         |
| `PEERS`                   | Comma-separated static peer addresses.             |           |                     |
| `RUST_LOG`                | Log level filter.                                  |           | `auth_manager=info` |
| `TEST_MODE`               | Enables dangerous operations like purge.           |           | `false`             |

### Liveness

A health check endpoint is available at `/_internal/health`.

### Clustering

For multi-node deployments, set `DISCOVERY_DNS_NAME` to a DNS name that resolves to all
node IPs. This works with Docker Compose service names and Kubernetes headless Services. Alternatively,
use `PEERS` for a static peer list.

The included `docker-compose.yml` runs a 3-node cluster with DNS-based discovery.

### API Documentation

Full API documentation is available in `api-docs/` as a [Bruno](https://www.usebruno.com/) collection.

## License

Copyright (c) 2026 Henry Popp

This project is MIT licensed. See the [LICENSE](LICENSE) for details.
