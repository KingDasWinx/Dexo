# Screens UX overhaul: Connections, Agents, Server, Compare, History

Status: draft for review. Date: 2026-10-03.

## Why

The five screens next to the workbench were built as dialogs grown large. A user test on
2026-10-03 found the same faults on all of them:

- **Everything is a hotkey.** Actions live in the status line or in a run of plain text at
  the bottom of a pane (`Enter connect  n new  e edit ...`). Nothing looks pressable, and a
  first-time user does not know what the screen can do.
- **Big and empty.** Connections shows four rows on a 140x40 screen and leaves 90% blank.
  The detail pane is five lines of prose.
- **Prose instead of data.** Agents' Setup spends its pane on sentences ("Writes Dexo's
  server into ~/... -- this project's, the folder Dexo started in -- and keeps the old
  file beside it.") and prints a 120-character command.
- **Missing basics.** History shows only the connection in use, records no failures, and
  has no duration, row count, status or filter. Server shows sessions only, cannot cancel
  a query (only terminate), and cannot hide idle sessions. Compare hides its sources once
  it has compared. Profiles cannot be edited.

Measured today: 4 data rows of 37 on Connections; status "offline" on every row; a
120-character `By hand:` line; History shows 2 of 3 statements run.

## Goal and success criteria

Each screen is something a person can use without reading the docs:

1. Every action on a screen is visible as a button where it applies, works with a mouse
   click, and keeps its single-key shortcut. The key is printed inside the button.
2. Details are key/value fields in sections, not sentences. No line of explanatory prose
   longer than one short sentence; hints go to the status line.
3. Lists are tables with a status glyph and the columns that matter, filterable and
   searchable, with an empty state that says what to do.
4. History is global, records failures, and filters by connection, status and text.
5. At 80x24 every screen keeps its list, its detail and its buttons reachable. At 60
   columns it folds to one column. The existing too-small guard (20x8) stays.

Out of scope: drag-and-drop ordering, connection colours beyond the environment, history
date-range pickers, schema-compare deploy-to-server, MCP HTTP transport.

## Shared anatomy

Every screen uses the same four rows of structure, taken from lazydb's footer rule and
lazygit's per-pane hints:

```
 Workbench  Connections [Agents] Server  Compare  History      Default  pg-dev  —   <- header (exists)
 1 Approvals  2 Activity  3 Profiles  4 Setup     / search______  Status: all   [n New]  <- toolbar
┌▸ Profiles (2) ────────────────────┐┌  assistant ────────────────────────────────────┐
│ ● assistant   3 conn  read SQL    ││ [e Disable] [c Connections] [g Grant] [x Delete] <- action bar
│ ○ reviewer    1 conn  browse      ││                                                  │
│                                   ││ Connections  pg-dev, pg-prod, my-dev             │ <- fields
│                                   ││ Access       read SQL (one SELECT, 1000 rows)    │
│                                   ││ Scopes       allow *                             │
│                                   ││ ── Grants                                        │ <- section
│                                   ││ data_write on app.public.orders · 12m left       │
└───────────────────────────────────┘└──────────────────────────────────────────────────┘
 Agents  Up/Down pick  e disable  g grant  x delete  1-4 views  Esc back      Ctrl+P  F1   <- status
```

### Components (new, in `crate::screen`)

- **Toolbar** (row under the header): the screen's views on the left (existing
  `views_bar`), its filters and search in the middle, and screen-wide buttons on the
  right (`[n New]`, `[r Refresh]`). Filters are chips such as `Connection: all` that cycle
  with their key or a click. A filter that is not at its default is shown in the accent
  colour. On narrow widths the buttons go first, then the chips collapse into one
  `Filters (2)` chip; the search box always stays.
- **Action bar** (first row of the detail pane): buttons for the picked item. Each is
  drawn `[k Label]` with the key in the accent colour. When the detail section has the
  keys (Alt+2 or a click), Left and Right move a focus between the buttons and Enter
  presses the focused one. The focused button is reversed and marked `>`, so focus does
  not depend on colour. A button that cannot act now is dimmed. Pressing it says why on
  the status line instead of doing nothing. Buttons wrap to a second row when the pane is
  narrow; a key is never split from its label.
- **Fields**: aligned `Label  value` rows with dimmed labels, plus `── Section` headings
  styled like the connection form's. Rows with no value are left out.
