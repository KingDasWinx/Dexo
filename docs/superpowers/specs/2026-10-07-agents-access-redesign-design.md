# Agents access redesign: write access per connection, every write asked, three views

Status: draft for review. Date: 2026-10-07.

## Why

The Agents screen (Approvals, Activity, Profiles, Setup) was rendered at 120x36 and 80x24
with real data and read against its code. What a person meets today:

- **Profiles is a dump.** `Sees  *`, `Access  read SQL (one SELECT at a time, at most 1000
  rows)`, and grants as wrapped sentences: `schema_apply_ddl on * (pg-dev): one write,
  then it is spent, lasts until it is revoked`.
- **Six buttons on two rows** over the fields. `[q Read SQL off]` does not say whether SQL
  is off or the button turns it off.
- **One grant cannot be revoked**: only all of a profile's (`r`) or all of everyone's (`R`).
- **What a profile sees, its tool rules and its limits are CLI-only.** The TUI shows the
  CLI command as an error (`assistant must allow it first: dexo mcp profile set --name
  assistant --allow-tool data_execute_sql`).
- **New grant** asks for the profile it was opened from, shows raw tool ids grouped by
  "capability", silently makes one grant per capability checked, hides the central choice
  ("one write, then spent" or "ask before each write") in a toggle at its end, offers
  `Tables  Everything assistant sees  *`, and asks the connection's name typed again on
  every grant, development included. At 80x24 only its WHO section shows.
- **Setup** still uses `< value >` choices, which `docs/design.md` forbids, and is a second
  place that creates profiles; `[n New]` on Profiles jumps there.
- **About fourteen concepts** stand between a person and "let Claude write to orders for an
  hour": profile, connections, sees/never, tools, read SQL, grant, capability, the grant's
  tools, tables, expires, ask, wait, confirm, approval.

