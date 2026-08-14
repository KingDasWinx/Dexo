# Security Policy

Report vulnerabilities privately to the repository maintainers. Do not open a public issue for secrets, auth bypass, or MCP policy holes.

## Scope

- Secret handling (keychain, logs, subprocess arguments)
- TLS/SSH verification
- MCP allowlists, grants, and stdout isolation
- Local SQLite integrity

## Response

We will acknowledge the report, assess impact, and ship a patched release when needed. 1.0 does not upload diagnostics or crash reports.
