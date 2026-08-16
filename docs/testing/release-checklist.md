# Dexo 1.0 release checklist

Columns: requirement | kind | evidence | result

Every bullet from spec sections 5–19 is listed. Unchecked boxes are product gaps.

## 5.1 Projects and local state
- [x] Create/rename/open/remove projects | test | `crates/dexo-storage/tests/project_repository.rs` | pass
- [x] Associate connections/documents/snippets/favorites/layouts | test | storage + layout tests | pass
- [x] Restore tabs/panes after exit or crash | test | `recovery_crash.rs`, session_recovery | pass
- [x] Open/save SQL files outside Dexo dir | test | document fingerprint tests | pass
- [x] Recents and clear history | test | history repository | pass
- [x] Export/import non-secret config | test | `schema_fixtures.rs` export_import | pass
- [x] Never include secrets in config export | test | connection strip_secret_keys | pass

## 5.2 Connections
- [x] CRUD/duplicate/test/organize connections | test | CLI connections + storage | pass
- [x] Group/project/environment | test | ConnectionProfile | pass
- [x] Environment labels | test | connection_policy | pass
- [x] Host/port/db/user/timeouts | test | connect_request | pass
- [x] Discover server version and capabilities | test | driver handshake + support.rs | pass
- [x] TLS platform CA, custom CA, client cert | test | `dexo-transport/tests/tls.rs` | pass
- [x] Certificate validation default + warning | test | tls tests | pass
- [x] SSH tunnel password/key/agent/known hosts | test | host_key.rs | pass
- [x] Host-key change requires confirmation | test | host_key.rs | pass
- [x] SOCKS5/HTTP CONNECT | test | `dexo-transport/tests/proxy.rs` | pass
- [x] Idle reconnect only when safe | test | session_manager | pass
- [x] Multiple independent sessions | test | session_manager | pass
- [x] Read-only connection/session | test | ConnectRequest.read_only | pass
- [x] Test connection without persisting password | test | CLI connections test + keychain | pass
- [x] Secrets only via keychain | test | dexo-secrets | pass
- [x] Missing/locked keychain prompts per session | test | `secret_store.rs` | pass

## 5.3 Explorer
- [x] Navigate server/database/schema/objects | test | catalog tests | pass
- [x] Lazy tree load | test | explorer snapshots | pass
- [x] Refresh node/subtree/catalog | test | catalog cache | pass
- [x] Filter by name/type/schema/favorite | test | search_service | pass
- [x] Global search ranking | test | catalog_search bench + tests | pass
- [x] Properties/DDL/data/deps | test | inspect CLI + TUI | pass
- [x] Copy simple/qualified/DDL | test | explorer copy | pass
- [x] Navigate SQL ref to object | test | catalog_service | pass
- [x] Effective privileges when available | test | grants inspect | pass
- [x] Local catalog snapshot | test | CatalogCache | pass

## 5.4 Workbench
- [x] Multiple tabs/documents | test | workbench + documents | pass
- [x] Incremental highlighting | test | dexo-sql parse | pass
- [x] Line numbers/search/undo | test | dexo-sql edit | pass
- [x] Indent/comment/pairs | test | dexo-sql | pass
- [x] Statement under cursor | test | statement_at | pass
- [x] Autocomplete | test | completion tests | pass
- [x] Alias/CTE resolution | test | completion | pass
- [x] Signature help | test | completion | pass
- [x] Go to definition | test | catalog | pass
- [x] Format by dialect | test | format_sql | pass
- [x] Local vs server diagnostics | test | diagnostic.rs | pass
- [x] Deterministic quick fixes | test | sql edit | pass
- [x] Named/positional params | test | parameter.rs | pass
- [x] Snippets | test | snippet.rs | pass
- [x] Searchable history | test | HistoryRepository | pass
- [x] Favorite/name/reopen | test | snippets/history | pass
- [x] Recoverable scratch docs | test | recovery documents | pass
- [x] External file change detection | test | has_external_conflict | pass

## 5.5 Execution
- [x] Statement/selection/script | test | query_service + script | pass
- [x] Result sets/notices | test | driver query tests | pass
- [x] Prepared vs direct | test | query executor | pass
- [x] Cancel | test | query cancel | pass
- [x] Timeout/row limit/cache | test | QueryRequest | pass
- [x] Autocommit and manual tx | test | transaction_service | pass
- [x] Visible commit/rollback/error | test | TUI status | pass
- [x] Savepoints | test | validate_savepoint | pass
- [x] No silent session switch in tx | test | session_manager | pass
- [x] No auto-retry mutations | test | query_service | pass
- [x] Duration/TTFB/rows/rate | test | query events | pass
- [x] Background tasks | test | runtime TaskRegistry | pass
- [x] Sequential scripts; explicit parallel | test | ScriptPolicy | pass

