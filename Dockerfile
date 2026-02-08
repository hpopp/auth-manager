# Build stage
FROM rust:1.87-slim as builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/auth-manager /usr/local/bin/auth-manager

# Create data directory
RUN mkdir -p /data

ENV AUTH_MANAGER_DATA_DIR=/data
ENV AUTH_MANAGER_BIND_ADDRESS=0.0.0.0:8080
ENV RUST_LOG=auth_manager=info

EXPOSE 8080

CMD ["auth-manager"]
