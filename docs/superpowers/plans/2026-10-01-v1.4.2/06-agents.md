# 1.4.2 Section 6: Agents Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An agent gets Dexo in one command, its writes wait for a person, it reads what a schema means, it can try an index without building one, and editors get Dexo's SQL intelligence over LSP.

**Architecture:** Client setup is a `dexo-app` module that knows each client's config file and format and merges one `dexo` server entry into it; the probe speaks MCP's JSON lines to a spawned `dexo mcp serve`, like the tests' client. Approvals are a storage table both processes poll: the MCP server inserts a pending row and waits without holding a session, the TUI's Agent Activity screen lists pending rows and live audit events and writes the decision. Notes are a storage table keyed by connection id and object; drivers start reading database comments into a `comment` attribute, and the note shown is the user's, else the comment. Hypothetical indexes ride `ExplainRequest`, which the Postgres provider turns into `hypopg_create_index`, EXPLAIN, `hypopg_reset` on its own session. The language server is a hand-written stdio JSON-RPC loop in `dexo-cli` over `dexo-sql`'s completion, diagnostics and formatting.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 6 (E3, E1, E2, E4, E5). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Every document belongs to a connection; offline actions connect by themselves.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies come from crates.io and pass `cargo deny check`.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- MCP's tool schemas are pinned: a changed input bumps `TOOL_SCHEMA_VERSION` and its snapshot.

## Rulings made from the code

- **Client files.** Claude Code: the project's `.mcp.json` (`mcpServers`); Cursor: `~/.cursor/mcp.json`; Claude Desktop: `claude_desktop_config.json` in the platform's config dir (`~/Library/Application Support/Claude`, `%APPDATA%\Claude`, `~/.config/Claude`); Codex: `~/.codex/config.toml` (`[mcp_servers.dexo]`). Setup merges one `dexo` entry (the running executable, `mcp serve --profile <name>`) and leaves every other key as it was; an existing file is copied to `<file>.dexo-backup` first. `--dry-run` prints the file it would write. A file that does not parse is not touched, and the error says so.
- **Skill file.** `--skill` writes `SKILL.md` for Claude Code (`.claude/skills/dexo/`) and Codex (`~/.codex/skills/dexo/`), and a rule file for Cursor (`.cursor/rules/dexo.mdc`); Claude Desktop has no such folder and says so. The text explains the tools, that access is read-only unless a grant says otherwise, and how grants and approvals work.
- **Probe.** `dexo mcp doctor --probe` spawns `dexo mcp serve --profile p` for each enabled profile (or `--profile`), sends `initialize`, `notifications/initialized` and `tools/list` with a 10 s limit, and reports the server's name and the tools; then, per client, whether its file has a `dexo` entry and whether that entry's command exists.
- **Approvals.** A grant created with `--ask` (and from the TUI's grant flow) is reusable until it expires, and every write it covers waits for a decision: the server inserts `mcp_approvals(id, profile, connection, tool, statement, targets, created_at, deadline, decision)` with `decision = 'pending'`, then polls it every 250 ms -- before it leases the connection, so the session stays free -- until it reads `approved` or `denied`, or the deadline (120 s by default, `--approval-timeout`) passes and it marks the row `expired`. A denied or expired write answers `POLICY_DENIED` with the reason. The statement text is kept only while the row is pending and is blanked once decided.
- **Agent Activity.** A TUI screen (palette, `Ctrl+Alt+A`) polls every second: pending approvals at the top with the statement (and the DDL for `schema_apply_ddl`), its targets and the time left, `a` approve / `d` deny each behind a two-button confirmation; below, the latest audit events as they arrive. It replaces the old one-shot audit popup.
- **Notes.** `object_notes(connection_id, object, note, updated_at)` keyed by the profile's id and the object's qualified name. Postgres reads `obj_description`/`col_description`, MySQL `TABLE_COMMENT`/`COLUMN_COMMENT`, into a `comment` attribute. The inspector shows the note (the user's, else the database comment, marked as such) and `n` edits it in a one-line dialog. `object_describe` gains a `note` line and a per-column note column; `catalog_search` returns the note and matches on it.
- **What-if index.** `ExplainRequest.hypothetical_indexes: Vec<String>` (index definitions such as `CREATE INDEX ON orders (customer_id)`); other drivers refuse a non-empty list. The Explain view's "Try index…" (`i`) asks for the definition, checks `pg_extension` for `hypopg` (else says `CREATE EXTENSION hypopg`, after installing the package), pins the current plan as the baseline and shows the new one compared with it. MCP's `query_explain` takes the same optional list through its own input type.
- **Language server.** `dexo lsp [--connection name]`; the first line `-- dexo: connection=name` overrides it per file. Full-text sync; diagnostics are published on open and change through `Diagnoser`; completion from the connection's cached catalog (CLI's cache key first, then the TUI's); formatting with `format_sql`. Positions are converted between bytes and UTF-16. No new dependency: JSON-RPC with Content-Length framing over stdin/stdout, written by hand.

## Tasks

### Task 1 (E3): MCP setup and probe
- `dexo_app::mcp::clients` (paths, merge for JSON and TOML, backup), `dexo mcp setup`, the skill text, `doctor --probe`.
- Commit: `feat(mcp): dexo mcp setup wires Dexo into Claude Code, Codex, Cursor and Claude Desktop`.

### Task 2 (E1): Approval cockpit
- Migration, ask grants, the server's wait, the Agent Activity screen.
- Commit: `feat(mcp): an agent's write can wait for a person to approve it`.

### Task 3 (E2): Semantic notes
- Comments in the Postgres and MySQL catalogs, the notes table and repository, inspector editing, the two MCP tools.
- Commit: `feat(mcp): notes on tables and columns, for people and agents`.

### Task 4 (E4): What-if index
- `ExplainRequest.hypothetical_indexes`, the Postgres provider, the Explain view's Try index, MCP's input.
- Commit: `feat(explain): try an index before building it, on Postgres with hypopg`.

### Task 5 (E5): Language server
- `dexo lsp`: framing, lifecycle, sync, diagnostics, completion, formatting.
- Commit: `feat(lsp): dexo lsp brings completion, diagnostics and formatting to any editor`.

## Review Focus

1. Setup never loses a key of the client's file, never writes a broken one, and backs up first; the probe never leaves a server running.
2. A write on an ask grant never runs without an `approved` row written after it was asked, never runs after its deadline, and never holds the connection while it waits; a decided row keeps no SQL.
3. Notes stay with their connection; a note never leaks a hidden object through `catalog_search`.
4. A hypothetical index never outlives its EXPLAIN, and never reaches a session that was not asked.
5. The language server never blocks on a slow catalog, answers every request id, and its positions are right with multi-byte text.
