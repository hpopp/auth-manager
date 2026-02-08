# Build stage
FROM rust:1.87-alpine as builder

WORKDIR /app

# Install build dependencies for musl
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconf

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM alpine:3.21

ARG CREATED
ARG VERSION

LABEL org.opencontainers.image.authors="Henry Popp <henry@hpopp.dev>"
LABEL org.opencontainers.image.created="${CREATED}"
LABEL org.opencontainers.image.description="Lightweight, clusterable authentication and API key management service"
LABEL org.opencontainers.image.documentation="https://github.com/hpopp/auth-manager"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.source="https://github.com/hpopp/auth-manager"
LABEL org.opencontainers.image.title="Auth Manager"
LABEL org.opencontainers.image.url="https://github.com/hpopp/auth-manager"
LABEL org.opencontainers.image.vendor="Henry Popp"
LABEL org.opencontainers.image.version="${VERSION}"

RUN apk add --no-cache ca-certificates curl

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