- **Table list**: the existing `list_pane`, with a fixed first column for a status glyph
  and per-column widths that drop low-priority columns as width shrinks. Detail-on-pick
  covers what a dropped column held.
- **Empty states**, in two kinds: nothing yet ("No connections yet." plus the buttons that
  make one) and nothing matches ("Nothing matches the filters." plus `[Esc Clear filters]`).
- **SQL view**: statements in a detail pane are drawn with the editor's syntax colours.
  History, Server, Approvals, Compare and Saved show SQL.

### Glyphs and colours (always paired with a word or letter)

| Meaning | Glyph | Colour |
| --- | --- | --- |
| connected / enabled | `●` | success |
| in use | `◉` | accent |
| offline / disabled | `○` | muted |
| succeeded | `✓` | success |
| failed | `✗` | error |
| denied / cancelled | `⊘` | warning |
| added / removed / changed | `+` / `−` / `~` | success / error / warning |

Environment is a short word coloured by risk: `prod` error, `staging` warning, `dev`
info, `local` muted.

### Keys

Letters stay the screen's shortcuts, printed in the buttons. `/` focuses the search on
every screen. Esc clears the search, then the filters, then goes back (unchanged rule).
Alt+1 and Alt+2 still go to the list and the detail. Every button is also a palette
command where the action already is one.

## Connections

Dominant loop: find a connection, look at it, connect or open SQL on it, sometimes edit,
test or add one.

```
 Workbench [Connections] ...                                     Default  pg-dev  —
 / search_____  [o Connected only]  Env: all              [u From URL]  [n New]
┌▸ Connections (4) ───────────────────────────────┐┌  pg-dev ◉ in use ─────────────────────────┐
│    NAME       DRIVER      ENV    ADDRESS         ││ [⏎ Open SQL] [b Browse] [c Disconnect]    │
│ ◉  pg-dev     PostgreSQL  dev    127.0.0.1/qa0   ││ [e Edit] [d Duplicate] [t Test] [y Copy URL] [x Delete]
│ ○  pg-prod    PostgreSQL  prod   127.0.0.1/qa0   ││                                            │
│ ○  my-dev     MySQL       dev    127.0.0.1/dexo  ││ Driver    PostgreSQL                       │
│ ○  shop       SQLite      local  ~/shop.db       ││ Address   127.0.0.1:55601                  │
│ ▾ team-a (2)                                     ││ Database  qa0                              │
│ ○  orders     PostgreSQL  staging db.int/orders  ││ User      dexo                             │
│                                                  ││ Password  from `cat …/pw`                  │
│ Found in Docker                                  ││ ── Session                                 │
│ +  shop-pg    postgres    127.0.0.1:5433         ││ Transaction  none open                     │
└──────────────────────────────────────────────────┘│ ── Last test                               │
                                                    │ ✓ reachable in 41 ms                       │
                                                    └────────────────────────────────────────────┘
```

- **List** is a table: status glyph, name, driver, environment, and address (host[:port]
  and database, or the file). Rows are grouped under their group (`▾ team-a (2)`,
  collapsible with Left/Right or a click). Ungrouped rows come first. Docker databases not
  yet saved follow under "Found in Docker", marked `+`.
- **Toolbar**: search (name, host, database, group, driver); `o` connected only; `Env`
  cycles all / prod / staging / dev / local. Buttons: `[u From URL]` and `[n New]`.
- **From URL** asks for one URL in a one-line prompt, parses it with
  `dexo_app::connection_url::parse`, and opens the form filled in, password included. A
  URL that does not parse keeps the prompt open with the parser's message.
- **Action bar** for a saved connection, depending on state:
  - offline: `[⏎ Connect] [s New SQL] [e Edit] [d Duplicate] [t Test] [y Copy URL] [x Delete]`
  - connected: `[⏎ Use] [s New SQL] [b Browse] [c Disconnect] [e Edit] ...`
  - in use: `[⏎ Open SQL] [b Browse] [c Disconnect] [e Edit] ...`
  - for a Docker row: `[⏎ Add as connection]`.

  What the new ones do:
  - `s New SQL` / `⏎ Open SQL` opens a new document on that connection on the workbench,
    connecting first if needed.
  - `b Browse` goes to the workbench with the explorer on that connection.
  - `y Copy URL` copies the connection's URL without its password.
