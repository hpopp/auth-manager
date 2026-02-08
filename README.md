[![Version](https://img.shields.io/badge/version-0.1.0-orange.svg)](https://github.com/hpopp/auth-manager/commits/main)
[![Last Updated](https://img.shields.io/github/last-commit/hpopp/auth-manager.svg)](https://github.com/hpopp/auth-manager/commits/main)

# Auth Manager

A lightweight, clusterable authentication and API key management service.

## Project Dependencies

- Rust 1.87+
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

| Key                                    | Description                                                   | Required? | Default             |
| -------------------------------------- | ------------------------------------------------------------- | --------- | ------------------- |
| `AUTH_MANAGER_BIND_ADDRESS`            | HTTP server bind address.                                     |           | `0.0.0.0:8080`      |
| `AUTH_MANAGER_DATA_DIR`                | Data directory for embedded database.                         |           | `./data`            |
| `AUTH_MANAGER_DISCOVERY_DNS_NAME`      | DNS name for peer discovery. Enables DNS strategy.            |           |                     |
| `AUTH_MANAGER_DISCOVERY_POLL_INTERVAL` | Discovery poll interval in seconds.                           |           | `5`                 |
| `AUTH_MANAGER_NODE_ID`                 | Unique node identifier.                                       |           | Random UUID         |
| `AUTH_MANAGER_PEERS`                   | Comma-separated static peer addresses.                        |           |                     |
| `LOG_FORMAT`                           | Log output format. Set to `json` for structured JSON logging. |           | `text`              |
| `RUST_LOG`                             | Log level filter.                                             |           | `auth_manager=info` |

### Liveness

A health check endpoint is available at `/_internal/health`.

### Clustering

For multi-node deployments, set `AUTH_MANAGER_DISCOVERY_DNS_NAME` to a DNS name that resolves to all
node IPs. This works with Docker Compose service names and Kubernetes headless Services. Alternatively,
use `AUTH_MANAGER_PEERS` for a static peer list.

The included `docker-compose.yml` runs a 3-node cluster with DNS-based discovery.

### API Documentation

Full API documentation is available in `api-docs/` as a [Bruno](https://www.usebruno.com/) collection.

## License

Copyright (c) 2026 Henry Popp

This project is MIT licensed. See the [LICENSE](LICENSE) for details.
