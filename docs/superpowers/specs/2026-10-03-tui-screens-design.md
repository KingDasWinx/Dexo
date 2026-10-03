# Screens: places beyond the workbench

Date: 2026-10-03. Status: proposal, not started.

## The idea in one paragraph

Dexo has one screen, the workbench (sidebar, editor, results), and 41 kinds of overlay drawn over it.
Some overlays are short questions and should stay overlays. Others are places where a person works for
minutes: they are squeezed into a centred box while most of the terminal stays empty. Agent Activity is a
100x7 box on a 160x45 terminal; Inspect Sessions cuts every query at 40 characters on the same terminal.
This proposal adds a small, fixed set of full screens next to the workbench. A screen is a place: it has
the whole terminal below the header, keeps its state while Dexo runs, and is reached by the palette, a
`Ctrl+G` jump, a click on the screen strip, or the shortcut that opened the old dialog. Esc leaves it.

Recommended set: **Workbench** (home, as today), **Connections**, **Agents**, **Server**, **Compare**,
**History**. Object details (inspector, DDL, grants) do not become a screen: they become views of the
table's own document, where the other database TUIs put them.

## What is there today (inventory)

All 41 `OverlayKind` variants (`crates/dexo-tui/src/mouse.rs:99`), drawn by `draw_workbench` in
`render.rs` with `centered(area, w, h)`. Verdict per overlay:

| Overlay | Size today | Verdict | Why |
|---|---|---|---|
| McpAudit (Agent activity) | up to 100 wide, height of its text | **Screen: Agents** | A queue to work through and a log to read; live; needs the SQL of each request in full. |
| McpProfiles + grant form | 84 / 92 wide | **Screen: Agents** | Profiles, their scopes and grants are a list + detail; the form fits as a side pane. |
| Admin (Inspect Sessions) | ~110 wide, height of the rows | **Screen: Server** | A live monitor; every tool that has one (pg_activity, pgcenter, lazydb) makes it full screen. |
| Connections (Browse) + Docker list | 72 wide | **Screen: Connections** | A managed list with groups, details, test, Docker discovery. |
| ConnectionForm (Add/Edit) | 72x22, scrolls 25 fields | **Screen: Connections** (side pane) | The form is long and folds half its fields; next to the list it has the room it needs. |
| DeleteConnection | sized to text | stays a confirm | A question. |
| SchemaDiff (Compare Schema) | 88x24 | **Screen: Compare** | Setup + list of differences + DDL per difference + migration script: four things in one box today. |
| History (editor) | 100x18 | **Screen: History** | One line per statement, cut; no time, connection, duration or preview. |
| SavedQueries | 100x22 | **Screen: History** (second tab) | Same shape as history: search, list, preview, open. |
| ObjectOverlay (Inspect, DDL, notes) | 84x24 | **Table document view** | Lives with the object: lazysql, gobang, dblab and vi-mongo show it as sub-tabs of the table. |
| Security (Manage Grants) | 14 rows | **Table document view** (Privileges) | Per object, so with the object. |
| SchemaForm (Create table) + DdlPreview | 76x20 / 72x20 | stays (later: a table designer document) | A bounded form today; a real designer is its own project. |
| Transfer (Import/Export/Backup/Restore) | 72x16 | stays a dialog for the setup; progress goes to the status bar | Bounded input. A Jobs screen only if long jobs show they need it (not proposed now). |
| ConfigTransfer, Diagnostics, Recovery | 64-72 wide | stay | Rare, short, summon-and-leave. |
| Settings | 64x13 | stays | Its theme preview is the real workbench behind it; no reference TUI has a settings screen. |
| Help (F1) | 76 x full height | stays; opens at the current screen's section | Every reference keeps help as an overlay or a page you leave at once. |
| Projects | 72 wide | stays | Summon, choose, leave. |
| Palette, ResultsMenu, NodeMenu, Completion, Snippets, Related | — | stay | Pickers and menus. |
| ClosePrompt, RunPrompt, ProductionPrompt, ExplainPrompt, QuitPrompt, TransactionPrompt, SecretPrompt, DocumentNamePrompt, Parameters, TryIndex, SaveQuery | sized to text | stay | Questions and one-line inputs. |
| Review, InsertRow, CellEdit, ValueViewer | 60-80 wide | stay | Bounded edits and a detail-on-Enter view of one value. |
| Onboarding (Welcome) | 72 wide | stays | First run only. |
| FilePicker | large | stays | A picker. |