- **Detail fields**:
  - Driver, Address, Database, User, Password (keychain / from command / none), Group.
  - `── Network` (TLS, SSH, Proxy) and `── Rules` (read-only, confirm destructive,
    verified TLS, max rows, timeout), each only when set.
  - `── Session` when connected (transaction state).
  - `── Last test`, which shows `⠋ testing (1.2s)` live, then `✓ reachable in 41 ms` or
    `✗ <error>`. It stays until another connection is picked.
- **Form**: the redesigned Add/Edit form stays. It gains a `URL` field at the top that
  fills the others when a URL is pasted into it.
- **Empty**: "No connections yet." with `[n New] [u From URL]`, and the Docker list when
  it finds databases.

## History

Dominant loop: find a statement run before, on any connection, and open it or run it
again.

```
 1 History  2 Saved   / search______  Connection: all  Status: all          [C Clear…]
┌▸ Statements (128 · 3 shown) ───────────────────┐┌  pg-dev · 15:42 · ✗ failed ──────────────────┐
│ Today                                           ││ [⏎ Open] [r Run again] [y Copy] [s Save…] [x Delete]
│ ✗ 15:42 pg-dev   2 ms        select * from nope ││                                              │
│ ✓ 15:42 pg-dev  11 ms  1 row select count(*) fr ││ select * from nope                           │
│ ✓ 15:41 my-dev   4 ms  1 row select now()       ││                                              │
│ Yesterday                                       ││ ── Run                                       │
│ ✓ 18:03 shop     1 ms  3 rows select * from t   ││ Connection  pg-dev (qa0)                     │
│                                                 ││ When        2026-10-03 15:42:10              │
│                                                 ││ Took        2 ms                             │
│                                                 ││ Result      relation "nope" does not exist   │
│                                                 ││ Runs        3, the last shown                │
└─────────────────────────────────────────────────┘└──────────────────────────────────────────────┘
```

- **Global**: every connection's history is read once when the screen opens and filtered
  in memory. It is no longer tied to the connection in use, and needs no connection open.
  The default filter is `Connection: all`. `c` cycles the connections present, the one in
  use first.
- **Recorded**: successes and failures, with status (ok / failed / cancelled), duration,
  rows returned or affected, the error message, and the database.
- **One entry per statement**: today a run records the whole document as one entry.
  Each statement that ran becomes its own entry with its own status, so a script of three
  statements whose second failed records ok, failed, and nothing for the third.
- **List**: grouped under day headings (Today, Yesterday, weekday and date). Each row
  shows the status glyph, time, connection, duration, rows (dropped at narrow widths) and
  the statement on one line. The same statement on the same connection shows once (its
  last run), with `Runs N` in the detail.
- **Filters**: search (smart-case substring of the SQL), `Connection`, and `Status` (all /
  ok / failed). The title shows `N shown`.
- **Detail**: the statement with syntax colours, then `── Run` (Connection and database,
  When, Took, Rows, Result or error, Runs).
- **Actions**:
  - `⏎ Open`: a new document on that connection.
  - `r Run again`: opens it and runs it.
  - `y Copy`.
  - `s Save…`: as a saved query, asking for a name.
  - `x Delete`: this entry and its earlier runs.
  - `C Clear…` (toolbar): clears what the filters show, after a confirmation naming how
    many go.
- **Saved** view: the same toolbar (search, `Connection`); rows show the name and
  connection. The detail is the SQL plus `Connection` and `Saved`. Actions are `[⏎ Open]
  [r Run] [y Copy] [F2 Rename] [x Delete]`.
- **Empty**: "Nothing has run yet." vs "Nothing matches the filters. [Esc Clear filters]".

## Server

Dominant loop: see what is running on a server, find the slow or blocking one, cancel or
end it.

