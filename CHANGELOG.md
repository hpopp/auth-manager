# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- Sessions now support generic `metadata`.
- Optionally create an API key with a client-provided value. Useful for migrations.

### Removed

- API keys no longer have a string prefix.

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