Rule used: **an overlay asks, a screen is where you work.** It stays an overlay when it is a question,
a one-shot input, a picker, or a detail of the thing under the cursor. It becomes a screen when it is a
list you work through, it updates by itself, it needs a detail pane next to the list, or it has more than
one view.

## What the reference TUIs do

Studied: gh-dash, superfile, yazi, posting, harlequin, and the database TUIs rainfrog, lazysql, sqlit,
pg_activity, pgcenter, visidata, gobang, lazydb, dblab, tabiew, vi-mongo, pspg.

- **One fixed frame, the middle swaps.** gh-dash, yazi, pgcenter and pg_activity keep the header and the
  footer and change only the content. None replaces the whole frame for a feature.
- **Peer places, not a deep stack.** gh-dash cycles views with one key, yazi uses 1-9 and a strip,
  pg_activity F1-F3, pgcenter single letters, lazysql switches pages. Only visidata has a real back stack.
- **State is kept.** yazi keeps every tab's cwd, selection and history; pgcenter saves sort, filters and
  widths per view and restores them; lazysql reuses a connection's page. gh-dash resets the selected
  section on switch, and that is the one to avoid.
- **Where you are is always shown**: a tab strip (most), a title line (pg_activity), the stack in the
  status bar (visidata). pgcenter's two-second message is the weakest.
- **Concept by concept**: sessions/activity are always full screen (pg_activity, pgcenter, lazydb's
  Dashboard tab); the connection manager is a full page in lazysql, gobang, vi-mongo and dblab; object
  structure is sub-tabs of the table in lazysql, gobang, dblab, vi-mongo; help, export, theme and value
  views are overlays nearly everywhere; history is a modal in all of them except rainfrog (a workbench tab)
  and harlequin, whose History modal fills the viewport. No reference has agent activity, MCP grants or
  schema compare at all: Dexo has nothing to copy there and the most room to stand out.
- **The footer follows the place**: gh-dash builds help from the view's keymap, yazi from the layer's,
  sqlit clears the footer under a modal.

## The screens

### Header: the screen strip

Row 1 today reads `Default  pg-dev  —`. It becomes the strip on the left and the context on the right:

```
 Workbench  Connections  Agents 2  Server  Compare  History          Default · pg-dev · public
```

The current screen is drawn in the accent, reversed, and in brackets when there is no colour
(`[Agents]`). A number after a name is work waiting there (agent requests waiting for approval). Below 100
columns the strip keeps the current screen and any screen with work waiting (`[Server]  Agents 2`);
below 60 columns, only the current screen.

### Agents (MCP profiles, grants, activity)

Three views in one screen, switched with `[`/`]` or `1`-`3` (no text input on this screen):

```
 Workbench  Connections  [Agents 2]  Server  Compare  History        Default · pg-dev · public
 Approvals 2   Activity   Profiles
┌Waiting for you────────────────────────────┐┌assistant wants to change pg-dev─────────────────┐
│> 12:41  assistant  data_update   pg-dev    ││UPDATE public.orders                              │
│  12:40  reporter   ddl_apply     pg-prod   ││   SET status = 'paid'                            │
│                                            ││ WHERE id = 1042;                                 │
│                                            ││                                                  │
│                                            ││grant data_write on db.public.orders, 1 write left│
│                                            ││asked 12:41, gives up in 4m 10s                   │
└────────────────────────────────────────────┘└──────────────────────────────────────────────────┘
 a approve  d deny  Enter details  ] next view  Esc back                              Ctrl+P  F1
```

- **Approvals**: the waiting requests and, for the one picked, the full statement, the grant it would
  use, the time it waits. Approve on production still asks for the connection's name.
- **Activity**: the audit log, newest first, with time, profile, tool, target, outcome; `/` filters;
  Enter shows one call in full.
- **Profiles**: profiles on the left; on the right the selected one's connections, scopes, tools and
  grants; `g` opens the grant form in that right pane, not as a dialog; `e` enable, `r` revoke.
- A request that starts waiting while you are elsewhere puts its count on the strip and one toast
  ("assistant waits for approval: Ctrl+G a"). Nothing takes the focus away from what you are typing.