```
 1 Sessions  2 Locks  3 Sizes  4 Stats  5 Settings   Server: pg-dev  ● every 2s   [p Pause] [r Refresh]
 / search______  [a Active only]                                       8 sessions · 3 shown
┌▸ Sessions ─────────────────────────────────────────────────────────────────────────────┐
│    PID    USER   DATABASE  STATE                TIME ▼  QUERY                            │
│ ⊘  4121   app    orders    active, blocked      12.4s  update orders set …               │
│ ●  4117   app    orders    idle in transaction   9.0s  begin; update …                   │
│ ●  50     dexo   qa0       active  · you         0.1s  select pid, …                     │
└───────────────────────────────────────────────────────────────────────────────────────┘
┌  Session 4121 ──────────────────────────────────────────────────────────────────────────┐
│ [k Cancel query] [t Terminate…] [y Copy query] [o Open in editor]                       │
│ update orders set status = 'x' where id = 1                                             │
│ ── Session        User app · Database orders · State active · Running 12.4s             │
│ ── Blocked by     4117 (idle in transaction) · lock RowExclusiveLock on orders          │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Views** over the driver's `AdministrationProvider`, which today serves only sessions:
  - Sessions.
  - Locks (`list_locks`).
  - Sizes (`sizes`, largest first).
  - Stats (`statistics`).
  - Settings (`variables`, searchable).

  A driver that refuses one says so in its view.
- **Toolbar**:
  - Server picker (`c` cycles the open connections).
  - Refresh state (`● every 2s` / `⏸ paused`).
  - `[p Pause]` and `[r Refresh]`.
  - Search, and `a` active only. Idle sessions are hidden by default, as in lazydb and
    pg_activity.
- **Sessions table**: glyph (⊘ blocked, ● other), PID, user, database, state, time
  (sorted longest first; `s` cycles the sort column, shown `▼`), and query. The session
  Dexo itself uses is labelled `you` and cannot be cancelled or ended.
- **Actions**:
  - `k Cancel query`: `AdminAction::CancelQuery`, confirmed with Cancel/Confirm.
  - `t Terminate…`: the existing typed confirmation.
  - `y Copy query`.
  - `o Open in editor`: a new document on that server's connection.
- **Detail**: the query with syntax colours, then Session fields and the blocking chain.

## Compare

Dominant loop: pick two sources, see what differs, take the script.

```
 From: pg-dev ▾   ⇄   To: pg-prod ▾     [s Swap] [⏎ Compare]        +3 added  −1 removed  ~4 changed
 / search______
