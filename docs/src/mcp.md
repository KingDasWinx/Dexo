# MCP

Dexo is an MCP server only (`stdio`). There is no HTTP listener.

Profiles start disabled and read-only. Write tools require a temporary grant created in the TUI or CLI. The MCP process cannot create grants, list objects outside the allowlist, or put secrets on stdout.

`dexo mcp config print` emits a client snippet. Audit logs stay local and sanitized.
