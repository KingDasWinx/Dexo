# Security Policy

## Reporting a vulnerability

Do not open a public issue or use the bug report form for a security problem.

Report it privately through GitHub: open the **Security** tab and choose **Report a vulnerability**, or go straight to [the private report form](https://github.com/KingDasWinx/Dexo/security/advisories/new). Only the maintainers see the report.

Please include the Dexo version (`dexo --version`), your operating system, the steps to reproduce, and what an attacker could do with it.

## Scope

- Secret handling (keychain, logs, subprocess arguments)
- TLS/SSH verification
- MCP allowlists, grants, and stdout isolation
- Local SQLite integrity

## Response

We will acknowledge the report, assess its impact, and ship a patched release when needed. Fixes go into the latest release. Dexo does not upload diagnostics or crash reports.