The reference projects (`dexo-examples-projects/tui`) agree on something simpler: dbhub,
mcp-toolbox, supabase-mcp, postgres-mcp and lazydb model access as a read-only switch per
source with write tools hidden while it is on; mcp-server-mysql adds a flag per operation;
lazydb names why a write is refused (`production_policy`). None has one-shot grants.
Asking before each write (supabase's confirmation) is rare, and is what Dexo keeps.

## Goal and success criteria

A person who uses Claude Code or Codex on their own databases can, without the docs:

1. connect an agent, choosing what it reads and sees, in one form;
2. read on one page what a profile may do on each connection;
3. let it write on one connection for a while, in one short form with no typed
   confirmation, and take that back with one key;
4. approve or deny each write in two keys;
5. see what agents did.

Measured: the concepts drop to six (profile, connections, reads, sees, write access,
approval). Every action is a button or a key printed where it acts. At 80x24 each view
keeps its list, its detail and its keys reachable; under 80 columns it folds to one pane.

Out of scope: approving from the CLI (`dexo mcp approve`), per-connection read modes,
editing limits or `--deny-tool` in the TUI, writes that do not ask.

## Decisions

Taken with the user on 2026-10-07:

| Question | Decision |
| --- | --- |
| Scope | The grant model changes too: dexo-app, storage, CLI, MCP server, docs. |
| Write modes | Only "ask before each write". The one-write grant goes. |
| How many per connection | At most one active write access per profile and connection. |
| Typed confirmation | Removed. The RESULT sentence and the primary button confirm. |
| Tabs | Three: Approvals, Activity, Profiles. Setup moves into Profiles. |
| What a write access covers | Three kinds: rows, schema, sessions. |
| Tables of a write access | None of its own: everything the profile sees on that connection. |
| Profile visibility in the TUI | Editable: Everything, Everything except…, Only…. |
| Profiles layout | F1 Sheet of `cargo run -p dexo-tui --example agents_styles`. |
| Approving | `a` then Enter (Approve focused); `d` denies at once. |

## The model

### Profile: who the agent is and what it reads

Unchanged in storage: name, enabled, connections, query mode, limits, audit retention,
selectors, tool rules.

- **Reads** is the query mode, per profile: *Tables* (structured tools) or *Tables and
  SELECT* (raw read SQL).
- **Sees** is the profile's selectors, written by the TUI in one of three shapes, from the
  tables Dexo last read of its connections' catalogs:
  - *Everything*: `allow *`.
  - *Everything, except…*: `allow *` plus `deny <db.schema.table>` for each table checked.
  - *Only…*: `allow <db.schema.table>` for each table checked.
  A selector names `database.schema.table`, so a table two connections share under the
  same database name is one box, checked in both. A connection whose catalog Dexo never
  read says `Dexo has not read pg-dev's tables: open it once in the Workbench` in place of
  its boxes, as New grant does now.
  Selectors made with `dexo mcp allow` that fit none of the three (a schema wildcard such
  as `allow app.public.*`) are shown as written and kept checked, so saving does not drop
  them unseen.
- `--allow-tool data_execute_sql` is no longer needed: SQL that writes is part of a
  write access's *rows*. `--deny-tool` and the opt-in `admin_list_sessions` stay CLI-only.

### Write access (the grant)

```rust
pub enum WriteKind { Rows, Schema, Sessions }
```

| Kind | Tools |
| --- | --- |
| `rows` | `data_insert`, `data_update`, `data_delete`, `data_execute_sql` |
| `schema` | `schema_apply_ddl` |
| `sessions` | `admin_cancel_query`, `admin_terminate_session` |

A `Grant` keeps `id`, `profile`, `connection`, `tools` (the kinds' tools), `expires_at`,
`revision`, `revoked` and `ask_secs`, and loses `capability`, `selectors` and
`remaining_uses`. It authorizes a write when it is active, on that connection, lists that
tool, and the profile's own policy allows every target. MCP-009 and MCP-011 (DDL and admin
need their own grant) hold through the kinds.

- **Every write asks.** `ask_secs` is always above zero: 120 by default, 1 to 3600 from
  the CLI's `--approval-timeout`. The TUI does not show it.
- **Lasts** 15m, 1h, 2h, 8h, 24h or until revoked. `parse_ttl` is unchanged.
- **One per profile and connection.** `insert_grant` revokes that pair's active grants in
  the same transaction; a write waiting under a replaced grant is denied, as revoking does.
- **Revoke** one (profile and connection), every one of a profile, or every one.
- Production connections and connections kept read-only never take an MCP write: unchanged.

`GrantRequest` becomes `{ connection, kinds, expires, ask_secs }` and its `issue` keeps
the checks that the profile uses the connection and the connection takes writes. It loses
`capability`, `tools`, `selectors` and `confirm_target`.

The `confirm_target` an agent passes with destructive DDL stays: it is the tool's own
guard, not the grant's.

### MCP server

- `visible_tools` lists a write tool while an active grant of the profile lists it on a
  connection that takes writes, as now.
- `covering_grant` is the active grant of the profile on that connection listing the tool;
  every write goes through `await_approval`. `consume` and the "a grant that does not ask is
  used first" rule go.
- The write tools' descriptions and the server's instructions say: write tools appear while
  a person has given write access; every call waits for that person's approval on Dexo's
  Agents screen and is refused when they deny it or the wait runs out.

### Storage: migration 19

```sql
UPDATE mcp_grants SET ask_secs = 120 WHERE ask_secs = 0 AND revoked = 0;
ALTER TABLE mcp_grants DROP COLUMN capability;
ALTER TABLE mcp_grants DROP COLUMN selectors_json;
ALTER TABLE mcp_grants DROP COLUMN remaining_uses;
```

(rusqlite is built with `bundled`, which supports `DROP COLUMN`.) Revoking keeps setting
`revoked = 1`. Older grants made for several tables or capabilities on the same connection
stay as they are; the TUI shows their union as one write access, and the next one made
there replaces them all. A grant that did not ask now asks: that only narrows it.

### CLI

```sh
dexo mcp grant create --profile assistant --connection pg-dev --allow rows,schema \
  --expires 1h [--approval-timeout 120]
dexo mcp grant list --profile assistant
dexo mcp grant revoke --profile assistant --connection pg-dev
dexo mcp grant revoke-all --profile assistant
```

`--capability`, `--tool`, `--selector`, `--confirm-target`, `--ask` and `revoke --id` are
removed: a breaking change, said in the CHANGELOG. `--allow` refuses an unknown kind and
`all`/`*`. `grant list` prints one line per connection: its kinds, how long is left, the
approval wait.

## The screens

Tabs: `1 Approvals · 2 Activity · 3 Profiles`. `Ctrl+G a` still opens Agents. The palette's
"Set Up an Agent (MCP)…" opens Profiles with Connect agent (or New profile when there is no
profile); "New MCP Grant…" becomes "Allow an Agent to Write…" and opens Writes for the
picked profile; "Revoke All MCP Grants" becomes "Stop All Agent Writes".

### Profiles (F1 Sheet)

```
┌▸ Profiles (2)──────────────────────┐┌  assistant─────────────────────────────────────────┐
│ [n New]  [R Stop all writes]       ││ [e Disable]  [⏎ Edit]  [x Delete]                  │
│                                    ││                                                    │
│> ● assistant  2 agents   ✎ writes  ││ Status        ● enabled: agents can connect        │
│  ○ reporting  no agent   disabled  ││ Reads         tables and SELECT, 1000 rows at most │
│                                    ││ Sees          everything, except app.public.secrets│
│                                    ││                                                    │
│                                    ││ CONNECTIONS   each write asks you first · w writes · r revoke
│                                    ││ > pg-dev      development  ✎ rows, schema · 28 min left
│                                    ││   my-dev      development  read only               │
│                                    ││                                                    │
│                                    ││ AGENTS   a connect · y copy its command            │
│                                    ││   ✓ Claude Code    .mcp.json in this folder        │
│                                    ││   ✓ Codex          ~/.codex/config.toml            │
└────────────────────────────────────┘└────────────────────────────────────────────────────┘
```

- **List row**: `●`/`○` enabled, the name, its agents (`no agent`, one name, `N agents`),
  then `✎ writes` when a connection has write access, else `read only` or `disabled`.
  Its buttons: `[n New]`, `[R Stop all writes]`.
- **Detail**: the profile's buttons on top; Status, Reads, Sees as fields (wrapped under
  themselves); then two sections whose titles say their keys.
  - CONNECTIONS: each connection, its environment in its colour, and its writes
    (`✎ rows, schema · 28 min left`, `read only`, or `read only: production`).
  - AGENTS: each agent whose config starts Dexo with this profile, and that file; with
    none, `none: a connects Claude Code, Codex, Cursor and the rest`.
