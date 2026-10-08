# MCP

## What it is

`dexo mcp serve` runs an MCP server over stdio. It opens no HTTP listener and embeds no MCP client. It offers tools, one resource (`dexo://profile/capabilities`) and three prompts (`explore_schema`, `review_migration`, `analyze_plan`).

A profile decides what a client may do: which saved connections it may use, which objects it may see, whether it may run raw SQL, and its limits. Profiles start disabled and read-only. A profile writes only through write access a person gives it for one connection, and every write waits for that person's approval. The server never creates, lists or revokes write access.

## Set up

The TUI does all of this on the Agents screen's Profiles view (`Ctrl+G a`, then `3`): `n` makes a profile -- its name, the connections it may use, whether it reads SQL, what it sees -- and connects an agent to it, the agents on this machine listed first. `a` connects another agent to a profile, Enter edits it: its connections, reads, and what it sees -- everything, everything except the tables checked, or only the tables checked. From a shell:

```sh
dexo mcp profile create --name assistant
dexo mcp profile set --name assistant --connection local --query-mode raw-read
dexo mcp allow --profile assistant --selector 'app.public.*'
dexo mcp allow --profile assistant --selector 'app.public.secrets' --deny
dexo mcp profile enable --name assistant --confirm
dexo mcp setup --client claude-code --profile assistant --skill
dexo mcp doctor --probe
```

`mcp setup --client claude-code|codex|cursor|claude-desktop|gemini-cli|windsurf|vscode` merges a `dexo` server entry -- the running binary (through the `dexo` on PATH when that is the same file, so a Homebrew upgrade does not break it), `mcp serve --profile <name>` -- into that client's config file and leaves every other entry as it was -- an existing `dexo` entry keeps its other keys, such as `env` or `cwd`, and only its command and arguments change: the project's `.mcp.json` for Claude Code, `~/.codex/config.toml` for Codex (edited in place, comments kept), `~/.cursor/mcp.json` for Cursor, `claude_desktop_config.json` in the platform's config directory for Claude Desktop, `~/.gemini/settings.json` for Gemini CLI, `~/.codeium/windsurf/mcp_config.json` for Windsurf, and the project's `.vscode/mcp.json` for VS Code (under `servers`, as a `stdio` server). The old file is copied to `<file>.dexo-backup` first (or to `.dexo-backup.1`, `.2` and on, so the file as it was before Dexo is never overwritten), a file Dexo cannot parse is left alone, and `--dry-run` prints what would be written. `--skill` also writes a skill file (`.claude/skills/dexo/SKILL.md`, `~/.codex/skills/dexo/SKILL.md`, or a Cursor rule) telling the agent that access is read-only, that every write waits for a person, and to read the notes on tables. `mcp doctor --probe` starts each enabled profile's server, asks it for its tools the way an agent would, and checks every client's file for a `dexo` entry whose command exists -- a bare command is looked up on PATH, as the client would -- or says the file cannot be parsed; with `--json` the probe comes in the same JSON document as the profiles.

A profile name uses letters, digits, `-` and `_`, since it goes into agents' configs; `profile delete --name assistant` removes a profile with its write access, and the TUI's Agents screen does the same with `x` in its Profiles view. `--expires` takes `90s`, `15m`, `2h`, `1d` or joined, `1h30m`; a bare number is refused.

`profile set` also takes `--max-rows` (1 to 1000000), `--max-bytes` (1 to 33554432), `--timeout-secs` (1 to 3600), `--max-concurrency` (1 to 32), `--allow-tool` and `--deny-tool`; it refuses unknown connections and misspelt tool names, and a connection it drops takes its write access with it. `mcp allow --remove` takes a rule back, and `mcp policy --profile assistant` shows the result -- write access included, connection by connection, and the tools it publishes. `config print` prints a JSON `mcpServers` entry by default, a `claude mcp add` line with `--client claude-code`, or the file another client reads with its `--client` (TOML for Codex, `servers` for VS Code), naming the running binary by its full path, the same way, quoted when it holds a space. `setup` says when the file already said what it would write.

Connections open on the first tool call that needs them. A server whose database is down still starts and reports `CONNECTION_FAILED` for each call, and the next call tries again.

## Selectors

A selector is matched segment by segment against the front of an object's path. Postgres paths are `catalog.schema.object`; MySQL paths are `catalog.object`. `app.public.*` covers every object in schema `public` of database `app`; `app.*` covers the whole database. A segment is an exact name or `*`; partial wildcards such as `pub*` are refused.