┌▸ Differences (8) ────────────────────────────┐┌  ~ public.orders (changed) ─────────────────────┐
│ Tables                                        ││ [⏎ Open script] [w Whole script] [y Copy]       │
│ +  public.invoices                            ││ ALTER TABLE public.orders ADD COLUMN note text; │
│ ~  public.orders                              ││ ── Risk   none                                  │
│ −  public.legacy          destructive         ││                                                 │
│ Indexes                                       ││                                                 │
│ +  orders_note_idx                            ││                                                 │
└───────────────────────────────────────────────┘└─────────────────────────────────────────────────┘
```

- **Sources stay visible** in the toolbar after comparing.
  - From and To are pickers (Left/Right or a click). File needs a path field, which shows
    only when a side is a file.
  - `[s Swap]` exchanges the sides (`x`, delete elsewhere, is not used here);
    `[⏎ Compare]` (or `e`) compares again.
  - The first visit shows the same row with nothing compared yet: "Pick two sources and
    Compare."
- **Summary chips** `+3 added  −1 removed  ~4 changed` are the filters (a, r, c, or a
  click). A hidden kind is dimmed.
- **List** grouped by object kind; glyph `+ − ~` with colour; the risk word when there is
  one.
- **Detail**: the statement for the picked one in SQL colours, or the whole script (`w`).
  Actions are `[⏎ Open script] [w Whole script] [y Copy]`.
- **Empty**: "✓ The schemas match." when nothing differs.

## Agents

### Setup (rewritten for brevity)

```
┌▸ Agents ───────────────────────┐┌  Claude Code ──────────────────────────────────────┐
│ ✓  Claude Code   assistant      ││ [⏎ Set up] [y Copy command]                         │
│ ·  Codex         found          ││                                                     │
│ ·  Cursor        not found      ││ Status    not set up · installed                    │
│ ·  Claude Desktop not found     ││ Writes    .mcp.json  (this folder)                  │
│ ·  Gemini CLI    not found      ││ Skill     .claude/skills/dexo/SKILL.md              │
│ ·  Windsurf      not found      ││                                                     │
│ ·  VS Code       found          ││ Profile   < assistant >  uses db2, db3 · read SQL   │
│                                 ││ Skill     [x] write it                              │
└─────────────────────────────────┘└─────────────────────────────────────────────────────┘
```

- **List** shows the status (`✓ set up · <profile>`, `· found`, `· not found`,
  `! unreadable`). Found means the client's command is on PATH (`claude`, `codex`,
  `gemini`, `code`, `cursor`, `windsurf`) or its config folder exists (Claude Desktop).
  Not-found clients stay in place, dimmed.
- **Detail**:
  - Fields: Status, Writes (path relative to the home or the folder, with "exists" or
    "new file"), Skill.
  - The form follows the fields: Profile picker; for a new profile, Name, the Connections
    checklist and Read SQL; then the Skill toggle.
  - No sentences. The status line holds the row's hint.
- **Actions**: `[⏎ Set up]` and `[y Copy command]`. The command is no longer printed.
- **Outcome**: a short checklist replaces the prose:
  `✓ .mcp.json written (old kept as .mcp.json.dexo-backup)`, `✓ assistant enabled`,
  `→ restart Claude Code`.

### Profiles

- **List**: `● name   N conn   read SQL|browse   G grants`.
- **Detail fields**: Connections, Access, Scopes, Tools, then `── Grants` (each with what,
  on what, time left).
- **Actions**:
  - `e Enable/Disable`.
  - `c Connections…`: a checklist in the detail pane, saved with Enter. New in the TUI.
  - `q Read SQL on/off`: new.
  - `g Grant…` (existing form), `r Revoke grants`, `x Delete`.
  - Toolbar: `[n New]`, which opens Setup's new-profile form.

### Approvals and Activity

- **Approvals**: the detail starts with `[a Approve] [d Deny]`. Fields are profile, tool,
  connection, target, waiting since and time left, then the SQL in colours. The toolbar
  holds `[R Revoke all grants]`.
- **Activity**: a table of time, profile, tool, target, outcome (`✓ ✗ ⊘`), ms and rows.
  Toolbar filters are `Profile`, `Outcome` and search. The detail shows the call's
  fields.

## Data changes

- **History** (`dexo-storage`): one migration adds `outcome TEXT`, `duration_ms INTEGER`,
  `row_count INTEGER`, `error TEXT` and `database TEXT` to `sql_history`. Old rows read as
  outcome ok with the rest empty.
  - `HistoryRow` gains `id` and those fields.
  - `HistoryRepository` gains `delete(id)`, `delete_matching(...)` for Clear, and
    `entries_all(limit)`.
  - The TUI records an entry per statement at its end. On a failure it records the error;
    on a cancel the outcome is cancelled.
  - The storage worker's `ListHistory` takes no connection.
- **MCP profiles**: saving connections and the query mode reuses `McpProfileRepository`
  (as Setup does), off the loop.
- **Server views**: new effects call `list_locks`, `sizes`, `statistics` and `variables`
  on the side connection the sessions already use, and `execute_action(CancelQuery)`.
- **Client detection**: `dexo_app::mcp::clients::McpClient::installed(&Places)` returns
  found / not found from PATH or the config folder.

## Floors

- **Wide (≥ 120 columns)**: list and detail side by side (today's 42/58 split). The
  toolbar holds everything.
- **Standard (80–119)**: list above detail (today's behaviour), list capped at half the
  height. The action bar is the detail's first row, so it is always visible. The toolbar
  drops screen buttons into the status line, then collapses its chips.
- **Narrow (60–79)**:
  - One column: the list has the screen until Enter or Alt+2, then the detail has it.
    Esc goes back to the list.
  - Tables keep the glyph, name and one more column.
- **Below 60 columns**, the same one-column flow with the list's name column only. Below
  20x8, the existing too-small message.

## Testing

- **Unit**: each screen's filter, sort, grouping and button state (enabled/disabled per
  item state), and the history dedupe and the "N shown" count.
- **Rendered frames** at 140x40, 80x24 and 60x20 for each screen in its main states:
  empty, loaded, filtered-empty, and with a form or question open. These are snapshot
  tests where they are stable.
- **Key and mouse flows**: a button click and its key do the same thing. Detail focus
  with Left/Right/Enter presses buttons. `/` then Esc clears.
- **Storage**: the migration on an old database, recording success, failure and cancel,
  and delete and clear.
- **QA harness** pass on every screen at the three sizes, against the QA Postgres and
  MySQL containers.

## Order of work

1. Shared components: toolbar, action bar with keyboard focus, fields and sections, SQL
   view, empty states.
2. History: migration, recording, global loading, filters, actions.
3. Connections: table, groups, filters, action bar, From URL, live test line.
4. Agents: Setup brevity and detection, Profiles editing, Approvals and Activity bars and
   filters.
5. Server: views, filters, sort, cancel query, `you`.
6. Compare: sources toolbar, swap, grouped list, summary chips.
7. Docs (`docs/src/workbench.md`, `mcp.md`), README screenshots, QA at the three sizes.

Each step is its own commits, one per change, in the 1.4.2 release.
