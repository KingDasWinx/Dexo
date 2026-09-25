# MCP

## What it is

`dexo mcp serve` runs an MCP server over stdio. It opens no HTTP listener and embeds no MCP client. It offers tools, one resource (`dexo://profile/capabilities`) and three prompts (`explore_schema`, `review_migration`, `analyze_plan`).

A profile decides what a client may do: which saved connections it may use, which objects it may see, whether it may run raw SQL, and its limits. Profiles start disabled and read-only. The server never creates, lists or revokes grants.

## Set up

```sh
dexo mcp profile create --name assistant
dexo mcp profile set --name assistant --connection local --query-mode raw-read
dexo mcp allow --profile assistant --selector 'app.public.*'
dexo mcp allow --profile assistant --selector 'app.public.secrets' --deny
dexo mcp profile enable --name assistant --confirm
dexo mcp config print --profile assistant --client claude-code
```

`profile set` also takes `--max-rows`, `--max-bytes`, `--timeout-secs`, `--max-concurrency`, `--allow-tool` and `--deny-tool`; it refuses unknown connections and misspelt tool names. `mcp allow --remove` takes a rule back, and `mcp policy --profile assistant` shows the result. `config print` prints a JSON `mcpServers` entry by default, or a `claude mcp add` line with `--client claude-code`, naming the running binary by its full path.

Connections open on the first tool call that needs them. A server whose database is down still starts and reports `CONNECTION_FAILED` for each call, and the next call tries again.

## Selectors

A selector is matched segment by segment against the front of an object's path. Postgres paths are `catalog.schema.object`; MySQL paths are `catalog.object`. `app.public.*` covers every object in schema `public` of database `app`; `app.*` covers the whole database. A segment is an exact name or `*`; partial wildcards such as `pub*` are refused.

An object is visible only when an allow rule matches it and no deny rule does: a deny wins however specific the allow is, so `deny app.*` hides even an object that `allow app.public.orders` names. A column belongs to its table, so denying a table hides its columns too. Names in SQL are completed the way the server resolves them: a bare Postgres table is `<database>.public.<table>`, a bare MySQL table is `<database>.<table>`.

## Tools

Every database tool takes an optional `connection`; it is required only when the profile has more than one. A tool the profile cannot see answers `Error [NOT_FOUND]: not found`, the same as an object that does not exist.

| Tool | What it does | Listed |
| --- | --- | --- |
| `list_connections` | The profile's connections, their environment, and whether writes are possible | Always |
| `catalog_list` | Children of a catalog node, live from the server | Always |
| `catalog_search` | Table, view and column names in the connection's indexed catalog | Always |
| `object_describe` | Columns of a table or view, with type and key role | Always |
| `object_get_ddl` | The object's CREATE statement | Always |
| `object_relationships` | What the object depends on and what depends on it | Always |
| `data_read` | One page of a table or view, without SQL | Always |
| `schema_diff` | Differences between two snapshots saved with `dexo schema snapshot` | Always |
| `query_validate` | Whether `query_execute_read` would accept a statement, and why not | Raw-read profiles |
| `query_explain` | Estimated plan of one read, without ANALYZE | Raw-read profiles |
| `query_execute_read` | One read-only statement, returned as a table | Raw-read profiles |
| `admin_list_sessions` | Server sessions, without their query text | With `--allow-tool admin_list_sessions` |
| `data_insert`, `data_update`, `data_delete` | One row, identified by the table's real key | While a `data_write` grant is active |
| `data_execute_sql` | One INSERT, UPDATE or DELETE | While a `data_write` grant is active, and with `--allow-tool data_execute_sql` |
| `schema_apply_ddl` | One CREATE TABLE, ALTER TABLE, DROP, CREATE INDEX or CREATE VIEW | While a `ddl` grant is active |
| `admin_cancel_query`, `admin_terminate_session` | Cancel or end one server session | While an `admin` grant is active |

A `--deny-tool` rule hides any of them. Each tool carries a description, a typed input with its required fields, and read-only and destructive hints. The contract is versioned: the server's instructions name the tool schema version.

## Reads

A statement is parsed before anything reaches the server. It must be exactly one `SELECT`, `WITH … SELECT`, `VALUES`, `TABLE` or plain `EXPLAIN`; `SELECT … INTO`, locking clauses, `EXPLAIN ANALYZE`, data-modifying CTEs and known side-effecting functions are refused. Every relation it names must be allowed after completing the name; one denied or unknown relation hides the whole statement.

The read then runs inside `BEGIN READ ONLY … ROLLBACK`, which is what stops a function that writes. It returns at most the profile's `max_rows` rows and `max_bytes` bytes, as a Markdown table plus the same rows in `structuredContent`; a cut result says so. `data_read` pages through a table the same way and returns `next_offset` when there is more. A client that cancels a call stops only that call: the query is cancelled and the transaction rolled back.

## Writes

A write needs a grant, created outside the MCP process:

```sh
dexo mcp grant create --profile assistant --connection local --capability data_write \
  --tool data_insert --selector 'app.public.orders' --expires 15m --confirm-target local
```

- Capabilities are `data_write`, `ddl` and `admin`; each allows only its own tools.
- A grant is used once and expires after `--expires` (15 minutes by default, 24 hours at most). `dexo mcp grant list`, `revoke --id` and `revoke-all` manage them; revoking hides the tools at once.
- A grant is bound to one connection and narrows the profile; it can never reach an object the profile denies. A SQL write or DDL statement has every table it touches held to both.
- Production connections, and connections whose own policy is read-only, never accept an MCP write, grant or not. An unknown environment label counts as production.
- Destructive DDL needs `confirm_target` equal to `target`, typed by the client. MySQL commits DDL implicitly, and the result says so.
- Each write carries an `operation_id`: retrying with the same id and payload returns the first result instead of writing twice.

## Audit

Every tool call writes one event: the tool, decision, status code, duration, rows, bytes, connection and object. SQL is stored only as a hash. Grant decisions inside write tools have their own events, tagged `grant <tool>` and correlated by operation id. `mcp serve` deletes events older than the profile's `audit_retention_days` when it starts; `dexo mcp audit` prints them and deletes nothing.

## Limits of the allowlist

The parse sees the tables a statement names, not what a function or a view reads inside the server. A view over a denied table, or a function that queries one, is not caught by the allowlist. For hard isolation, give the connection a database user that can read only the allowed objects.