A selector matches names exactly as written, and Postgres keeps an unquoted name in lowercase: `app.public.Orders` matches only a table created as `"Orders"`, and `mcp allow` says so. A wide allow such as `app.*` also reaches `pg_catalog`, `information_schema` and `pg_largeobject`; when that matters, give the agent's database user only what it may read.

An object is visible only when an allow rule matches it and no deny rule does: a deny wins however specific the allow is, so `deny app.*` hides even an object that `allow app.public.orders` names. A column belongs to its table, so denying a table hides its columns too. Names in SQL are completed the way the server resolves them: a bare Postgres table is `<database>.public.<table>`, a bare MySQL table is `<database>.<table>`.

## Tools

Every database tool takes an optional `connection`; it is required only when the profile has more than one. A tool the profile cannot see answers `Error [NOT_FOUND]: not found`, the same as an object that does not exist.

| Tool | What it does | Listed |
| --- | --- | --- |
| `list_connections` | The profile's connections, their environment, and whether writes are possible | Always |
| `catalog_list` | Children of a catalog node, live from the server, each with the `id` that `parent_id` takes | Always |
| `catalog_search` | Tables, views, columns, indexes and constraints by name, or by what their notes say, in the connection's indexed catalog | Always |
| `object_describe` | Columns of a table or view, with type, key role and note, and the table's own note | Always |
| `object_get_ddl` | The object's CREATE statement | Always |
| `object_relationships` | What the object depends on and what depends on it | Always |
| `data_read` | One page of a table or view, without SQL | Always |
| `schema_diff` | Differences between two snapshots saved with `dexo schema snapshot` of the connection named | Always |
| `query_validate` | Whether `query_execute_read` would accept a statement, and why not | Raw-read profiles |
| `query_explain` | Estimated plan of one read, without ANALYZE; `hypothetical_indexes` plans as if those indexes were built (Postgres with hypopg), each on a table the profile may read | Raw-read profiles |
| `query_execute_read` | One read-only statement, returned as a table | Raw-read profiles |
| `admin_list_sessions` | Server sessions, without their query text | With `--allow-tool admin_list_sessions`, or while the profile may end `sessions` |
| `data_insert`, `data_update`, `data_delete` | One row, identified by the table's real key | While the profile may change `rows` |
| `data_execute_sql` | One INSERT, UPDATE or DELETE | While the profile may change `rows` |
| `schema_apply_ddl` | One CREATE TABLE, ALTER TABLE, DROP, CREATE INDEX or CREATE VIEW | While the profile may change the `schema` |
| `admin_cancel_query`, `admin_terminate_session` | Cancel or end one server session | While the profile may end `sessions` |

A `--deny-tool` rule hides any of them. Each tool carries a description, a typed input with its required fields, and read-only and destructive hints. The contract is versioned: the server's instructions name the tool schema version. The list changes while a client is connected -- write access given or taken back -- and the server says so: with `notifications/tools/list_changed`, on a `subscriptions/listen` subscription from protocol 2026-07-28.

A call that fails answers `Error [CODE]: message`:

| Code | When |
| --- | --- |
| `NOT_FOUND` | The tool, the object or the connection is hidden from the profile, or does not exist |
| `INVALID_INPUT` | The arguments, SQL that does not parse, or more than one statement |
| `POLICY_DENIED` | A statement the rules refuse -- a locking clause, a function with side effects, `U&` -- or a write denied, revoked or not approved in time |
| `PERMISSION_DENIED` | The database refused it, or a destructive change without its `confirm_target` |
| `CONFLICT` | An `operation_id` reused with another payload, or retried while it runs |
| `CONNECTION_FAILED` | The database is down, or its password is wrong |
| `TIMEOUT`, `CANCELLED` | The profile's `timeout_secs` ran out, or the client cancelled |
| `UNSUPPORTED` | The connection cannot do it |
| `BUSY` | More calls at once than the profile's `max_concurrency`: retry shortly |
| `INTERNAL` | Anything else |

## Reads

A statement is parsed before anything reaches the server. It must be exactly one `SELECT`, `WITH … SELECT`, `VALUES`, `TABLE` or plain `EXPLAIN`; `SELECT … INTO`, locking clauses, `EXPLAIN ANALYZE`, data-modifying CTEs and known side-effecting functions are refused. Every relation it names must be allowed after completing the name; one denied or unknown relation hides the whole statement. A Unicode-escaped identifier (`U&"…"`) is refused outright: the parser keeps it as written while the server decodes it. On Postgres, the session MCP opens has its `search_path` pinned to `public`, the schema a bare name was judged by, for reads and writes alike.