- **Keys**: Tab or → moves into the detail, Esc or ← back; ↑/↓ walk its rows, and the
  detail scrolls to keep the walked row in sight. `w` opens Writes on the walked connection
  (the first that takes writes from the list); `r` revokes the walked connection's access at
  once; `a` opens Connect agent (on the walked agent: set it up again); `y` copies the walked
  agent's own command; `e`, Enter, `x`, `n` and `R` from either pane. `e` enabling, `x` and
  `R` ask first, as now; disabling and `r` do not.
- **Empty**: `No profile yet. An agent connects through a profile, which says what it may
  read.` over `[n New profile]`.
- Under 80 columns the list or the detail shows, whichever has the keys
  (`list_and_detail`).

### Forms

All four follow `docs/design.md` (v4 panel) and take the detail pane. Enter submits only on
the primary button; a refusal focuses its row.

**New profile** (`n`)

```
 WHO
 › Name            │ assistant-2 │            letters, digits, - and _
 WHAT IT READS
   Connections     [x] pg-dev              [ ] my-dev
                   [ ] pg-staging          [ ] pg-prod
                   pg-prod is production: agents read it, never write to it
   Reads           Tables   Tables and SELECT
                   browse and describe, and one SELECT at a time, 1000 rows at most
   Sees            Everything   Everything, except…   Only…
                   (a grid of the checked connections' tables, by connection, when not Everything)
 AGENT
   Connect         │ Codex          found                 ▼ │
                   writes ~/.codex/config.toml
   Skill file      [x] On   tells the agent how Dexo's tools behave
 RESULT
   assistant-2 reads pg-dev: tables and SELECT, everything in its connections.
   Codex starts it, enabled, from ~/.codex/config.toml.
   It cannot write: w on a connection lets it, asking you each time.
                 [   Cancel ]  [   Create profile ]
```

The name defaults to `assistant` or the next free `assistant-N`; the connection in use is
checked; the agent defaults to the first one found here that no profile starts, else None.
Agent's dropdown lists None and the seven clients, each with `set up · <profile>`, `found`
or `not found here`. Picking one another profile starts says so in RESULT (`Claude Code
used assistant until now.`). Create saves the profile enabled, writes the agent's config
(as Setup does now, backups included) and shows what was written in the status line. A
profile made here is enabled because a person made it for an agent, as Setup does now;
`dexo mcp profile create` still makes one disabled (MCP-002).

**Edit** (Enter): New profile without WHO and AGENT; Save. Its name is fixed: agents'
configs start Dexo by it. Unchecking a connection that has write access revokes it, and
RESULT says so.

**Writes** (`w`)

```
 WHERE
   Connection      │ pg-dev      development ✎ has writes   ▼ │
                   pg-prod is not offered: production never takes agents' writes
                   covers everything assistant sees there
 WHAT
 › May             [x] change rows         [x] change schema
                   [ ] end sessions
                   change rows: insert, update, delete, and SQL that writes
 HOW LONG
   For             15m   1h   2h   8h   24h   Until revoked
 RESULT
   assistant may change rows and the schema in pg-dev, asking you before each write,
   for 1 hour.
   Each write waits up to 2 min on Approvals, then is refused.
   Replaces what it has there now: rows, schema, 28 min left.
   ! no time limit: r on the connection takes it back            (Until revoked only)
                  [   Cancel ]  [   Allow writes ]               ("Change writes" when replacing)
```

