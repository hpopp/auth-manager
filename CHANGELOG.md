# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] - 2026-02-22

### Added

- Expiration index tables for sessions and API keys, eliminating full table scans during cleanup.

### Changed

- Upgrade muster to v0.2.0.

### Fixed

- Properly propagate deserialization errors in subject_sessions index instead of silently falling back to default.

[#12](https://github.com/hpopp/auth-manager/pull/12)

## [0.7.0] - 2026-02-16

### Fixed

- Properly handle null vs undefined when updating API key description.

### Changed

- Switch storage serialization to map-based MessagePack for forward-compatible schema changes.
  Existing data volumes must be purged.

[#11](https://github.com/hpopp/auth-manager/pull/11)

## [0.6.0] - 2026-02-15

### Changed

- Use [muster](https://github.com/hpopp/muster) crate for multinode clustering.

### Removed

- Dropped support for TOML file configuration. Not useful as you should be running
  in containerized environments with ENV variables.

[#10](https://github.com/hpopp/auth-manager/pull/10)

## [0.5.1] - 2026-02-12

### Changed

- Clustering TCP socket port default to 9993.
- Set Dockerfile uid and gid to 993.
- Faster startup, cluster elections, and heartbeats.

[#9](https://github.com/hpopp/auth-manager/pull/9)

## [0.5.0] - 2026-02-11

### Changed

- Migrated `/_internal` clustering HTTP API to a custom TCP Socket. [#7](https://github.com/hpopp/auth-manager/pull/7)
- Internal refactoring of storage. [#8](https://github.com/hpopp/auth-manager/pull/8)
- Docker image now runs as non-root `auth-manager` user. This will break existing storage volumes. [#8](https://github.com/hpopp/auth-manager/pull/8)

## [0.4.0] - 2026-02-10

### Added

- Sessions now support generic `metadata`.
- Optionally create an API key with a client-provided value. Useful for migrations.
- `GET /api-keys` to list all API keys. Optionally filter by `subject_id`.
- `GET /sessions` to list all sessions. Optionally filter by `subject_id`.
- E2E test suite with Hurl (+ CI job).

### Changed

- Dockerfile and release optimizations.

### Removed

- API keys no longer have a string prefix.
- `GET /api-keys/subject/:subject_id` replaced by generic list route.
- `GET /sessions/subject/:subject_id` replaced by generic list route.

[#6](https://github.com/hpopp/auth-manager/pull/6)

## [0.3.0] - 2026-02-10

### Added

- `limit`/`offset` pagination for listing sessions and API keys by subject. [#4](https://github.com/hpopp/auth-manager/pull/4)
- Snapshot replication for stale followers. [#5](https://github.com/hpopp/auth-manager/pull/5)

### Removed

- Cleaned up various unused legacy functions. [#5](https://github.com/hpopp/auth-manager/pull/5)

## [0.2.0] - 2026-02-09

### Added

- `last_used_at` for both sessions and API keys.
- `ip_address` for sessions.
- `updated_at` for API Keys
- GET by ID routes for both sessions and API keys.

### Changed

- Internal storage now uses MessagePack.
- `TEST_MODE` flag now controls whether the admin purge route is enabled.
- Removed `AUTH_MANAGER_` prefix from most ENV variables.

[#3](https://github.com/hpopp/auth-manager/pull/3)

## [0.1.1] - 2026-02-08

### Fixed

- Gracefully handle `SIGTERM`/`SIGINT` for shutdown. [#2](https://github.com/hpopp/auth-manager/pull/2)

## [0.1.0] - 2026-02-08

Initial release.