The read then runs inside `BEGIN READ ONLY … ROLLBACK`, which is what stops a function that writes. It returns at most the profile's `max_rows` rows and `max_bytes` bytes, as a Markdown table plus the same rows in `structuredContent`; a cut result says so. `data_read` pages through a table the same way and returns `next_offset` when there is more. A client that cancels a call stops only that call: the query is cancelled and the transaction rolled back.

## Notes

A note says what a table or a column means: one row per paid checkout, amounts in cents, a column nothing reads any more. It is written in the TUI's object inspector (`n`) and kept per connection; without one, the database's own comment (`COMMENT ON`, MySQL's `COMMENT`) is shown and marked as such. `object_describe` returns the notes, and `catalog_search` finds an object by its note -- among the objects the profile lets the agent see, so a note never reveals a hidden one.

## Writes

A profile writes only through write access, created outside the MCP process, for one connection at a time:

```sh
dexo mcp grant create --profile assistant --connection local --allow rows,schema --expires 1h
dexo mcp grant list --profile assistant
dexo mcp grant revoke --profile assistant --connection local
dexo mcp grant revoke-all --profile assistant
```

The TUI gives the same access on the Agents screen's Profiles view: `w` on one of a profile's connections opens Writes -- what it may change, and for how long -- and `r` there takes it back at once. `R` stops every agent write, after asking. Connections MCP never writes to are not offered; the form says which and why.

- `--allow` takes `rows` (`data_insert`, `data_update`, `data_delete` and `data_execute_sql`), `schema` (`schema_apply_ddl`) and `sessions` (`admin_cancel_query`, `admin_terminate_session`), several joined by commas. Each lets only its own tools through; `all` and `*` are refused, and a tool the profile denies with `--deny-tool` is left out.
- Write access is given on a connection the profile uses, and covers everything the profile sees there; it can never reach an object the profile denies. A SQL write or DDL statement has every table it touches held to the profile -- a DDL that adds a foreign key, the table it references too.
- A profile has at most one write access per connection: a new one replaces the old, and the writes waiting under the old one are denied.
- It lasts `--expires`: 15 minutes by default, 24 hours at most, or `never`, until it is revoked. Revoking hides the tools at once and denies the writes waiting under it.
- Every write waits for a person. The server records the request -- the SQL or DDL, or the call's arguments, and the tables -- and waits, without holding the connection, until it is approved or denied on the TUI's Agents screen (`Ctrl+G a` or `Ctrl+Alt+A`), or until `--approval-timeout` seconds (120 by default, 1 to 3600) pass, which refuses it. On Approvals, `a` then Enter approves, and `d` denies at once. Once decided, the request keeps no SQL, and it is deleted after a day. A request is withdrawn when its agent stops waiting -- the call is cancelled, or the server stops or is killed -- and the Agents screen says so instead of approving it. The TUI says when a request arrives while you are on another screen, and counts the waiting ones beside Agents on its top line.
- Production connections, and connections whose own policy is read-only, never accept an MCP write, write access or not. An unknown environment label counts as production.
- Destructive DDL needs `confirm_target` equal to `target`, and ending a session `confirm_target` equal to `session_id`, typed by the client; without it the write is refused before anyone is asked to approve it. MySQL commits DDL implicitly, and the result says so.
- Each write carries an `operation_id`: retrying with the same id and payload returns the first result instead of writing twice; a retry while the first call still runs is refused with `CONFLICT`.
- An approval stands for the write access it was given under: revoked or replaced between the answer and the run, the write is refused.

## Audit

Every tool call writes one event: the tool, decision, status code, duration, rows, bytes, connection and object; the Agents screen's Activity view shows the latest as they arrive. SQL is stored only as a hash. Approval decisions inside write tools have their own events, tagged `grant <tool>` and correlated by operation id; write access given and taken back has its own too (`give_access`, `revoke_access`). `mcp serve` deletes events older than the profile's `audit_retention_days` when it starts; `dexo mcp audit` prints them and deletes nothing.

## Upgrading from 1.4

- The grant commands changed: `grant create` takes `--allow rows,schema,sessions`, and `grant revoke` takes `--connection`. Write access needs a connection the profile lists.
- Write access given before the redesign is revoked when the new version first opens the database; give it again where it is still wanted.
- An older `dexo` refuses a database a newer one wrote, and leaves it as it is, instead of moving it aside. `dexo doctor` names any copy an earlier version moved aside (`dexo.db.unsupported-v*`).
- The tool schema is v4: `catalog_list` has an `id` column, and `schema_diff` takes `connection`.

## Limits of the allowlist

The parse sees the tables a statement names, not what a function or a view reads inside the server. A view over a denied table, or a function that queries one, is not caught by the allowlist. For hard isolation, give the connection a database user that can read only the allowed objects.
