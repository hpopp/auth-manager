# Build stage
FROM rust:1-alpine AS builder

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

# Create non-root user and directories
RUN addgroup -S -g 993 auth-manager && adduser -S -u 993 auth-manager -G auth-manager \
    && mkdir -p /app /data \
    && chown auth-manager:auth-manager /app /data

WORKDIR /app

# Copy binary from builder
COPY --from=builder --chown=auth-manager:auth-manager /app/target/release/auth-manager /usr/local/bin/auth-manager

USER auth-manager

EXPOSE 8080 9993

CMD ["auth-manager"]