### Server (sessions)

Per connection, defaulting to the active one; `c` picks another.

```
 Workbench  Connections  Agents  [Server]  Compare  History              pg-dev · PostgreSQL 16
 Sessions 14   Blocking 1                                        every 2s · p pause · r now
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│  PID    USER  DATABASE  STATE                WAIT        TIME  QUERY                          │
│> 2193   app   shop      active               Lock       12.0s  UPDATE orders SET status = …   │
│  3700   app   shop      idle in transaction  ClientRead  4.1s  SELECT * FROM orders WHERE …   │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
┌Session 2193──────────────────────────────────────────────────────────────────────────────────┐
│ blocked by 3700 (idle in transaction for 4.1s), waiting for a row lock on public.orders      │
│ UPDATE orders SET status = 'paid' WHERE id = 1042                                            │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
 t terminate  x cancel query  / filter  s sort  Enter full query  Esc back            Ctrl+P  F1
```

Refreshes on its own while it is on screen, pauses with `p`, stops polling when you leave. Terminate
keeps today's guards (read-only refuses, production asks for the name).

### Connections

```
 Workbench  [Connections]  Agents  Server  Compare  History                       Project Default
┌Connections────────────────┐┌pg-prod───────────────────────────────────────────────────────────┐
│ production                ││ PostgreSQL 16   10.0.0.5:5432   shop   as app                      │
│>  ● pg-prod               ││ production: confirms writes, needs verified TLS                    │
│ development               ││ TLS verify_full   SSH bastion:22   password from the keychain      │
│   ○ pg-dev                ││ last connected today 10:42                                         │
│   ○ mysql-dev             ││                                                                    │
│ Running in Docker         ││                                                                    │
│   + qa-pg  :55601         ││                                                                    │
└───────────────────────────┘└────────────────────────────────────────────────────────────────────┘
 Enter connect  n new  e edit  t test  u duplicate  m group  Delete delete  Esc back   Ctrl+P  F1
```

`n` and `e` turn the right pane into the form (all fields visible at once on a tall terminal, the
advanced ones still folded below 30 rows); Esc in the form goes back to the details, not out of the
screen. Groups finally show (they are saved today and never drawn). Delete stays a confirm. The sidebar
keeps its quick `n`/`e`, which open this screen with the form ready.

### Compare (schema diff)

Setup in the top row (`From ‹ pg-dev ›  To ‹ snapshot v2 ›  [Compare]`), then the differences on the left
with their filters and risk, and on the right the DDL of the picked difference; `s` shows the whole
migration script, Enter opens it in a new document on the From connection and goes to the workbench.

### History (history and saved queries)

Two views: **History** (every statement run, by connection, with time, duration, rows and outcome) and
**Saved**. The left list searches as you type; the right pane shows the statement in full with syntax
colours. Enter opens it in a new document and goes back to the workbench, so the quick recall that the
dialog serves today still takes one key and Enter. `F2` rename and Delete stay for saved queries.

### Object views in the table document (not a screen)