## 5.6 Results
- [x] Virtualized grid | test | GridModel viewport | pass
- [x] Column width/freeze/hide | test | grid.rs | pass
- [x] Local sort/filter | test | data screens | pass
- [x] Re-query editable table | test | data mutator | pass
- [x] Copy cell/row/col formats | test | copy tests | pass
- [x] Specialized viewers | test | data inspect | pass
- [x] NULL vs empty vs truncated | test | DbValue + grid | pass
- [x] Large values on demand | test | data inspect | pass
- [x] FK navigation | test | data fk tests | pass
- [x] Multiple result sets | test | query events | pass
- [x] Change set pending until apply | test | data change set | pass
- [x] Review SQL before apply | test | TUI review | pass
- [x] PK required for update/delete | test | mutation | pass
- [x] No identity => read-only | test | mutation | pass
- [x] Affected-row validation | test | mutation | pass
- [x] Concurrency conflicts | test | mutation conflict | pass
- [x] Error preserves remaining edits | test | change set | pass

## 5.7 Schema engineering
- [x] Create/alter/rename/drop | test | ddl tests | pass
- [x] TUI forms + DDL editor | test | schema_editor | pass
- [x] DDL preview | test | schema preview | pass
- [x] Dependency order | test | graph order | pass
- [x] Impact on dependents | test | preview | pass
- [x] Correct quoting | test | PgDialect/MysqlDialect | pass
- [x] Columns/indexes/constraints/FKs | test | ddl render | pass
- [x] Views/routines/triggers/roles | test | ddl + security | pass
- [x] Catalog invalidate after DDL | test | catalog cache | pass
- [x] No comment-preservation promise | manual | docs/src/data-schema.md | pass

## 5.8 Schema diff
- [x] Compare db or snapshot | test | schema_diff | pass
- [x] Filter type/namespace | test | diff | pass
- [x] Conservative normalize | test | normalize | pass
- [x] Preserve driver-specific diffs | test | driver schema_diff tests | pass
- [x] Added/removed/altered | test | diff | pass
- [x] Dependency order + cycles | test | graph | pass
- [x] Bidirectional scripts | test | generate_script | pass
- [x] Destructive/data-loss/lock markers | test | risk | pass
- [x] Save snapshot/report | test | SchemaSnapshotStore | pass
- [x] Apply only after review | test | schema_apply_guard | pass

## 5.9 Transfer
- [x] Import CSV/TSV/JSON/JSONL/SQL | test | transfer import | pass
- [x] Detect encoding/delimiter/header/types | test | detect.rs | pass
- [x] Map columns + preview | test | map.rs | pass
- [x] Error strategy stop/skip/reject | test | rejects.rs | pass
- [x] Batched import + cancel | test | import.rs | pass
- [x] Export those formats | test | export.rs | pass
- [x] Streaming + atomic rename | test | export.rs | pass
- [x] NULL/date/binary/escape control | test | codec.rs | pass
- [x] Optional pg_dump/mysql tools | test | native_tool.rs | pass
- [x] Detect tool/version/args | test | native_tool.rs | pass
- [x] Never pass password on argv | test | threat.rs + native_tool | pass
- [x] Sanitized stdout/stderr + cancel | test | native_tool | pass

## 5.10 Explain
- [x] Explain without execute when possible | test | explain tests | pass
- [x] Extra confirm for ANALYZE | test | explain CLI --confirm | pass
- [x] Prefer JSON | test | parse_explain_json | pass
- [x] Tree/table/text | test | explain screens | pass
- [x] Cost/cardinality/time/loops | test | PlanMetrics | pass
- [x] Estimate vs actual without false certainty | test | explain | pass
- [x] Copy/export plan | test | explain | pass
- [x] Compare two saved plans | test | explain | pass

## 5.11 Admin
- [x] List sessions | test | admin tests | pass
- [x] Active queries/locks | test | admin | pass
- [x] Cancel/terminate with confirm | test | sessions CLI | pass
- [x] Sizes | test | admin | pass
- [x] Stats + collection timestamp | test | admin | pass
- [x] Variables session vs server | test | admin | pass
- [x] Maintenance preview | test | admin preview | pass
- [x] Users/roles/grants | test | security admin | pass
- [x] Capability explains unavailability | test | CapabilityState | pass

## 5.12 CLI
- [x] SQL via arg/file/stdin | test | query CLI | pass
- [x] Params separate from SQL | test | --param | pass
- [x] stdout data vs stderr diag | test | jsonl_query_keeps_diagnostics | pass
- [x] table/csv/tsv/json/jsonl | test | OutputFormat | pass
- [x] Stable exit categories | test | AppError | pass
- [x] --non-interactive never prompts | test | mutating guard | pass
- [x] Destructive needs flag | test | schema_apply_guard | pass
- [x] No TTY disables color/progress | test | capabilities | pass
- [x] Completions | test | `completion` command + help.rs | pass

