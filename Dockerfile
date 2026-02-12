# Build stage
FROM rust:1.87-alpine AS builder

WORKDIR /app

# Install build dependencies for musl
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconf

# Cache dependencies in a separate layer
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && cargo build --release \
    && rm -rf src

# Build actual source
COPY src ./src
RUN touch src/main.rs src/lib.rs && cargo build --release

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

EXPOSE 8080 8081

CMD ["auth-manager"]