A table's document gets views next to its grid: **Data**, **Structure** (columns, keys, indexes,
foreign keys, size, owner, comments), **DDL**, **Privileges** (today's Manage Grants). Inspect (`i`)
opens the object's document on Structure. Views and functions get a document with Structure and DDL.
This keeps "every document belongs to a connection" true and frees two overlays.

## How screens work

| Question | Answer |
|---|---|
| Open | Palette ("Go to Agents", category Screen); `Ctrl+G` then a letter (`w` workbench, `c` connections, `a` agents, `s` server, `d` compare, `h` history), with a small which-key list after `Ctrl+G`; a click on the strip; and every shortcut that opened the old dialog now opens its screen (`s` in the sidebar → Server on that connection, `Ctrl+Alt+A` → Agents › Approvals, `h` → History). |
| Leave | `Esc` peels one layer at a time: clear the search, close the side form, then go back to the screen you came from (usually the workbench). `Ctrl+G Ctrl+G` jumps back and forth between the last two screens. |
| Close | Screens are not closed. Leaving keeps their state; `Ctrl+W` stays a document key. |
| State | Each screen's state lives in the model for the session (selection, filters, scroll, view), as the dialogs' state already does. Its data refreshes when you enter, and Agents and Server keep refreshing while on screen. Dexo always starts on the workbench. |
| Where am I | The strip in row 1; the status bar's left word names the screen; the footer hints are the screen's. |
| Overlays | Any overlay opens over any screen and belongs to it. While one is open, `Ctrl+G` does nothing (the overlay has the keys), as today. |
| Help | F1 opens at the current screen's section; the palette lists the screen's commands first. |
| Keymap | New key contexts `connections`, `agents`, `server`, `compare`, `history`, overridable in the user's keymap like the others. |
| Mouse | The strip switches screens; panes, rows and buttons take clicks as in the workbench. |
| Too small | Below 100 columns a list + detail screen stacks the detail under the list; below 80x24 (compact) it shows the list only and Enter opens the detail full height, Esc goes back. Below 40x12 every screen shows "Terminal too small: Dexo needs 40x12" (there is no such guard today). |

## Clutter audit (what this removes)

- Border depth: an overlay today sits inside the workbench's pane borders, so its content is two borders
  deep and crosses the panes' edges. A screen's panes are one border deep.
- Empty space: Agent Activity uses 7% of a 160x45 terminal, Saved Queries and Projects less. The
  screens give the list the width and the detail the rest.
- Truncation: Sessions' QUERY column and History's one-line statements are cut by the box, not by the
  terminal. The detail panes show them whole.
- Signals: the strip shows the place once (accent + brackets for no colour); the status bar stops
  repeating it.

## How it is built (crates/dexo-tui)

- `model.screen: Screen` (`Workbench`, `Connections`, `Agents`, `Server`, `Compare`, `History`) and
  `model.previous_screen`. The dialogs' state structs in `screens/` (`mcp_audit`, `mcp_profiles`, `admin`,
  `connections`, `connection`, `schema_diff`, `saved_queries`, editor history) are reused: their `open`
  flag goes away and the screen owns them.
- `render`: `draw_workbench` becomes one arm of `match model.screen`; each screen gets a `draw_*` with
  its own layout from the full area; the header strip replaces `context_line`; overlays are drawn after,
  as now.
- `update`: key routing stays top overlay first, then the screen's handler, then global keys. New
  actions `GoToScreen(Screen)` and `ScreenBack`. `top_overlay` loses the seven variants that became
  screens.
- `keymap`: the new contexts, `ctrl+g` chords in the three profiles (the chord machinery exists for
  Emacs), help sections per context.
- `palette`: a Screen category; screen commands filtered by the current screen, as posting does.
- `HitMap`: the strip and each screen's panes.
- Everything goes through `dexo-app` as now; no new dependency.

## Order of work

1. **Frame**: `Screen`, the strip, `Ctrl+G` with its which-key list, Esc back, per-screen status and
   help, the too-small guard. Snapshots at 160x45, 120x36, 80x24, 60x20, 40x12.
2. **Agents**: Approvals, Activity, Profiles with the grant form in place. (The one that bothers most.)
3. **Server**: sessions, blocking, live refresh.
4. **Connections**: list with groups and Docker, details, form in place.
5. **Compare** and **History**.
6. **Object views** in the table document; Inspect and Manage Grants move there.
7. Later, if wanted: a table designer document for Create/Alter Table; a Jobs screen if long imports and
   restores show they need one.

Each step ships the old dialog's shortcut opening the new screen, its tests moved from the dialog to the
screen, and the docs page (`docs/src`) updated.

## Testing

State tests for switching, back, state kept on return, keys ignored under an overlay, live refresh
starting and stopping. Snapshots per screen at the five sizes and in no-colour and ASCII. The palette and
keymap count tests. A QA pass through the harness (`qa.sh`) on every screen before release, as the
1.4.2 user test did.

## Decisions to make

1. Screen keys: `Ctrl+G` + letter (proposed; free in all three keymaps, though Emacs users know `C-g` as
   cancel) or F-keys (F3, F4, F6, F9 are free but say nothing about what they open).
2. Esc on a screen goes back (proposed) or only clears, with `Ctrl+G w` the way home.
3. History as a screen (proposed) or a larger dialog with a preview, as harlequin does.
4. Release: ship 1.4.2 as it is and build screens for 1.5.0 (proposed: the frame changes every screen's
   tests and deserves its own QA round), or hold 1.4.2 for them.