## 5.13 Accessibility
- [x] Light/dark/16/256/truecolor | test | theme_snapshots | pass
- [x] Color never sole indicator | test | accessibility | pass
- [x] Rebindable keys + conflicts | test | keymap.rs | pass
- [x] Palette with shortcuts | test | palette.rs + command_palette_flow | pass
- [x] Layouts per project | test | layout.rs | pass
- [x] Compact mode | test | clamp | pass
- [x] Disable mouse/animation/unicode | test | settings + Preferences | pass
- [x] NO_COLOR | test | theme snapshots | pass
- [x] Terminal capability diagnosis | test | capabilities.rs | pass

## 5.14 MCP
- [x] Server-only, no sampling | test | mcp protocol | pass
- [x] stdio only, no daemon | test | mcp_stdio | pass
- [x] stdout is JSON-RPC only | test | mcp_stdio | pass
- [x] Sanitized logs off stdout | test | mcp_stdio | pass
- [x] Negotiate implemented caps | test | protocol.rs | pass
- [x] Disconnect cancels/rollback | test | mcp write | pass
- [x] Unknown state not claimed rolled back | test | operation unknown | pass
- [x] Profiles disabled by default | test | mcp profile | pass
- [x] Allowlists and deny-wins | test | policy.rs | pass
- [x] No enumeration of denied objects | test | hidden_error + threat.rs | pass
- [x] Column isolation vs free SQL | test | query_mode | pass
- [x] Grants not created by MCP | test | tools_write | pass
- [x] data_write/ddl/admin independent | test | GrantCapability | pass
- [x] TTL 15m default / 24h max | test | grant.rs | pass
- [x] Single-use consume | test | ledger tests | pass
- [x] Revoke grant/profile/all | test | ledger + TUI | pass
- [x] Re-eval every call | test | mcp service | pass
- [x] operation_id replay/conflict | test | ledger | pass
- [x] Read tools catalog | test | advertised_tools | pass
- [x] Write tools need grant | test | threat.rs | pass
- [x] Audit sanitized | test | audit_repo | pass
- [x] CLI mcp admin tree | test | crates/dexo-cli/tests/mcp.rs | pass

## 6–8 Platform, architecture, persistence
- [x] CI matrix oldest/LTS/newest | test | compatibility-matrix.md + integration.yml | pass
- [x] Outside matrix = unverified | test | support.rs | pass
- [x] Features by capability not version | test | support.rs handshake test | pass
- [x] MariaDB not officially supported | manual | compatibility-matrix.md | pass
- [x] Postgres/MySQL native objects | test | driver catalog tests | pass
- [x] Crate map and dep rule | test | workspace | pass
- [x] Capability traits | test | driver-api contracts | pass
- [x] Canonical types | test | DbValue | pass
- [x] TUI loop never waits on IO | test | event loop | pass
- [x] SQLite stores no secrets | test | sentinel.rs | pass
- [x] Backup before destructive migration | test | database.rs | pass
- [x] TOML preserves unknown keys | test | Preferences | pass
- [x] Keychain opaque ids | test | SecretRef | pass

## 9–11 Flows, security, errors
- [x] Connection open flow | test | CLI/TUI connect | pass
- [x] Query flow | test | query tests | pass
- [x] Data edit flow | test | mutation | pass
- [x] Schema diff flow | test | schema_diff | pass
- [x] MCP call flow | test | mcp_read/write | pass
- [x] Secrets not in sqlite/toml/argv/panic/spans | test | sentinel + threat | pass
- [x] TLS default verify | test | tls.rs | pass
- [x] Production policies | test | connection_policy | pass
- [x] MCP threat controls | test | threat.rs | pass
- [x] Error categories | test | error.rs | pass
- [x] Lost tx => unknown, never reused | test | recovery | pass
- [x] Terminal restored after fatal | test | panic hook | pass

## 12–16 TUI, stack, tests, perf, dist
- [x] Default layout panes | test | snapshots | pass
- [x] Palette + no hidden tx | test | palette/modals + command_palette_contract | pass
- [x] MCP area | test | mcp_audit screen | pass
- [x] Approved stack lockfile | test | Cargo.lock + deny.toml | pass
- [x] Unit/property/fuzz/contract/TUI/CLI/MCP | test | workspace tests | pass
- [x] Perf budgets | test | benches + verify-release | pass
- [x] MIT OR Apache-2.0 | manual | Cargo.toml | pass
- [x] Semver + dist + SBOM | test | dist-workspace + release.yml | pass
- [x] No telemetry | test | diagnostic never-upload | pass

## 17–19 Roadmap, DoD, risks
- [x] Marcos 1–5 delivered | manual | sprints 00–15 | pass
- [x] DoD 1–9 | manual | this checklist | pass
- [x] Risk table mitigations | test | threat + recovery + streaming | pass
