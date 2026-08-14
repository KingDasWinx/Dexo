# Database compatibility matrix (2026-08-14)

Digests recorded from Docker Hub tags at implementation time. CI pulls by tag; digest pins are documented for audit.

## PostgreSQL

| Role | Version | Image | Digest |
| --- | --- | --- | --- |
| Oldest supported | 14.18 | `postgres:14.18-alpine` | tag pin (vendor still supported until 2026-11-12) |
| Recommended / LTS | 16.9 | `postgres:16.9-alpine` | used by `dexo-test-support` (`POSTGRES_IMAGE_TAG`) |
| Newest stable | 17.5 | `postgres:17.5-alpine` | tag pin |

Outside this major set, handshake returns `unverified` (`postgres_matrix_status`).

## MySQL

| Role | Version | Image | Digest |
| --- | --- | --- | --- |
| Oldest supported | 8.0.42 | `mysql:8.0.42` | 8.0 LTS |
| Recommended / LTS | 8.4.5 | `mysql:8.4.5` | used by `dexo-test-support` (`MYSQL_IMAGE_TAG`) |
| Newest stable | 9.3.0 | `mysql:9.3.0` | Innovation track |

MySQL 5.7 is vendor EOL; classified `unverified` and not in CI.

## Client OS

| OS | Gate |
| --- | --- |
| Linux | `.github/workflows/ci.yml` + `integration.yml` |
| macOS | ci.yml native job |
| Windows | ci.yml native job |