Connections offered: the profile's, less those that never take writes (each named, with
why). On a connection with write access the form opens on what it has. With none to
offer, `w` says why in the status line instead of opening.

**Connect agent** (`a`): Agent dropdown (as above), where it writes and that the old file is
kept as a backup, `not found on this machine: the file is written for when it is` when so,
Skill file toggle; RESULT `Codex starts Dexo with assistant.`, the profile it replaces if
any, `→ restart Codex once it is written`; `[Connect]`. `y` copies the agent's own command
where it has one.

### Approvals

- List row: time left, profile, the request in words, connection:
  `1:52  assistant  update a row in app.public.orders · pg-dev`. Tool words: insert a row
  into, update a row in, delete a row from, run SQL on, change the schema of, cancel a query
  on, end a session on.
- Detail fields: Profile, Wants (the words), Connection (with its environment), Tables,
  Asked, Left (with `the agent has stopped answering` when so); then the statement in the
  editor's colours, as now.
- `a` opens the confirmation with Approve focused (`a`, Enter); `d` denies at once.
- No `[R Revoke all grants]` here.
- Empty: `No write is waiting.` then who may ask, one line per write access
  (`assistant may write on pg-dev, 28 min left`), or `No agent may write: w on a profile's
  connection lets one.`

### Activity

Unchanged: the audit table, `/` search, profile and outcome filters, the picked call below.

## Code

- `crates/dexo-app/src/mcp/grant.rs`: `WriteKind`, the new `Grant` and `GrantRequest`;
  `GrantCapability` and `why_not_grantable`'s `data_execute_sql` rule go (a `--deny-tool`
  still withholds a kind's tool).
- `crates/dexo-app/src/mcp/ledger.rs`, `crates/dexo-storage/src/mcp/{grant_repo,mod}.rs`:
  replace-on-insert, revoke by profile and connection, no `consume`.
- `crates/dexo-storage/src/migrations.rs`: `MIGRATION_19`.
- `crates/dexo-mcp/src/{tools_write,server}.rs`: the simplified cover, always asking.
- `crates/dexo-cli/src/{args,run}.rs`: the grant commands above.
- `crates/dexo-tui`:
  - `screens/mcp_profiles.rs`: `McpProfileSummary` carries each connection with its
    environment and its write access (kinds, time left, until revoked), and its agents;
    `GrantForm` and `Checklist` give way to `ProfileForm`, `WritesForm`, `AgentForm`.
  - `screens/mcp_setup.rs`: its client rows and `set_up_mcp_client` stay, used by New
    profile and Connect agent; its own form goes.
  - `screen/agents.rs`: three views; Profiles as the Sheet; the forms; Approvals as above.
  - `update.rs`, `runtime/mod.rs`, `action.rs`, `palette/registry.rs`: the keys, effects and
    commands above; `AgentsView::Setup` goes.
  - The profile's tables for Sees come from the catalog snapshot the way New grant reads
    them now (`GrantTablesLoaded`), for each of its connections.
- `examples/agents_styles.rs` is committed as the reference for these screens;
  `examples/grant_styles.rs` is deleted, and `docs/design.md` draws Writes as its whole-form
  example and points to `agents_styles`.

## Docs

`docs/src/mcp.md` (Set up, Writes, the CLI), `docs/src/workbench.md` (the Agents screen),
README (Agents lines), the skill text in `clients.rs` (every write waits for a person on the
Agents screen), CHANGELOG (the model, the CLI break, migration 19).

## Testing

New tests are not written while implementing (the repo's rule). Tests that break are
updated: `dexo-tui/tests/{agents_setup,agents_profiles,admin_settings_mcp_flow,
sprint14_screens,snapshots}.rs`, `dexo-mcp/tests/protocol.rs`, `dexo/tests/mcp_write.rs`,
`dexo-cli/tests/{mcp,dispatch,schema}.rs`, and the unit tests of grant, ledger and the
repositories. These properties keep a test each, adjusted to the model:

- every write waits for approval; a denial, a timeout or the agent's cancel refuses it;
- revoking or replacing a write access denies the writes waiting under it;
- production and read-only connections refuse writes with or without access;
- a write access never reaches a table the profile does not see;
- `--allow all` and an unknown kind are refused.

Each step: `cargo build`, rust-analyzer clean, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo fmt --all -- --check`, `cargo deny check`, the
touched crates' tests, and the Agents views rendered at 120x36 and 80x24.

## Consequences

- An agent writes only while someone has Dexo open to approve within the wait: there is no
  CLI approval and no write that does not ask.
- Scripts using `dexo mcp grant create --capability … --selector …` break, and the
  CHANGELOG says what to write instead.
- A grant that let one write through without asking now asks for each.
