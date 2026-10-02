# TUI user test, 2026-10-02 (before 1.4.2)

Every command of the palette (161) was used as a user would — from the palette and by its hotkey, keyboard and mouse — and every modal it opens was walked through: fields, buttons, Esc, clicks, small terminals. Nothing here is fixed yet; this file collects the findings to fix later.

**Build:** `development` at the time of the test, with the `duckdb` feature.
**Connections:** Postgres 16.9 (development, production, read-only), MySQL 8.4.5, SQLite (demo shop), DuckDB over a CSV.
**Method:** a tmux session per tester, keys and SGR mouse events sent to Dexo, the screen read back.

**Severity:** BLOCKER = crash, data loss, safety guard bypassed, or a feature that cannot be used at all. MAJOR = works wrongly, or breaks a TUI standard users will hit. MINOR = works but awkwardly. COSMETIC = looks wrong.

**Standards checked:** dialogs walk with the arrows and Esc cancels; hotkeys shown and working; focus and selection visible; single-line inputs edit alike (Ctrl+A, word keys); documents bound to connections; nothing overlaps or is cut off, down to 80x24; clear messages without debug text; safety guards; no crash or freeze.

## Summary

_Filled in once every area is in._

## Findings by area

### MCP and agents (mcp-agents)



Binary: development build of 1.4.2. Data home `homes/mcp-agents`. Profiles used: `pg-dev` (raw-read, allow `qa7.public.*`, deny `qa7.public.customers`, `--allow-tool data_execute_sql`), `pg-prod` (same, on the production connection), and a stray `bad name!`.

##### [MINOR] MCP Profiles with no profile: empty state gives no way forward
- **Where:** mcp.profiles (palette "MCP Profiles")
- **Steps:** fresh data home, Ctrl+P, "MCP Prof", Enter
- **Expected:** an empty state that says how to create a profile (there is no create action in the TUI), and the key hints (7, 6).
- **Actual:** the popup says only `no MCP profiles`, no hint line at all (the `e enable/disable  g new grant ...` line is only drawn once a profile exists), and the status bar under it still shows the workbench hints. A new user does not learn that profiles are made with `dexo mcp profile create`.

##### [MAJOR] `g` (New MCP Grant) in the empty MCP Profiles screen opens a form that cannot succeed
- **Where:** mcp.grant, MCP Profiles with no profile
- **Steps:** palette > MCP Profiles (empty), press `g`
- **Expected:** a one-line refusal ("create a profile first") instead of a form (6, 7).
- **Actual:** the full "New MCP grant" form opens with `For MCP profile` blank, the first focus is on `tools:` (not `connection:`), Enter reports `no MCP profile is selected` at the bottom of the form (the form grows by one line and the buttons move).

##### [MINOR] Palette shows hotkey `g` for "New MCP Grant…" but it only works inside MCP Profiles; help does not list it
- **Where:** mcp.grant
- **Steps:** Ctrl+P "mcp": row shows `g`. Close the palette. In the SQL editor press `g`; in the explorer press `g`; F1 and search "grant".
- **Expected:** (2) the hotkey listed in the palette does the action where it is meant to work, and F1 lists it with its context.
- **Actual:** in the editor `g` is typed into the document (starts query-1.sql* and opens completion); in the explorer nothing happens; F1 search for `grant` says `no matches for 'grant'` (only `ctrl+alt+a Agent Activity` is listed under [Workbench]; New MCP Grant, Revoke All and MCP Profiles have no help entry).

##### [MINOR] Agent Activity `r revoke all grants` closes Activity and opens the MCP Profiles screen with an unexplained pending "confirm revoke all grants"
- **Where:** mcp.audit / mcp.revoke_all
- **Steps:** Ctrl+Alt+A (empty), press `r`
- **Expected:** a confirmation in the same screen with the keys to confirm or cancel (1, 7).
- **Actual:** the Agent Activity popup is replaced by the "MCP profiles" popup, which reads `no MCP profiles` and `confirm revoke all grants` (what to press is not said; a second `r` says `no MCP profile selected`, Enter then says `revoked 0 grants`). The message `confirm revoke all grants` also stays as stale text and is shown again the next time MCP Profiles is opened.

##### [MAJOR] MCP Profiles shows raw debug lines (`profile NAME enabled=false`, `mcp profile=... enabled=false confirm=true`)
- **Where:** mcp.profiles with profiles present
- **Steps:** create profiles from the CLI, palette > MCP Profiles
- **Expected:** a readable list (name, enabled/disabled, connections) and a status line that says what is selected (7: no `foo=bar` dumps).
- **Actual:**
```
│> profile bad name! enabled=false
│  profile pg-dev enabled=false
│  profile pg-prod enabled=false
│mcp profile=bad name! enabled=false confirm=false
│confirm revoke all grants
│e enable/disable  g new grant  r revoke  R revoke all  esc close
```
Pressing `e` shows `confirm enable bad name!`, the second `e` shows `enabled bad name! scopes=0 tools=0`; the status line keeps `confirm=true/false` internals.

##### [COSMETIC] MCP Profiles selected row has no highlight
- **Where:** mcp.profiles
- **Steps:** open the screen, `ansi`
- **Expected:** the selected row visibly highlighted (3).
- **Actual:** only the `>` marker; no reverse video or colour on the row.

##### [MINOR] Inconsistent revoke-all keys between screens
- **Where:** MCP Profiles vs Agent Activity
- **Steps:** compare the hint lines
- **Expected:** one key for one action (2).
- **Actual:** Profiles: `r revoke  R revoke all`; Activity: `r revoke all grants`.

##### [MINOR] CLI: `dexo mcp profile create` accepts any name, including `bad name!`, and there is no way to delete a profile
- **Where:** CLI `mcp profile create`
- **Steps:** `dexo mcp profile create --name 'bad name!'`
- **Expected:** a name usable in `--profile NAME` and in client configs (letters, digits, `-`, `_`), and a way to remove a profile (also in the TUI).
- **Actual:** `created bad name! enabled=false access=read_only`. There is no `profile delete` (CLI) and no delete in the TUI, so the profile stays forever.

##### [MINOR] CLI: creating a profile with an existing name shows the raw SQLite error
- **Where:** CLI `mcp profile create`
- **Steps:** `dexo mcp profile create --name pg-dev` twice
- **Expected:** `profile 'pg-dev' already exists` (7).
- **Actual:**
```
Error: UNIQUE constraint failed: mcp_profiles.name

Caused by:
    Error code 2067: constraint failed
```

##### [MINOR] CLI: `profile show`/`policy` print Rust Debug names; empty answers are silent
- **Where:** CLI `mcp profile show`, `mcp policy`, `mcp profile list`, `mcp audit`, `mcp grant list`, `mcp doctor`
- **Steps:** `dexo mcp profile show --name pg-dev`; `dexo mcp grant list --profile nope`; `dexo mcp audit` with no events; `dexo mcp doctor` with no profile
- **Expected:** human words for query mode (7) and a message when a list is empty or the profile does not exist.
- **Actual:** `query_mode=StructuredOnly` / `RawReadSql` (enum Debug names, the flag is `raw-read`); `grant list --profile nope` prints nothing and exits 0 (an unknown profile is not reported); `profile list`, `audit` and `doctor` print nothing and exit 0 when there is nothing. `policy` prints `selector allow ...` while `allow`/`enable` print `scopes:` blocks for the same rules.

##### [MAJOR] MCP Profiles popup is fixed-height: with a few grants the status line and the hint line are cut off, so a pending confirmation is invisible
- **Where:** mcp.profiles / revoke
- **Steps:** profile `pg-dev` with scopes (allow + deny), a tool rule and 3 grants; open MCP Profiles at 120x36, select `pg-dev`, press `r`
- **Expected:** every line of the selected profile is reachable (scroll) and the status/confirmation line is always visible (6).
- **Actual:** the popup is 13 rows tall; the detail lines (`scope ...`, `tool ...`, `grant ...` + `diff ...`, two rows per grant) push the status line and the `e enable/disable  g new grant  r revoke ...` hint line out of the box, the third grant shows only its first line and its `diff` line is cut. PageDown and the wheel do nothing (the wheel moves the profile selection). `r` asked for confirmation on its first press, which could not be seen; the second `r` then revoked all 3 grants (`revoked 3 grants` was visible only after the selection moved to the first profile and the detail got shorter).
```
 15 │ tool data_execute_sql
 16 │ grant data_write data_update 1630s asks (120s)
 17 │ diff pg-dev allow qa7.public.orders
 18 │ grant data_write data_execute_sql 1630s asks (20s)
 19 │ diff pg-dev allow qa7.public.orders
 20 │ grant data_write data_insert 86400s
 21 └──────
```

##### [MINOR] MCP Profiles: key `r` is labelled "revoke" but revokes ALL grants of the selected profile (after a silent first press)
- **Where:** mcp.profiles
- **Steps:** select a profile with 3 grants, press `r` twice
- **Expected:** a label that says what it does (`r revoke grants`), a visible `press r again to confirm`, and a way to pick one grant (the CLI has `grant revoke --id`; the TUI shows no grant ids and cannot revoke one grant) (1, 7).
- **Actual:** the hint says `r revoke  R revoke all`; the first `r` sets an invisible/unlabelled pending state; the second says `revoked 3 grants` for the profile. `R revoke all` is for all profiles but is not explained either.

##### [MINOR] MCP Profiles detail uses internal words and units: `diff pg-dev allow qa7.public.orders`, `grant data_write data_update 1796s asks (120s)`
- **Where:** mcp.profiles detail lines
- **Steps:** select a profile that has grants
- **Expected:** a grant line says what it allows, on which connection and objects, and when it ends: `data_update on qa7.public.orders, pg-dev, asks before each write (120 s), expires in 29 min`.
- **Actual:** `grant data_write data_update 1796s asks (120s)` followed by `diff pg-dev allow qa7.public.orders` (the word `diff` and `allow` are internal: it is the grant's connection and selector); a grant that is not `--ask` shows `grant data_write data_insert 86400s` (seconds left, not "one use, 24h"), no grant id. `scope allow ...`/`tool data_execute_sql` (a tool allow rule) are not explained either.

##### [MINOR] MCP Profiles: status line is sticky across openings and stale
- **Where:** mcp.profiles
- **Steps:** press `e` on a profile (`disabled bad name!`), Esc, reopen the screen, move the selection
- **Expected:** the status line is cleared when the screen is reopened or the selection moves (6, 7).
- **Actual:** `disabled bad name!` / `confirm revoke all grants` / `Granted admin_cancel_query on qa7.public.orders for one wr...` are shown again the next time MCP Profiles is opened and stay while the selection moves to other profiles.

##### [MINOR] MCP Profiles: list is a snapshot, it does not follow changes made by `dexo mcp` while it is open
- **Where:** mcp.profiles
- **Steps:** open the screen; `dexo mcp profile enable --name pg-dev --confirm` in another shell; look at the screen; close and reopen
- **Expected:** a visible refresh (or a key to refresh), or at least that `e` toggles the real state.
- **Actual:** `pg-dev enabled=false` stays until the screen is reopened.

##### [MINOR] Enabling in MCP Profiles needs a second `e`, but the prompt does not say so; selection resets after actions
- **Where:** mcp.profiles
- **Steps:** select a disabled profile, `e`; `Enter`; `Down`; `Up`, `e`
- **Expected:** `Press e again to enable bad name! (Esc cancels)`; the confirmation also says what enabling means (agents may use the profile).
- **Actual:** the line says `confirm enable bad name!` plus `mcp profile=bad name! enabled=false confirm=true`; Enter does nothing; moving the selection cancels it silently; the second `e` enables (`enabled bad name! scopes=0 tools=0`) even for a profile with no connections, no scopes. Disabling needs one press (asymmetric, fine) and says only `disabled bad name!`. After creating a grant or revoking, the selection jumps back to the first profile, so the grant that was just made is not on screen.

##### [MINOR] New MCP Grant: first focus is `tools:`, not `connection:`; the connection is not prefilled even when the profile has one connection
- **Where:** mcp.grant
- **Steps:** MCP Profiles, select `pg-dev` (single connection `pg-dev`), `g`
- **Expected:** focus on the first field, connection prefilled (or chosen from the profile's connections) and capability as a choice (data_write / ddl / admin).
- **Actual:** `> tools:` has focus, `connection:` is empty free text, `capability:` is free text (`data_write`; `data_writezzz` is accepted until submit, then `unknown grant capability`, which does not list the valid values).

##### [MINOR] New MCP Grant: clicking the "ask before each write" checkbox only moves focus, it does not toggle it
- **Where:** mcp.grant form
- **Steps:** click on `[ ]` (col 45-47, row 15) or on its label; Space toggles
- **Expected:** a click on a checkbox toggles it (1).
- **Actual:** the row gets the `>` marker, `[ ]` stays `[ ]` on every click. Space works: `[x] each write waits for you in Agent Activity`. Clicking Create and Cancel works; clicking fields focuses them.

##### [MINOR] New MCP Grant: validation messages are terse, stale or misleading
- **Where:** mcp.grant form
- **Steps:** submit with wrong values
- **Expected:** a message that says what is wrong and what is accepted (7); an error that stops showing after the field is fixed.
- **Actual:** `unknown grant capability` (no valid list); expires `abc`/`1d` -> `invalid ttl`, `25h`/`0m` -> `grant ttl must be 1s..=24h` (Rust range syntax; the accepted spellings `15m`, `2h` are not shown); empty selector -> `selectors allow exact names or explicit * only`; empty connection -> `unknown connection ''`; wrong confirm -> `type the connection or the selector to confirm` (does not say it did not match); selector outside the profile or denied -> `grant scope cannot be broader than the profile` (for a denied table too). Once shown, the error stays after the field is fixed until the next submit (`name the tools the grant allows` stays while `tools:` is filled), and the form grows by one row so the buttons move down by one. Focus stays on the field you were on, not on the wrong one. The `tools:` hint line lists `data_insert data_update data_delete data_execute_sql · schema_apply_ddl` without saying which capability each belongs to (admin tools missing) and is truncated at 80 columns (`schema_apply_d`).

##### [COSMETIC] New MCP Grant: the checkbox label describes the unchecked state as a feature
- **Where:** mcp.grant form
- **Steps:** read the `ask before each write` row
- **Expected:** a clear label for both states.
- **Actual:** unchecked: `[ ] one write, then the grant is spent`; checked: `[x] each write waits for you in Agent Activity`. The second field `approval timeout (s): 120` stays editable and visible when ask is off and is silently ignored (even `12.5` is accepted then). There are two different rows starting with `confirm`: the field `confirm:` and the hint `confirm: type the connection or the selector again`.

##### [MINOR] Form allows several tools in one grant, the TUI success message names only the tool and selector
- **Where:** mcp.grant
- **Steps:** tools `data_update data_delete`, Create
- **Actual:** accepted (grant list shows `data_update,data_delete`); the message `Granted data_insert on qa7.public.orders for one write.` never says for how long it lasts (`expires 24h`) or on which connection. (A grant created with `ask` checked is shown in the CLI as `asks=120s`, valid until it expires, not "for one write".)

##### [MAJOR] New MCP Grant from the palette silently targets the first profile; the form cannot choose a profile
- **Where:** mcp.grant (palette "New MCP Grant…")
- **Steps:** with profiles `bad name!`, `pg-dev`, `pg-prod`: workbench (SQL editor), Ctrl+P, "New MCP", Enter
- **Expected:** a profile is chosen first (or the form has a profile selector), because a grant is security relevant (5, 8).
- **Actual:** the form opens `For MCP profile bad name!` (the first row alphabetically) without any profile screen having been opened or any choice offered; the profile is a label only and cannot be changed in the form. Esc then lands on the MCP Profiles screen instead of the workbench (the palette command opened the profiles screen under the form).

##### [MINOR] Ctrl+P (palette) is swallowed inside MCP Profiles; typed text acts as hotkeys there
- **Where:** mcp.profiles
- **Steps:** open MCP Profiles, press Ctrl+P and type `Revoke All`, Enter (as if the palette had opened)
- **Expected:** the palette opens, or nothing happens; letters never trigger actions silently.
- **Actual:** no palette; the typed letters were read as hotkeys (`R` = revoke all pending, `e` = enable twice), the first profile (`bad name!`) was **enabled** (`enabled bad name! scopes=0 tools=0`). Enabling a profile needs only two plain `e` presses.

##### [MINOR] "Revoke All MCP Grants" (palette) only opens MCP Profiles with a pending confirmation, shows a stale snapshot, and says `1 grants`
- **Where:** mcp.revoke_all
- **Steps:** Ctrl+P, "Revoke All", Enter; then `R` (or Enter)
- **Expected:** a confirm dialog that says what will be revoked (how many grants, which profiles) with Revoke/Cancel buttons (1).
- **Actual:** the MCP Profiles popup opens with the line `confirm revoke all grants` (nothing says that `R` or Enter confirms; Esc closes and cancels); the list shown is the state of the previous opening (a profile I had just disabled in the CLI still showed `enabled=true`); the result is `revoked 1 grants` (plural). From Agent Activity, `r` does the same hop (see above). While a request waits, revoking denies it with the wrong reason (next entry).

##### [MAJOR] Revoking grants denies the waiting request with the reason "a person denied this write"
- **Where:** Agent Activity / revoke all, agent side and audit
- **Steps:** `--ask` grant on pg-dev; client calls `data_update`; in the TUI `r` in Agent Activity then Enter (`revoked 2 grants`)
- **Expected:** the agent is told the grant was revoked (docs: "revoking the grant denies its waiting requests"), the audit shows revoke as the reason (7).
- **Actual:** the client gets `Error [POLICY_DENIED]: a person denied this write` and the audit/Activity line reads `grant data_update deny public.orders a person denied this write`, identical to a real human denial.

##### [MAJOR] Agent Activity: with nothing waiting, PgDn scrolls for half a second and snaps back; only the newest 20 events can ever be seen
- **Where:** mcp.audit / Agent Activity (audit view)
- **Steps:** make > 20 tool calls with `dexo mcp serve` clients (the audit then holds 78 events), no request waiting, resize to 100x24 so the popup overflows (title `Agent activity · PgDn for more`), Ctrl+Alt+A, press PgDn
- **Expected:** the list scrolls and stays where it is (6); older events can be read.
- **Actual:** the content moves for about 0.4 s and returns to the top with the next refresh (sampled every 0.25 s: scrolled, then `Recent activity` back at row 4 and staying). Up/Down/Home/End/wheel do nothing. With a request waiting, PgDn scrolls the whole popup and the position holds (checked with a 60-line SQL: the last line `WHERE id > 0 -- touches EVERY row` is reachable), so the reset only happens in the audit-only view. At any size the screen shows only the newest 20 events (the audit holds 78) with no timestamps, so older events are unreachable in the TUI; only `dexo mcp audit` (raw JSON, epoch timestamps) shows them.

##### [MINOR] Agent Activity: mouse is not supported
- **Where:** Agent Activity
- **Steps:** 3 waiting requests; click on the 1st/2nd request line; wheel over the list
- **Expected:** a click selects the request, the wheel scrolls (1, 6). (MCP Profiles does select on click.)
- **Actual:** nothing happens on click or wheel. Only the confirm buttons `[Approve]` and `[Cancel]` react to the mouse.

##### [MINOR] Agent Activity rows are raw, contradictory and carry no time
- **Where:** Agent Activity, Recent activity
- **Steps:** make a read, a denied read, a refused `delete` via `query_execute_read`, a timeout, a human denial
- **Expected:** one readable line per event with time, profile, tool, what was attempted, and the outcome in words (7).
- **Actual:** lines are the audit columns joined by spaces:
```
pg-dev tools/call data_update allow public.orders POLICY_DENIED
pg-dev grant data_update deny public.orders a person denied this write
pg-dev grant data_execute_sql allow public.orders Succeeded Committed 1 rows affected
pg-dev grant data_execute_sql approved public.orders approved
pg-prod tools/call query_execute_read allow  POLICY_DENIED
pg-dev tools/call list_connections allow  ok
```
Decision `allow` with status `POLICY_DENIED` contradicts itself; `Succeeded Committed 1 rows affected` is a Debug-style status (and `1 rows`); `approved ... approved` repeats; empty object leaves a double space; there is no timestamp, so I cannot tell when anything happened; the object appears as `public.orders` here and `qa7.public.orders` in the Waiting section; the first word is the profile name (`pg-dev`), but in the Waiting line `on pg-dev` is the connection (same name in my setup, ambiguous); six `POLICY_DENIED` rows for different refused statements look identical (no SQL, no reason), so the TUI does show refusals but not which one was what. The agent-facing text has the same Debug flavour: `outcome: "Succeeded Committed 1 rows affected"`, `(1 rows, 3 ms)`.

##### [MINOR] Agent Activity: waiting requests - only the selected one shows its SQL, the confirm line does not repeat it
- **Where:** Agent Activity
- **Steps:** two `data_execute_sql` requests at the same time, select one, `a`
- **Expected:** every request distinguishable; the approval dialog shows the statement that will run (8).
- **Actual:** unselected requests show `data_execute_sql on pg-dev · qa7.public.orders · 297s left` twice, identical; the confirm reads `Run this write now? data_execute_sql on pg-dev · qa7.public.orders · 281s left` with no SQL. `data_update` is shown as raw JSON (`{"identity":{"id":7},"target":"public.orders","values":{"note":"small-term"}}`), not as a row change. Times are raw seconds (`296s left`, `103s left`). The first `a` has focus on `[Cancel]`; a second `a` does nothing (needs Left/Tab then Enter, or a mouse click); `d` opens `Refuse this write?` whose default focus is `[Deny]`, so deny and approve behave differently and the hint line does not say either needs a second step.

##### [MINOR] The "waiting" notice is a short toast titled "warn"; nothing persistent shows pending requests
- **Where:** notice when Agent Activity is closed
- **Steps:** screen closed, a client calls a write under an `--ask` grant, watch for 20 s
- **Expected:** a notice that stays until seen, or a status bar counter, since the request waits up to 120-3600 s (6).
- **Actual:** a toast `warn` / `An agent's write is waiting for your approval in Agent Activity (Ctrl+Alt+A).` appears for roughly 6-12 s; afterwards the status bar shows nothing (`disconnected  Ctrl+J run ...`). Anyone who looks away misses it; the request then silently times out. (The text itself is fine; the label `warn` is a raw level name.)

##### [MINOR] A killed agent: the request lingers as "waiting" and a late approval says "Approved: the agent's write runs now"
- **Where:** Agent Activity
- **Steps:** client calls a write; `kill -9` the `dexo mcp serve` process; look 2.5 s later and press `a`, Left, Enter
- **Expected:** the request is marked withdrawn (docs: "Agent Activity says so instead of approving it"), and approving never says it runs.
- **Actual:** for a few seconds the row stays under `Waiting for you`; approving then gives the toast `Approved: the agent's write runs now.` while nothing runs (row 7 unchanged in the database). After about 6 s the request leaves the Waiting list, and Recent activity keeps `pg-dev grant data_update ask public.orders waiting` forever for both killed requests (no `withdrawn` line). A cleanly cancelled call does show `CANCELLED` / `deny ... cancelled`.

##### [MINOR] Timeout while the confirm dialog is open: the dialog just disappears
- **Where:** Agent Activity
- **Steps:** 20 s approval timeout; press `a` at 6 s left; wait for it to expire; Left, Enter
- **Actual:** the dialog and the request vanish with no word that it expired; the Recent list shows `deny ... no one approved this write within 20s` (good); the later Left+Enter act on nothing.

##### [COSMETIC] Agent Activity: popup draws over the SQL pane border at 120x36 (`┌▸ SQL────┌Agent activity───┐─────────┐`)
- **Where:** Agent Activity with 3 waiting requests
- **Actual:** the popup is 2 rows taller than the space and its top border joins the pane's border on row 3; at 100x24 and 80x24 it also covers the tab bar row and the pane's bottom border shows a second `└──┘` row under it.

##### [MAJOR] MCP Profiles: with more than 11 profiles the selection scrolls out of sight, and `e`/`r` act on rows you cannot see
- **Where:** mcp.profiles
- **Steps:** 17 profiles (`dexo mcp profile create --name extra-01` ...), open MCP Profiles, press Down 14 times, press `e`
- **Expected:** the list scrolls with the selection and the selected row, its details, the status line and the hint line stay visible (6).
- **Actual:** the popup shows the first 11 profiles and nothing else (no details, no status, no hint line); after the 12th Down the `>` marker is gone from the screen. `e` still acts on the hidden selection: it silently **disabled `pg-prod`** (it was enabled) and the second `e` started an invisible "confirm enable" for it. No feedback of any kind is shown. (Profiles 12-17 can never be seen in the TUI.)

##### [MINOR] The waiting request for a destructive DDL without `confirm_target` is queued for a person, who approves it for nothing
- **Where:** Agent Activity with an `--ask` ddl grant
- **Steps:** client calls `schema_apply_ddl` with `DROP TABLE public.qa_agent_t` and no `confirm_target`; approve with `a`, Left, Enter
- **Expected:** the server refuses the call before asking a person (the confirm_target rule is checkable up front), or the TUI shows that it will fail.
- **Actual:** the request waits; the toast after approving says `Approved: the agent's write runs now.`; the agent then gets `Error [PERMISSION_DENIED]: type public.qa_agent_t as confirm_target to confirm` and Recent activity shows `grant schema_apply_ddl allow public.qa_agent_t Failed RolledBack type public.qa_agent_t a...` (Debug words `Failed RolledBack`). The table stayed. With `confirm_target` it dropped fine (`Succeeded Committed committed`).

##### [MINOR] Agent Activity: an admin request (`admin_terminate_session`) says nothing about what it would terminate
- **Where:** Agent Activity, Waiting section
- **Steps:** `--ask` admin grant, client calls `admin_terminate_session` with `session_id` 1147 (a `select pg_sleep(60)` psql session), look at the entry
- **Expected:** the session's user/application/query (or at least the pid and that it is a termination) so a person can decide (8).
- **Actual:** `admin_terminate_session on pg-dev · qa7.public · 118s left` then `{"confirm_target":"1147","session_id":"1147"}`; the object column shows `qa7.public` (the grant's selector), not the target. Approving killed the session; the result text is `Succeeded Committed signal sent`.

##### [MINOR] `--expires`/`expires:` accepts `15m`, `2h` and a bare number (seconds), but not `12s`, `90s`, `1d`, `1h30m`; the error advertises `1s`
- **Where:** mcp.grant form and `dexo mcp grant create`
- **Steps:** `--expires 12s` or `expires: 90s`; `--expires 15`
- **Expected:** the spelling in the error (`1s..=24h`) is accepted, or the field shows what is accepted; a bare `15` is refused or shown as `15s` (a user typing `15` for "15 minutes" gets a 15-second grant without any hint).
- **Actual:** `Error: invalid ttl` for `12s`, `90s`, `1d`, `1h30m`, `1.5h`, `15 m`, `15min`; `15` and `900` are accepted as seconds; the form has no hint about the format. The message `grant ttl must be 1s..=24h` is Rust range syntax. When the grant expires before the approval timeout, the agent is told `no one approved this write within 20s` (it is the grant's expiry, not the approval timeout).

##### [MINOR] CLI messages and output of `dexo mcp grant` are raw
- **Where:** CLI `mcp grant create|list|revoke`
- **Steps:** `dexo mcp grant revoke --id bogus`; `dexo mcp grant create ...`; `dexo mcp grant list --profile pg-dev`
- **Expected:** `no grant with id 'bogus'`; readable times; the list shows what the grant covers (7).
- **Actual:** `Error: invalid character: found `o` at 1` (UUID parser text); create prints `grant 5e2a6723-... expires_at=1790956500 asks: each write waits up to 120s for approval` (epoch seconds); list prints `5e2a6723-... data_write data_update asks=120s expires=1800` (seconds left, no connection, no selector, no ask/one-use words, e.g. `uses=1`); a TUI-made grant has no id visible anywhere in the TUI. `grant create --approval-timeout 50` without `--ask` is refused by clap (`the following required arguments were not provided: --ask`), while the TUI form accepts and silently ignores the timeout when ask is off.

##### [MINOR] Agent side: raw Rust/serde and Debug text in errors and results
- **Where:** MCP tool results (seen by the agent, and echoed in Agent Activity)
- **Steps:** `object_describe {"object": ...}` with the wrong key; any approved write
- **Actual:** `failed to deserialize parameters: missing field `name`` (serde text; the schema says `name`), `Succeeded Committed 1 rows affected`, `Succeeded Committed committed`, `Succeeded Committed signal sent`, `Succeeded Committed applied`, `Failed RolledBack ...`, `(1 rows, 3 ms)`. `object_describe` puts the table note only in the Markdown text; `structuredContent` (columns/rows only) lacks it. `catalog_search` returns internal ids (`pg:table:17128`).

##### [MINOR] `list_connections` and the tool list disagree after a connection becomes production; the refusal reason is TLS, not the production rule
- **Where:** safety, agent side
- **Steps:** grant (`data_execute_sql`) active on pg-dev; edit the pg-dev connection in the TUI, environment `production`; client: `list_connections`, `tools/list`, then the write
- **Actual:** `list_connections` says `production ... writes to production connections are not available over MCP`, but `tools/list` still offers `data_execute_sql` (the grant stays active). The write failed with `Error [INVALID_INPUT]: verified TLS is required for this environment` (the demo server has no TLS), so the production-write guard itself could not be observed in this setup; the audit logs `allow` + `INVALID_INPUT`. Every read on that connection fails the same way.

##### [MAJOR] (layout, found while testing popups) After the terminal is made 20 rows high and back to 120x36, the Sidebar and the Results pane stay hidden
- **Where:** resize / workbench layout
- **Steps:** `qa.sh start`; `qa.sh resize NAME 120 20` (or 60x20, 80x20), then `resize NAME 120 36`
- **Expected:** the panes return when there is room again (6: resizing redraws cleanly).
- **Actual:** only the SQL pane remains (`┌▸ SQL──...` starting at column 1, no `Sidebar`, no `Results`), also after restarting Dexo; only Ctrl+P > "Reset layout" brings them back. 80x24 and 60x30 round trips are fine; heights of 20 trigger it. The hidden sidebar made the "No document open"/connection workflow disappear until I used the palette.

##### [MINOR] Object inspector (`i`, `n` note) shows internal ids
- **Where:** explorer inspector (notes, item 6)
- **Steps:** connect pg-dev, select `orders`, `i`
- **Actual:** `deps: pg:schema:2200, pg:type:17094, pg:table:17104` and `dependents: pg:sequence:17127, pg:constraint:17137, ...` (internal catalog ids, not names; the line is also cut at the popup edge). For a column (`orders.status`) it shows the table's deps.

#### Checked and fine

- Palette lists `Agent Activity` (Ctrl+Alt+A), `MCP Profiles`, `New MCP Grant…` (g) and `Revoke All MCP Grants`; each runs. Ctrl+Alt+A opens Agent Activity from the editor, explorer and with the palette closed; F1 lists `ctrl+alt+a Agent Activity`.
- MCP Profiles: Esc closes; click selects a row; Up/Down/wheel move the selection; `e` on a disabled profile asks first (second `e` enables), disabling is immediate; pending confirmations are cancelled by moving the selection or Esc; the popup fits at 80x24 and 60x20.
- New MCP Grant form: Tab/BTab walk all 9 stops including Create and Cancel; Up/Down move between fields; Left/Right/wrap between the buttons; Enter on a button works; Esc cancels and returns to MCP Profiles; clicking fields, Create and Cancel works; Ctrl+A shows reverse video and typing replaces it; Ctrl+W, Ctrl+Left, Home/End work; Alt/Ctrl letters are not inserted; Space toggles the ask checkbox.
- Grant rules enforced in the form: unknown capability, tool not valid for the capability, connection not in the profile, selector wider than the profile or denied by it, partial wildcards, ttl above 24 h, approval timeout outside 1-3600, wrong confirmation, production connection (`writes to production connections are not available over MCP`), read-only connection (`connection is read-only`, CLI).
- `--ask` flow: toast when Agent Activity is closed; request in the list with SQL, time left and live countdown; approve (`a`, then Approve via Left+Enter or mouse) wrote the row (`note = 'agent'`, checked in `qa7`); deny gave `a person denied this write` and no write; timeout gave `no one approved this write within 20s` and no write; `r` while a request waits denied it; cancelled client call shows `CANCELLED`; multi-line and 3 KB SQL is wrapped and reachable with PgDn while a request waits; three requests at once: Up/Down pick, approving the selected one only; DDL (`CREATE TABLE`, `DROP` with `confirm_target`) and admin (`admin_terminate_session`) approvals work and change the database.
- Agent side refusals: writes without a grant, writes on `pg-prod` (tools not listed, call answers `NOT_FOUND`), grants on production and read-only connections, a read of the denied table `customers` (`NOT_FOUND`, same as an unknown table), `query_execute_read` with `delete ...` (`statement is not allowed for this tool`), `set_config(...)` and `pg_sleep` (`function ... is not allowed`), `select 1; delete ...` (`exactly one statement is required`), a data-modifying CTE. All are written to the audit and show in Agent Activity (as `allow ... POLICY_DENIED`, see the findings above). Nothing reached the database. (A view over the denied table, `paid_orders`, does show customer names, the documented limit.)
- Notes: `i` then `n` saves a table note and a column note in the inspector; `object_describe` shows both to the agent, `catalog_search` finds the table by its note, a `|` in a note is escaped in the Markdown table, a denied table is invisible to the agent.
- `dexo mcp doctor --probe` (scratch HOME): lists profiles, probes enabled ones (pg-dev 13 tools, pg-prod 11), reports the four clients as `no config file`; `--json` returns one document; unknown profile gives `Error: unknown MCP profile 'nonexist'`. `mcp setup --dry-run` prints without writing.
- Palette "Revoke All MCP Grants" and `R`/Enter in the profiles popup revoke the grants (`revoked 1 grants`); Esc cancels it.

#### Not testable

- The production-write guard itself on a real production connection: the demo server has no TLS, so a connection labelled `production` fails earlier with `verified TLS is required for this environment`. Grants on production and read-only connections are refused at creation and were checked.
- `mcp setup` for real (Claude Desktop path comes from `XDG_CONFIG_HOME=/home/winx/.config`, not the scratch HOME, so it would touch the real config; Claude Code writes `.mcp.json` into the current directory), so only `--dry-run` was run.
- MySQL grant flow (a grant on `mysql-dev` was created and revoked via CLI only); `ask` approvals on MySQL were not run.
- Behaviour of the toast with a very narrow terminal (< 60 columns) and with `DEXO_NO_ANIMATION` unset.

### Settings, layout and mouse (settings-layout-mouse)



Binary: dev build 1.4.2. Terminal: tmux 120x36 unless stated. Findings are appended as testing goes.

#### Findings

##### [MINOR] Clicking an option in Settings cycles to the next value instead of choosing the clicked one
- **Where:** settings.open / Settings popup, Accent, Keymap and Mode rows
- **Steps:** Open Settings (Ctrl+P, "Open Settings"). With Accent on Blue, click the word `Rose` (row 11, col 83), or any other spot on the row.
- **Expected:** (standard 1, mouse parity) clicking `Rose` selects Rose; there is a way to go backwards with the mouse.
- **Actual:** a click anywhere on the row (label, option text, blank cell inside the popup) advances the value by one (Blue -> Violet, then Violet -> Green). Clicking `Rose` gave Violet; clicking col 45..55 on the same row gave Green, Amber, Rose, Cyan, ... one step per click. With 6 accents or 3 modes the mouse can only cycle forward; the value you click is never the value you get.

##### [MINOR] Empty-editor hint `Ctrl+N  new query / Ctrl+O  open a file` is hard-coded and wrong under the Emacs keymap
- **Where:** main screen with no document, Settings > Keymap = Emacs
- **Steps:** Settings > Keymap: Emacs, Esc. Look at the editor placeholder; press Ctrl+N.
- **Expected:** the hint follows the keymap (the status bar does: `Ctrl+X Ctrl+N new sql`, palette `Alt+X`).
- **Actual:** placeholder still says `Ctrl+N  new query`; pressing Ctrl+N does nothing. Only `Ctrl+X Ctrl+N` opens the New document dialog. Also the two hints disagree in wording: `new query` (placeholder) vs `new sql` (status bar).

##### [MINOR] Unicode = Off still draws non-ASCII glyphs
- **Where:** settings.unicode / main screen with Unicode Off (also `unicode = "Ascii"` in settings.toml)
- **Steps:** Settings > Unicode: Off, Esc. Open a document, connect, run a query. Count the non-ASCII cells on the screen.
- **Expected:** with Unicode glyphs off the UI falls back to ASCII (the brief: box drawing, arrows, ellipsis).
- **Actual:** the `○`/`●` markers become `o`/`*`, the focused-pane `▸` becomes `>` and `[DEV]`/`[PROD]` replace the dot badges, but still on screen: all box-drawing borders (`┌─┐│└┘`, 500+ cells), `▸` after every connection name and on schema/editor-gutter rows (`o duck-sales▸`, `   1▸SELECT ...`), `▾` for expanded connections, the `…` ellipsis in the tab label (`mysql-d…·query-1.sql`), the `·` separator, `×` on the tab close button, and the em dash in the header line (`Default  pg-dev  —`). Borders may be intended to stay, but the rest are missed spots.

##### [COSMETIC] Modals sit at different heights and have different sizes
- **Where:** Settings, New/Rename document, Open/Save file, Add connection, History, Value, Palette, Help at 120x36
- **Steps:** Open each one in turn.
- **Actual:** all are centred horizontally, but vertically they start at row 1-2 (Help, Row record), row 5 (Open file, Add connection), row 7-11 (Palette, depends on the list), row 8 (Settings), row 9 (History), row 10 (New/Rename document) and row 11 (Run destructive statements). Rename/New document and Add connection are 5-12 rows taller than their content (a block of blank rows under the `[Create] [Cancel]` / `[Submit] [Cancel]` buttons).

##### [MAJOR] "MOUSE OFF · Ctrl+P settings.mouse" shows an internal command id and points to a palette entry that does not exist
- **Where:** settings.mouse / status bar with the mouse switched off
- **Steps:** Settings > Mouse: Off, Esc. Read the left of the status bar (red). Press Ctrl+P and type `settings.mouse` (or `mouse`, `toggle mouse`).
- **Expected:** (standard 7) plain words, and a way back that works: e.g. `Mouse off - turn it on in Settings (Ctrl+P > Open Settings)`.
- **Actual:** status bar says `MOUSE OFF · Ctrl+P settings.mouse  disconnected ...`. `settings.mouse` is an internal id and the palette does not list that command at all (it is hidden: only `Open Settings` matches, see next finding), so the only recovery hint is a dead end.

##### [MAJOR] Cycle Theme / Toggle Light-Dark Mode / Cycle Accent / Cycle Keymap / Toggle Mouse / Toggle Animation / Toggle Unicode / Reset Settings and Hide Explorer / Hide Results / Grow-Shrink Results / Grow-Shrink Explorer are not in the command palette
- **Where:** settings.theme, settings.mode, settings.accent, settings.keymap, settings.mouse, settings.animation, settings.unicode, settings.reset, layout.hide_*, layout.*_grow/shrink (palette)
- **Steps:** Ctrl+P, type `Cycle Theme`, `Toggle Mouse`, `Unicode`, `Hide Explorer`, `Grow`, `Shrink`, `Reset Settings`; also with an empty query and scroll the whole list.
- **Expected:** (standard 2) the palette lists every command and its hotkey; the brief and the command list expect these to run from the palette.
- **Actual:** none of them is listed (only `Cycle Layout`, `Reset layout` and `Open Settings` exist for layout/settings; `Grow` matches only `Toggle Record View`). The settings.* commands have no default hotkey either, so they are unreachable unless the user writes a keymap.toml overlay (I bound them to Alt+T/M/G/K/U/N/Y/Z that way to test them; they then work). The layout hide/grow/shrink commands are reachable only by hotkey (Alt+E, Alt+R, Alt+=, Alt+-, Alt+], Alt+[) and Alt+[ is broken (below). The palette source marks them as hidden on purpose ("already a labelled row inside the Settings screen"), so this may be intended, but then the command list/docs that name them as palette commands are wrong, and the status-bar hint above points at one of them.

##### [MAJOR] Alt+[ (Shrink Explorer Pane) does nothing and swallows the next key
- **Where:** layout.explorer_shrink (Alt+[)
- **Steps:** Grow the explorer (Alt+] several times, or drag). Press Alt+[ (tmux `M-[`, which sends ESC [ as every terminal does). Then press Down.
- **Expected:** the explorer shrinks; the next key works.
- **Actual:** nothing changes (40 presses: width stays 60). The ESC [ pair is taken as the start of a CSI sequence, so the next key is eaten: after Alt+[ the first Down did nothing, the second moved the selection. The key is bound in the Default keymap and shown in the palette list as `Alt+[`, so users will hit it. (Alt+Left in the explorer and the mouse drag do shrink it.)

##### [MINOR] Alt+= / Alt+- mean different things depending on focus, but the hotkey is shown only as "Grow/Shrink Results Pane"
- **Where:** layout.results_grow / results_shrink (Alt+=, Alt+-)
- **Steps:** Focus the explorer (Alt+1), press Alt+= : the explorer grows. Focus the editor (Alt+2), press Alt+= : the results grow.
- **Expected:** one key, one action, or the hint says which pane.
- **Actual:** the command list names Alt+= as "Grow Results Pane", but with the explorer focused it grows the explorer (the [explorer] keymap section rebinds it). Not discoverable.

##### [MINOR] Panes can be shrunk until they are useless
- **Where:** layout.explorer_shrink (Alt+Left / drag), layout.results_shrink (Alt+-), layout.results_grow (Alt+=)
- **Steps:** Focus explorer, hold Alt+Left (or drag the divider to the far left). Focus editor, hold Alt+- (results) or Alt+= (editor).
- **Expected:** a floor that keeps the pane readable (hide commands exist for removing a pane).
- **Actual:** explorer shrinks to 8 columns (`┌▸ Side┐`, names cut to `du`, `my`, `pg`); results shrink to a single row that holds only the `[Grid] Explain Messages` strip (no data row); with Alt+= the editor shrinks to one text line. Maximum explorer width is 50% of the screen (fine).

##### [MINOR] Only one of the two border cells of a divider is draggable
- **Where:** drag pane dividers (layout)
- **Steps:** Between the explorer and the editor the borders sit side by side (`││`, cols 28 and 29 at 120x36). Drag from col 28 and from col 29 to col 60 with `qa.sh drag`. Same for the editor/results divider (rows 23 and 24).
- **Expected:** either cell of the visible divider starts a resize.
- **Actual:** only the right cell (the editor's left border, col 29) resizes; the explorer's right border (col 28) just focuses the explorer. Likewise only the results' top border row (24) drags, not the editor's bottom border (23). A user aiming at "the line" misses half of the time.

##### [COSMETIC] Layout and settings commands give no feedback about what they did
- **Where:** layout.cycle (F10), layout.reset, settings.* via keymap overlay, Settings keys
- **Steps:** Press F10 four times; run Reset layout; with the overlay press Alt+T / Alt+K / Alt+Y.
- **Expected:** (standard 7) success says what changed ("Layout 2 of 4", "Theme: Dracula", "Keymap: Vim").
- **Actual:** the screen changes but no toast or status text names the new layout/theme/keymap; with F10 the user cannot tell which of the 4 layouts they are on or when the cycle wraps. (Inside the Settings popup the value is visible, so it is fine there.)

##### [COSMETIC] Command name case differs: "Reset layout" vs "Cycle Layout" / "Reset Settings"
- **Where:** palette, layout.reset
- **Actual:** `Reset layout` (lower-case l) next to `Cycle Layout`, `Grow Results Pane`, `Reset Settings`.

##### [COSMETIC] Settings footer does not list `e` (cycle theme) which the docs describe
- **Where:** Settings popup footer `up/down field  left/right change  r reset  esc close`
- **Steps:** Focus the Theme row, press `e`.
- **Actual:** `e` steps to the next theme (docs/workbench.md says so) but it is not in the footer; also `Enter` on the Reset button asks `[Confirm reset]` without saying how to cancel (Esc / moving away both work).

##### [COSMETIC] Explain placeholder is cut off at the pane edge instead of wrapped
- **Where:** results view Explain, empty
- **Steps:** Click `Explain` in the results view selector at 120x36.
- **Actual:** `No plan yet. F7 explains the statement under the cursor; Shift+F7 runs it with ANALYZE; th` - the sentence stops mid-word at the border (no wrap, no ellipsis). Worse at narrower widths.

##### [COSMETIC] Right click: grid and tree respond, editor, tabs and status bar do not
- **Where:** right click everywhere
- **Steps:** Right click a grid row (opens the Row actions / Record popup, good), a tree node (opens the node's Actions menu, good), then the editor text, a document tab, and the status bar.
- **Actual:** the editor, tabs and status bar ignore the right click entirely (no copy/paste menu, no tab menu, focus does not even move). Not wrong, just uneven.


##### [MAJOR] Running Dexo in a small terminal permanently hides the explorer and results (compact mode overwrites the saved layout)
- **Where:** layout persistence / compact mode (terminal below 80x24)
- **Steps:** At 120x36 run Reset layout (palette), Ctrl+Q. `qa.sh start NAME 40 12`, Ctrl+Q. `qa.sh start NAME 120 36`. (Same without restarting: `qa.sh resize NAME 60 20`, wait 2 s, `resize NAME 120 36`.)
- **Expected:** a layout saved at one size is not rewritten by a transient small size; the 3-pane layout comes back when the terminal is large again.
- **Actual:** the saved layout row becomes `explorer_visible:false, results_visible:false, explorer_width:20, results_height:6`; at 120x36 only the editor shows (no explorer, no results), with no message. The user has to find F10 / Reset layout / Alt+E / Alt+R. Resizing a tmux pane or a window through a narrow size, or starting once in a small split, is enough. (Compact mode starts below 80 columns OR below 24 rows: 80x24 is 3-pane, 80x23, 79x24 and 100x23 are single-pane, so an 80x24 terminal inside tmux, which has 23 rows, is already compact.)

##### [MAJOR] "Inspect value" shows raw Rust Debug text for numbers and booleans
- **Where:** results grid > right click / Enter on a row > `Inspect value` (modal `Value`)
- **Steps:** On pg-prod (read only query) run `select 'abc' as s, 1.5::numeric as n, now() as t, true as b, null as z, '{"a":1}'::jsonb as j, 7::int4 as i4`; focus Results (Alt+3), Enter on the row, choose `Inspect value` with a click or Enter on each column (Right moves the column).
- **Expected:** (standard 7) `1.5`, `true`, `7`.
- **Actual:** text `abc`, timestamp `2026-10-02 15:48:21.920201+00`, json (pretty) and `NULL` are fine; numeric shows `Decimal("1.5")`, boolean `Bool(true)`, int `I64(7)` (and `I64(3)` for `select 3`). Footer of the modal is `  esc close` (lower case, two leading spaces).

##### [MINOR] "Toggle Light/Dark Mode" cycles through three modes
- **Where:** settings.mode (Alt+M in my overlay)
- **Steps:** Bind settings.mode (keymap.toml), press it three times from Dark.
- **Expected:** Dark <-> Light.
- **Actual:** Dark -> Light -> Low color -> Dark. The command name says toggle Light/Dark; Low color appears unannounced (and with no feedback, see above).

##### [MINOR] The terminal's colour depth (TERM / COLORTERM) is ignored; only NO_COLOR works
- **Where:** looks / Low color mode (observed in tmux with `pipe-pane` to read the bytes Dexo sends)
- **Steps:** Run `env -u COLORTERM TERM=xterm-16color dexo` (also `TERM=linux`, `TERM=dumb`) and count `38;2;` sequences; then `NO_COLOR=1`.
- **Expected:** a 16-colour terminal gets ANSI colour names (the theme has an ansi16 slot for every role, and the code in capabilities.rs maps TERM/COLORTERM to a depth).
- **Actual:** 71 24-bit RGB colour sequences are sent in all three cases (also with Settings > Mode: Low color), so Low color on a real 16-colour terminal depends on the terminal's own approximation. `NO_COLOR=1` does remove all colour and the UI stays usable through markers (`>`, `▸`, `[value]`), though also without bold/dim/reverse (selected grid row is only marked by `▸`).

##### [MINOR] Compact mode (< 80x24) shows one pane and the mouse cannot switch panes
- **Where:** main screen at 60x20 / 40x12
- **Steps:** `qa.sh resize NAME 60 20` (or start at that size). Try to reach the editor/results by clicking.
- **Expected:** a clickable way to change pane, or a visible hint.
- **Actual:** only the focused pane is drawn (explorer first). Switching needs Alt+1 / Alt+2 / Alt+3; the explorer focus hint line does not mention them (the editor focus hint does: `Alt+1 connections  Ctrl+P commands`). The status bar in compact mode also reads `pg-dev  ctrl+p  F1  Enter connect  a actions  n new  e edit` (lower-case `ctrl+p` and F1 moved to the left), cut at the width (`Enter actions  v view  n/p page  Ctrl+W` loses `close`).

##### [MINOR] Help: any click closes the overlay, even a click on the Search field
- **Where:** F1 help
- **Steps:** F1, click the `Search:` row (or a key row, or the blank row under Search).
- **Expected:** a click in the search field focuses it; clicking a row does not close the dialog.
- **Actual:** every click inside or outside the popup closes Help and clears the search; text typed straight after goes into the editor. The help list has no scrollbar or "more" marker either, and the search is a loose subsequence match (`pane` lists `Duplicate Line`, `Discard All Pending Changes`, `Next Data Page`).

##### [MINOR] "Run destructive statements" is the title for a statement Dexo merely cannot parse, and it has no warning styling
- **Where:** Ctrl+J on an unparseable statement
- **Steps:** Editor: `selec nonsense from nowhere`, Ctrl+J.
- **Expected:** a title that matches (`Statement not recognised`) and warning colour (the `Unsaved changes` dialog colours its warning line amber).
- **Actual:** modal `Run destructive statements` / `1. selec nonsense FROM nowhere` / `Dexo could not read this statement` / `[Run]  >[Cancel]`; border and text are plain foreground (same for the `New document` and `Rename document` dialogs, while Settings, Palette, Help, Add connection, Open file and Unsaved changes use a bold accent border).

##### [MINOR] "Search History" is not searchable and shows duplicates
- **Where:** connection Actions > Search History (modal `History`)
- **Steps:** Select a connection, `a`, `Search History`; type `SELECT 1`.
- **Actual:** the modal is titled `History`, has no search field and no hint line; typing does nothing; the same statement is listed five times in a row.

##### [MINOR] Key hints behind and inside modals are inconsistent
- **Where:** status bar, modal footers, key names
- **Actual:** (a) the status bar keeps showing editor hints (`Ctrl+J run  Ctrl+N new sql  Ctrl+W close`) behind Settings and Help, which ignore those keys, while the palette clears them; (b) Esc is advertised four ways: Settings footer `esc close`, Value footer `  esc close`, Actions menu footer `Enter run  Esc close`, Help title `Keybindings  Esc to close`, Row record title `Row 5  Esc to close`, and not at all in New/Rename document, Open/Save file, Add connection, Unsaved changes, History; (c) key names are `Ctrl+Shift+F10` in the palette but `ctrl+shift+f10` / `alt+down` in Help; compact status `ctrl+p` vs wide `Ctrl+P`; (d) the Add connection form uses raw field names (`password_command`, `pre_connect`, `tls_mode`, `ssh_host`) next to `name:` / `driver:`.

##### [COSMETIC] Button rows differ between dialogs
- **Where:** Unsaved changes `>[Save]   [Don't save]   [Cancel]` (three spaces, focus marker glued to the button), Run destructive `[Run]  >[Cancel]` (two spaces), New/Rename document, Open/Save file, Add connection ` [Create]   [Cancel]`, ` [Submit]   [Cancel]` (one leading space, no marker until Tab), Welcome `[Get started]`. The generic label `Submit` is used for Add/Edit connection while the other dialogs name the action (`Create`, `Rename`, `Open`, `Save`, `Run`). Titles mix sentence case (`New document`, `Open file`, `Unsaved changes`, `Add connection`) and title case (`Command Palette`); the menu item `Search History` opens `History`, `New Connection` opens `Add connection`.

##### [COSMETIC] Muted text and Light-mode accents have low contrast
- **Where:** looks (colours read with `qa.sh ansi`, WCAG ratio computed)
- **Actual:** muted hint text such as `WHERE w to filter` / sidebar titles is 3.0:1 in Dracula and 2.8:1 in Tokyo Night (Nord 3.5, Gruvbox 4.0; Dexo dark 6.1). In Light mode the default Cyan accent on the near-white background is 2.5:1 (Amber 2.7, Green 3.3), so the focused-pane border, the active option `[Cyan]` and the unfocused (dim, 2.0:1) borders are all faint. Selected grid rows and the active tab stay clear in every mode and theme (dark-on-accent block).

##### [COSMETIC] Compact status bar, 40x12 cut-offs
- **Where:** 40x12
- **Actual:** the palette cuts a key hint mid-token (`Execute Document Ctrl+Sh`), the Unsaved changes text is cut mid-word (`query-1.sql has changes that are not s`), the Welcome dialog repeats `DEXO` in title and first line, and the Add connection form drops the `Advanced options` row. Everything else (Settings, Help, record popup, forms, close prompt) fits and keeps its buttons reachable down to 40x12.

##### [COSMETIC] F10 cycles four unnamed layouts and one of them leaves 3 inner rows for results
- **Where:** layout.cycle
- **Actual:** the four layouts are (explorer width / results height at 120x36) 28/12, 22/19, 22/5 and 48/10; the third leaves only 3 inner rows for results. F10 also re-shows a hidden explorer. There is no indication which one is active.


#### Checked and fine

- Welcome (first run): `[Get started]` works with the mouse; compact version fits 40x12.
- settings.open (palette, Enter): popup opens, Esc closes, Up/Down walk the fields and the Reset button, Left/Right change the value with wrap-around on every row, `r` arms and a second `r` resets, Esc cancels a pending reset, moving off the button cancels it.
- Every Settings field (Theme 1/6..6/6 Dexo, Dracula, Gruvbox, Nord, Catppuccin, Tokyo Night; Mode Dark/Light/Low color; Accent Cyan..Rose; Keymap Default/Vim/Emacs; Mouse; Animation; Unicode; Updates) changes at once, is shown with `[value]` brackets (readable without colour), and survives Ctrl+Q and a kill + restart (checked in settings.toml and on screen: Amber, Unicode Ascii, Gruvbox, mouse off, Vim `-- NORMAL --`, Emacs hints `Ctrl+X Ctrl+N` / `Alt+X`).
- Mode/Accent changes go back to the Dexo theme (docs say so); the theme presets keep selected rows, focus borders and the active tab clearly visible (dark text on an accent block, bold accent border).
- settings.reset via the Reset button with mouse (two clicks) and keyboard: every field back to default immediately.
- settings.theme / .mode / .accent / .keymap / .mouse / .animation / .unicode / .reset via a keymap.toml overlay: each works and applies at once (they are not reachable otherwise, see findings).
- Mouse Off: applies at once, releases terminal mouse reporting (tmux `mouse_any_flag` 1 -> 0), clicks and wheel do nothing, keys still work, persists across restart, can be turned on again with a key.
- layout.cycle (F10, four layouts, wraps, re-shows a hidden explorer), layout.hide_explorer (Alt+E), layout.hide_results (Alt+R), layout.explorer_grow (Alt+]), layout.explorer_shrink via Alt+Left in the explorer, layout.results_grow / shrink (Alt+= / Alt- with editor focus, Alt+Up/Down), layout.reset (palette), layout stays usable at every cycle step; focusing a hidden pane (Alt+1/3) shows it again; keys do not leak behind an open Settings popup.
- Divider drag (`qa.sh drag`): explorer divider (cell col 29) and results divider (row 24) resize live, clamp at 50% / the minimums, and the result is saved (SQLite `workbench_layouts`, written within ~1 s of the change; F10 is written within ~3 s). Layout and hidden panes survive Ctrl+Q and a kill.
- Mouse: sidebar rows (select, single click on a connection connects), `[n]ew [e]dit [a]ctions` header items, node Actions menu (right click and click), document tabs (switch tab and connection, `×` opens Unsaved changes, `+` opens New document), Unsaved changes buttons (Save -> Save file dialog, Don't save, Cancel), New document / Add connection buttons, results view selector (Grid / Explain / Messages), result tabs (`result 1..3`), grid header click sort (asc/desc/off), grid cell click, right click on a grid row (Record + Actions popup, Copy cell click works, toast `copied to clipboard`), wheel in tree, editor, grid, palette, Help, Open file list.
- Terminal sizes: 200x50, 120x36, 80x24 full layout; 60x20 and 40x12 compact. Palette, Help, Settings (shrinks to one value per row at 40x12), Add/Edit connection form (scrolls, buttons pinned), Unsaved changes, Row record all fit, no popup larger than the screen, resize redraws cleanly, `start` at each size works.
- Looks: Dark, Light, Low color and NO_COLOR render every pane; PROD badge is red in all three modes; errors are red (toast, Messages); selected grid row and active tab are a solid accent block in all modes.

#### Not testable

- A real 16-colour terminal or Linux console (only tmux at TERM=xterm-256color / xterm-16color; observed the bytes via pipe-pane).
- Animation On/Off (qa.sh sets DEXO_NO_ANIMATION=1) and Updates On/Off (DEXO_NO_UPDATE_CHECK=1): the settings toggle and persist, but I could not observe their effect.
- Double-click timing and Shift/Alt/middle-click variants beyond what `qa.sh click` can send; mouse drag selection of text in the editor.
- Quit prompt with unsaved work: Ctrl+Q quit at once with a dirty untitled document (content recovered on restart), so the asking path was not reached.

### Connections and projects (connections-projects)



(Findings are appended as testing proceeds.)

##### [MAJOR] "[offline]" tags appear on every connection after Shift+D and stay, even on connected ones
- **Where:** connection.close_session (Shift+D) / sidebar
- **Steps:** click pg-readonly (connects), focus sidebar, press `D` (Shift+D), then Enter on pg-readonly to reconnect.
- **Expected:** only the disconnected connection is marked, and the mark goes away on reconnect (3, 7).
- **Actual:** all six connections, including ones never connected (duck-sales) get `[offline]`; after reconnecting, the connected row reads `● pg-readonly▾  [offline`  (tag stays and is cut off at the sidebar edge: `[offline` without `]`). A toast said "offline catalog from 2026-10-02 15:22:29". The tags persist for the whole session (also after reconnecting pg-dev).

##### [MAJOR] Changing the driver between server drivers does not change the port (MySQL on 5432, PostgreSQL on 3306)
- **Where:** connection.new / Add connection form, driver field
- **Steps:** `n`, Tab to driver, press Right once (PostgreSQL -> MySQL); keep pressing Right/Left.
- **Expected:** port default follows the driver (5432 / 3306 / 3306) while the user has not typed a port.
- **Actual:** PostgreSQL -> MySQL -> MariaDB keeps `port: 5432`. The port is only reset when coming from SQLite/DuckDB into a server driver (DuckDB -> PostgreSQL gives 5432, SQLite -> MariaDB with Left gives 3306). Going Left from MariaDB to PostgreSQL ends with `driver: < PostgreSQL >` and `port: 3306`. A user who picks MySQL and does not look at the port gets a wrong one.

##### [MAJOR] "Add connection": empty submit says only "password is required"; the error stays after the driver changes to one with no password
- **Where:** connection.new / Add connection form
- **Steps:** `n`, Tab x8 to [Submit] (or just Enter from there), Enter. Then Shift+Tab to the driver field and Right to SQLite.
- **Expected:** the message names what is missing (name, host) in field order; it clears when the form changes (7).
- **Actual:** `error: password is required` although name and host are also empty (fill the name only and Submit: same message). The line is also drawn flush against the dialog border (`│error: ...`, no 2-space indent like the fields) and stays on screen on the SQLite/DuckDB form, which has no password field at all.

##### [MAJOR] The form lets you save a custom environment that then cannot connect; the error is internal jargon with no way out
- **Where:** connection.new / connection.edit, Advanced options, `environment`
- **Steps:** `n`, fill name/host/port/db/user, Advanced options, set `environment: dev`, Submit (saved, "saved x1"). Disconnect, select it and press Enter (or click it).
- **Expected:** the form says which environments exist (local, development, production...) or, for a custom one, asks for read_only right there; the connect error says what to do (7).
- **Actual:** the form accepts any text (the field is prefilled `local`, no list of valid values). Connecting shows the error toast `custom environment requires persisted read_only` (the word "persisted" is internal; nothing says "open Edit, Advanced options, set read_only"). The connection stays unusable.

##### [MAJOR] Submit saves the connection but the dialog stays open when the automatic connect fails; the next Submit says "already exists"
- **Where:** connection.new / Add connection
- **Steps:** `n`, fill name `x3`, host 127.0.0.1, port 55601, db qa5, user dexo, Advanced: `environment: dev`, `password_command: cat $QA/pw`; Tab to the end, Enter (or Tab to [Submit], Enter).
- **Expected:** the dialog closes (the connection is saved) and the connect error is shown as an error, or the dialog stays and nothing is saved (7).
- **Actual:** `x3` appears in the sidebar (saved) but the Add connection dialog stays open with the same values and no error line inside it. Submitting again gives `error: connection 'x3' already exists`. A user will think the save failed.

##### [MINOR] Enter in a text field submits the whole form; nothing in the dialog says so, and the long form has no shortcut to Submit
- **Where:** connection.new / Add connection
- **Steps:** `n`, type a name, Tab Tab, type a host, Enter.
- **Expected:** a footer hint (`Enter save  Esc cancel`) or Enter moving to the next field (1).
- **Actual:** Enter submits at once (`error: password is required`). The dialog has no hint line at all; with Advanced options open it has 25 fields and Tab is the only way to reach [Submit] besides Enter in a field.

##### [MAJOR] tls_mode is a free-text field: any value is saved, a wrong one fails only at connect time with a serde error, and Postgres ignores the setting
- **Where:** connection.new / connection.edit, Advanced options, `tls_mode`; Test Connection
- **Steps:** (1) type `foo` or `require` in `tls_mode`, Submit (saved: `tls = {"mode":"foo"}`), then palette > Test Connection. (2) Set `tls_mode: verify_full` on the Postgres connection `x2` (server has `ssl = off`), `dexo connections test --name x2`, then `dexo query --connection x2 --sql "select ssl from pg_stat_ssl where pid = pg_backend_pid()"`.
- **Expected:** a picker or validation with the real modes (`disable`, `preferred`, `required`, `verify_ca`, `verify_full`); `required`/`verify_full` refuse a server without TLS (8).
- **Actual:** (1) the toast reads `x2: invalid tls config: unknown variant `require`, expected one of `disable`, `preferred`, `required`, `verify_ca`, `verify_full`` (serde text; the allowed values are only in this error, and it is clipped at 120 columns after `req`). (2) Postgres: `ok`, and `ssl` is `false`: verify_full on a plaintext server is accepted and the session is unencrypted (the setting is saved but not applied). MySQL does honour it (`verify_full` against the self-signed server: `Error: Input/output error: Input/output error: invalid peer certificate: UnknownIssuer`, the prefix repeated twice and no hint to set `ca_file`).
- No field has a hint of its values: `proxy_kind`, `read_only`, `confirm_destructive`, `require_verified_tls`, `max_rows`, `timeout_secs` are raw snake_case names too.

##### [MAJOR] Typing a password in "Edit connection" says "saved" but the password is not stored
- **Where:** connection.edit / Edit connection, `password`
- **Steps:** Duplicate a connection (palette "Duplicate Connection" on one whose password is stored: `pwtest` made with the Add form), select the copy, `e`, Tab x6 to `password`, type `abc`, Enter. Toast `saved pwtest (copy)`. Run `dexo connections test --name 'pwtest (copy)'` or connect it in the TUI.
- **Expected:** the new password is kept (or the dialog says it was not).
- **Actual:** `Error: secret is missing for this connection` (also in the TUI: Test Connection says `bad-creds: secret is missing for this connection`). Done twice on `bad-creds`, once on the copy. The same password typed in the Add form IS stored (`pwtest`). So the only way to give a connection a password after creation is the connect-time prompt.

##### [MAJOR] Duplicate Connection silently drops the password
- **Where:** connection.duplicate
- **Steps:** a connection with a stored password (`pwtest`), select it, palette > Duplicate Connection.
- **Expected:** either the copy keeps the password, or the toast says "password not copied".
- **Actual:** toast `saved pwtest (copy)` only. The copy fails with `secret is missing for this connection`. The duplicate also has no chance to edit the name first (it is saved at once as `<name> (copy)` and the selection stays on the original).

##### [MINOR] The password prompt at connect time: "save to the keychain" checkbox is not in the Tab order; a rejected password is stored anyway; a wrong password closes the prompt
- **Where:** "Secret" dialog (Password for <name>) shown when connecting a connection with no stored password
- **Steps:** Browse Connections, select `pwtest (copy)`, Enter. Tab: password -> Submit -> Cancel -> password (the checkbox is skipped). Alt+K and a mouse click toggle it. Type `abc`, tick, Submit.
- **Expected:** Tab/arrows reach the checkbox (1); a failed login does not overwrite the stored secret; the prompt stays open with the error so the password can be retyped (7).
- **Actual:** the toast says `password authentication failed for user "dexo"`, the dialog closes, and `dexo connections test` afterwards still reports authentication failure (the wrong password was written to the keychain).

##### [MAJOR] No way to test a connection before saving; Submit always saves, and a failed connect leaves a broken saved connection and an open dialog
- **Where:** connection.new / Add connection (driver Postgres)
- **Steps:** `n`, name `unreach`, host 127.0.0.1, port 1, db qa5, user dexo, password x, Enter. Then Enter again.
- **Expected:** a [Test] button in the form (the brief and the palette have "Test Connection" only for saved ones), or a clear choice "Save anyway"; the dialog closes or says it saved.
- **Actual:** toast `saved unreach`, the dialog stays open with `error: error connecting to server`; Enter again says `error: connection 'unreach' already exists`. Esc closes it and `unreach` is in the sidebar. The same happens with wrong credentials (`error: password authentication failed for user "dexo"`).

##### [MAJOR] "Unreachable host" error has no cause and no address
- **Where:** Add connection error line, Test Connection toast, `dexo connections test`
- **Steps:** Test/connect a Postgres connection to 127.0.0.1:1.
- **Expected:** `could not connect to 127.0.0.1:1: connection refused` (what happened, to what) (7).
- **Actual:** `unreach: error connecting to server` (TUI) / `Error: error connecting to server` (CLI): the underlying driver text, with no reason (refused / timed out / DNS) and no address.

##### [MAJOR] Validation errors in the connection form are invisible while the Advanced section is scrolled
- **Where:** connection.edit / connection.new, Advanced options open
- **Steps:** Edit a connection (Advanced is open when it has advanced values), Tab to `port`, Ctrl+A, type `abc`, Enter.
- **Expected:** a visible error next to the field or in a fixed line above the buttons (7).
- **Actual:** nothing happens; no message. The error line (`error: port must be a number`) is part of the scrolling field list and is only drawn after the last field (`timeout_secs`), so it appears only when focus is on `timeout_secs` or [Submit] (checked: Tab x20 later it shows `error: port must be a number`). The same hides `password is required`, `already exists`, and connect errors on any submit made from a field in the middle of a long form.

##### [MAJOR] pre_connect: the one error that explains it is cut off; the command must stay in the foreground
- **Where:** connection.new / Add connection, `pre_connect`; connecting the saved connection
- **Steps:** Add MySQL `my-new` with `pre_connect: touch <file>` (a command that exits at once), Submit; then select it and press Enter.
- **Expected:** the whole message visible (7); a field hint saying what pre_connect is for (a tunnel/proxy that must keep running).
- **Actual:** at 120 columns the error toast is one unwrapped line: `pre-connect command `touch /tmp/claude-1000/-home-win...` and is cut at the right border, and the box starts over the sidebar border (`┌┌error`, `││ pre-connect...`, `│└───`). The text that tells the user what to do (`went to the background; it has to keep running in the foreground (drop -f or &), so Dexo can stop it with the session`) can be read only at 300+ columns or in the Messages tab (which does wrap it). The dialog from Add also stays open after Submit with no inline error (the connect failed after the save).

##### [MAJOR] The driver field of the connection form cannot be focused or changed with the mouse
- **Where:** connection.new / Add connection
- **Steps:** `n`; click anywhere on the `driver: < PostgreSQL >  left/right` row (on the label, on `<`, on the name, on `>`, to the right).
- **Expected:** the row takes focus; clicking `<` / `>` cycles the driver (1).
- **Actual:** nothing happens, focus stays on the previous field. Every other field and both buttons respond to clicks. A mouse-only user cannot choose MySQL/SQLite/DuckDB.

##### [COSMETIC] The buttons jump one row down when the error line appears; a click made where the button was lands on the error text
- **Where:** Add connection
- **Steps:** `n`, click [Submit] (row 14): `error: password is required` appears and [Submit]/[Cancel] move to row 15.
- **Actual:** a second click at the same place does not hit a button.

##### [MAJOR] Config transfer import: the conflict list is clipped, shows Rust Debug text, has no key hints, and the import happens silently with a stale sidebar
- **Where:** config.transfer / Import config
- **Steps:** palette > Import/Export Config, `i`, Tab, type the path of an exported file that has 15 conflicting connection names (+ 3 new ones), Enter.
- **Expected:** a list of what will happen per connection (skip / overwrite / rename) with the keys named, all rows reachable (6), a clear "Import" button (1), a result such as `Imported 3, renamed 1, skipped 14`, and the sidebar showing the new connections (7).
- **Actual:**
  - the dialog is a fixed ~14 content lines (even at 120x60): `path: ...` (truncated at the border), `conflicts: bad-creds, duck-new, duck-sales, lite-new, pg-dev, pg-prod,` (also cut) and then `  bad-creds: Skip` ... down to `  x1: Skip`; `x2`, `x3` and anything below are not visible, and there is no hint line, no scroll indicator, no selected row, no Import/Cancel buttons;
  - pressing `r` turns the first row into `bad-creds: Rename("bad-creds-2")`: that is Rust `Debug` output (7); nothing says which key does what (`r` = rename; Up/Down/Tab/Space/s/o/y did nothing visible) and only the first conflict can be changed;
  - Enter applies the import (the CLI `connections list` then shows `bad-creds-2`, `my-new`, `mysql-dev` added) but the dialog does not change, no toast or Messages line says it happened, and the sidebar and Browse Connections still do not show the three new connections (stale until restart). Pressing `i` inside the conflict list silently opens a second file picker over it.

##### [MINOR] Export result is just "ok"; the path is cut off at the border
- **Where:** config.transfer / Export
- **Steps:** palette > Import/Export Config, `e`, Tab, type a full path, Enter.
- **Expected:** `Exported 15 connections and 1 project to <file>` with the path wrapped.
- **Actual:** two lines, `path: /tmp/claude-1000/-home-winx-Documents-github-Dexo/c604136e-4aad-` (cut) and `ok`. The file is correct and has no password (the real password does not appear anywhere in it; `secret_ref = ""`; only `password_command` text is exported). The dialog also keeps `e export  i import  esc close` as the only key hint but `e`/`i` cannot be clicked.

##### [MINOR] The export/import file picker opens in the process's working directory and has no "up to home / jump to path" key besides Backspace/Left
- **Where:** config.transfer / file picker ("Export config to", "Import config from")
- **Steps:** `e` (or `i`).
- **Actual:** starts at the directory Dexo was launched from (here the repo checkout) with a `name:` field that is only reached with Tab (the first focus is the list). Typing a full path into `name:` works.

##### [MAJOR] Config transfer dialog keeps the previous import's conflict list when reopened, and an export over an existing file overwrites it without asking
- **Where:** config.transfer
- **Steps:** import a file with conflicts (see above), Esc Esc, palette > Import/Export Config, `e`, Tab, type the path of an existing file (`export-test.toml`), Enter.
- **Expected:** the dialog opens fresh (`e export  i import  esc close`); an existing file asks "Overwrite?"; the result is shown (7).
- **Actual:** the dialog shows the old `conflicts: ... Skip` list from the import, so the export result ("path / ok") is never displayed; the existing file was rewritten (mtime 12:42:11, 6421 -> 7892 bytes) with no confirmation.

##### [MAJOR] Switch Project with an unsaved document: no confirmation prompt, raw enum text, a hidden key
- **Where:** project.switch / Projects dialog
- **Steps:** in project `qa-proj` create `doc1.sql` (Ctrl+N, name `doc1`, Enter), type `select 1 as unsaved_marker;`. Palette > Switch Project, Up to `Default`, Enter.
- **Expected:** the same dialog as closing a tab: `Unsaved changes: Save / Don't save / Cancel` (5), or the drafts are kept and the user is told.
- **Actual:** the dialog text changes to `> Default switching` and `switch to Default (ConfirmDirty)` (a Rust enum name shown to the user, 7). There are no buttons and no hint. Enter again does nothing. The only way forward is the letter `y` (found by trial; `s`, `d`, Tab, Right do nothing). Esc abandons it. After `y` the project is switched.

##### [MAJOR] A document left unsaved across a project switch comes back with an internal id as its name
- **Where:** project.switch, tab bar
- **Steps:** as above; after `y` switch back to `qa-proj` (palette > Switch Project, Down, Enter).
- **Expected:** the draft comes back as `doc1.sql*` (or asks first) (5, 7).
- **Actual:** the draft text is there, but the tab and the pane title show `e7c03b76-95a8-41ba-…` / `SQL · e7c03b76-95a8-41ba-885c-93d070739f33`, no `*`, and the tab has no connection prefix until a connection is used (then `pg-dev·e7c03b76-95a…`). The status bar says `offline:pg-dev`. Running it (Ctrl+J) connected pg-dev by itself and ran the query (good).

##### [MAJOR] Switching project disconnects every connection without saying so, and marks every row "[offline]"
- **Where:** project.switch / sidebar
- **Steps:** with pg-dev, x2, duck-new, qa-mysql connected, switch to another project.
- **Actual:** all four sessions are closed, the toast says `offline catalog from 2026-10-02 15:30:1...`, and every sidebar row (even those never connected) gets a `[offline]` suffix that is cut off at the sidebar edge (`pg-readonly▸  [offl`). The suffix stays on a row after it is connected again (`● pg-dev▾  [offline]`).

##### [MAJOR] Deleting the active project leaves the app in a project-less state; its document stays on screen
- **Where:** project.delete
- **Steps:** with `qa-renamed` active and a draft document open (`documents=1`), palette > Delete Project, Enter on it, type the name, Enter.
- **Expected:** the app moves to another project (Default), closes the project's documents, header shows the new project (5).
- **Actual:** the header project name becomes empty (`  pg-dev  —`), the deleted project's document (`e7c03b76-95a8-...`) is still open in the editor and can be run (Ctrl+S offers `Save file` with that UUID as the name), and the dialog's `recent:` line still lists the deleted name (`recent: qa-proj, Default`).

##### [MAJOR] Delete Project confirmation: `foo=bar` dump, clipped text, an input with no field or buttons, jargon
- **Where:** project.delete / Projects dialog
- **Steps:** palette > Delete Project, Enter on a project.
- **Expected:** a plain sentence (`Delete project "qa-renamed"? It has 1 document... Type its name to confirm.`), a visible input, Delete/Cancel buttons (1, 7).
- **Actual:** two lines: `delete qa-renamed? connections=0 documents=1 snippets=0 saved queries=` (cut at the border; the last count is not visible, even at 170 columns the dialog keeps its width) and `type name to confirm () connections:detach Alt+C`. The typed text is echoed inside the parentheses (`(qa-ren)`), there is no input field or cursor, no [Delete]/[Cancel], no hint for Enter/Esc. Alt+C flips `connections:detach` to `connections:delete` with no explanation of what detaching means. Enter with a partial name shows only a short-lived warn toast `type the project name to confir...` (cut at the screen edge).

##### [MAJOR] Projects dialog: the `create:` field is after the buttons in the Tab order and has no focus marker; the rename/create dialogs keep stale hint lines
- **Where:** project.create, project.rename
- **Steps:** palette > Create Project; press Tab repeatedly. Palette > Rename Project, Enter on a project.
- **Expected:** fields before buttons; the focused input is marked (1, 3).
- **Actual:** Tab order is list -> [Submit] -> [Cancel] -> `create:` (the input), and while the input has focus no `>` marker moves to it (the list row `> Default` keeps its marker, so the user types "blind"). In Rename the old line `choose project to rename` stays under the `rename: <name>` input and buttons. After creating a project the `create:` input and the buttons disappear from the dialog and it becomes the plain list. The dialog is titled `Projects` for all five commands.

##### [MAJOR] Renaming the active project does not update the header or the recent list
- **Where:** project.rename
- **Steps:** active project `qa-proj`; Rename Project, `qa-proj` -> `qa-renamed`, Submit.
- **Actual:** the list shows `qa-renamed` but the header still reads `qa-proj` and `recent: qa-proj, Default` keeps the old name; after switching to `qa-renamed` the recent line is `qa-renamed, qa-proj, Default` (a project that no longer exists).

##### [MINOR] Projects list: which project is active is not shown; single click only selects, Enter/double-click switches; no hint line
- **Where:** project.browse / project.switch
- **Actual:** `>` marks the selected row, not the active one; the only trace of the active project is the header and the `recent:` line. There is no footer saying `Enter switch  n new  Esc close` (1, 2). Double-click (two clicks within a few ms) switches; two clicks 0.4 s apart do not. After a switch the dialog stays open.

##### [MAJOR] After deleting the active project no project can be switched to until Dexo is restarted
- **Where:** project.switch after project.delete
- **Steps:** delete the active project (see above), close the orphan document (Ctrl+W), palette > Switch Project > Default > Enter. Also: Create Project `p2`, select it, Enter.
- **Expected:** switching works and lands in the chosen project (9).
- **Actual:** every switch shows the error toast `FOREIGN KEY constraint failed` (a raw SQLite error) and nothing changes; the header stays empty. (Not rechecked after restart: see "Checked and fine" for the restart result.)

##### [MAJOR] After a restart an unsaved document is restored as if it were saved: no `*`, and closing it throws the text away without asking
- **Where:** documents / restart; Ctrl+Q; Ctrl+W
- **Steps:** doc `query-1.sql` on pg-dev with `select 111 as from_pg` (tab `pg-dev·query-1.sql*`), a second doc `second.sql` on sqlite-shop. Ctrl+Q (quits at once, no question). Start again: tabs `pg-dev·query-1.sql ×  sqlite-…·second.sql ×` (no `*`). Ctrl+W on the first.
- **Expected:** the draft keeps its unsaved mark and Ctrl+W asks Save / Don't save / Cancel (5); or Ctrl+Q says that drafts are kept.
- **Actual:** the text and the connection binding come back (good) but Ctrl+W closes `query-1.sql` immediately; its content is gone. The same document closed before the restart asks `Unsaved changes ... [Save] [Don't save] [Cancel]`.

##### [MINOR] After a restart the header and status bar show another connection than the visible document's
- **Where:** header line / status bar
- **Steps:** as above; the active tab is `sqlite-…·second.sql`.
- **Actual:** header `Default  pg-dev  —` and status bar `offline:pg-dev` while the document belongs to sqlite-shop; after Ctrl+J they change to sqlite-shop. Documents are bound to their connection but not reconnected until an action needs it (the run connects by itself, which is fine).

##### [MAJOR] SSH tunnel: `missing secret for ssh_password` and the form has no way to supply it
- **Where:** connection.new / connection.edit, Advanced options (`ssh_host`, `ssh_port`, `ssh_user`, `ssh_key`); Test Connection / connect
- **Steps:** Edit a Postgres connection, set `ssh_host: 127.0.0.1`, `ssh_user: nobody` (leave `ssh_key` empty), Submit, then Test Connection.
- **Expected:** a connect-time prompt for the SSH password (like the Secret dialog for the database password) or a clear hint "set ssh_key or run `dexo connections set-secret`" (7).
- **Actual:** toast `x2: missing secret for ssh_password` (internal key name). No Secret prompt appears and the form has no ssh password field. The saved config is `"ssh":{"host":"127.0.0.1","port":22,"username":"nobody"}`.

##### [MINOR] Proxy failures name neither the proxy nor the cause
- **Where:** Advanced `proxy_kind` / `proxy_host` / `proxy_port`
- **Steps:** `proxy_kind: socks5`, `proxy_host: 127.0.0.1`, `proxy_port: 1`, Submit, Test Connection (or `dexo connections test --name x2`).
- **Actual:** `Error: error communicating with the server`. It does not say the SOCKS5 proxy at 127.0.0.1:1 refused the connection. `ssh_port` silently drops non-digits (typing `/nonexistent/key` into it leaves `22`) while `port` keeps the letters and fails on Submit with `port must be a number` (inconsistent).

##### [MINOR] Connection groups are never shown in the sidebar
- **Where:** connection.move_group, sidebar
- **Steps:** palette > Move to Group on `duck-new`, open Advanced options, `group: grp-a`, Submit. Restart Dexo.
- **Expected:** a `grp-a` header (collapsible) in the sidebar with the connection under it.
- **Actual:** the sidebar stays a flat list; `duck-new` just moves to the end (grouped rows sort after ungrouped ones) with no group name. Only Browse Connections shows `grp-a/duck-new`. Before a restart a newly added connection is appended at the bottom instead of its sorted place. "Move to Group" itself opens the whole Edit connection form (focus on `name`, Advanced options collapsed, `group` two Tabs below) instead of a small "Group:" prompt.

##### [MAJOR] Import hides its own safety warning: "need secret" and "runs on this machine when it connects -- read before applying" are clipped out of the dialog
- **Where:** config.transfer / Import, a file without conflicts on every row (re-import of an export after deleting connections)
- **Steps:** export, delete connections, palette > Import/Export Config, `i`, Tab, type the path, Enter.
- **Expected:** the whole report is readable, and the commands that will run (pre_connect, password_command) are listed in full before the user confirms (8).
- **Actual:** the fixed-height dialog shows `conflicts: ...` (cut at the border), eight `<name>: Skip` rows, `need secret: bad-creds, bad-creds-2, duck-sales, lite-new, my-new, mys...` (cut; it also lists file-based connections that have no secret), `runs on this machine when it connects -- read before applying:` and only the first two `my-new runs `touch /tmp/...`` / `... runs `cat /tmp/...`` lines (cut at the border). The remaining lines are below the dialog's bottom edge and cannot be reached (no scrolling). Enter then applies it (the three-line report is not seen), the sidebar still lacks the re-imported connections (`pg-dev`, `pg-prod`, `x1` ... are in the database, not in the list) until a restart.

##### [MINOR] Find Databases in Docker hides containers that already have a saved connection, without saying so
- **Where:** connection.find_docker (palette) / Browse Connections `r docker`, section "Running in Docker"
- **Steps:** with `pg-dev` etc. saved on 127.0.0.1:55601 and `mysql-dev` on 55602, open it: only `pg-commerce-lab [postgres] 127.0.0.1:5432` is listed. Delete every connection that uses 55602 (or 55601), press `r`: `qa-mysql [mysql] 127.0.0.1:55602` (`qa-pg`) appears.
- **Expected:** the container stays listed as "already saved as mysql-dev" or the section says "2 more already saved" (7).
- **Actual:** silent filtering; a user looking for their container thinks Dexo cannot see it. The list is not refreshed after deleting (needs `r`). The prefilled Add form is good: name `qa-mysql`/`qa-pg`, driver, 127.0.0.1, published port, database `dexo`, username `dexo`, password (masked, 23 stars: the container's own password is read). The menu entry says "Find Databases in Docker" while the hint line says only `r docker`. After deleting the last row of Browse Connections the selection jumps into the Docker section (onto `pg-commerce-lab`), so a following Enter would act on a container the user never chose; and after a delete the selection marker is missing until Up/Down.
##### [MAJOR] Sidebar actions menu runs on the active session, not on the connection it is opened for, and does not connect the selected one
- **Where:** sidebar `a` (Actions menu titled with the selected connection): Inspect Sessions, Manage Grants (also Search History, Native Backup/Restore were not run)
- **Steps:** `qa-pg` connected and active, select the offline `mysql-dev` in the sidebar, `a` (menu title `mysql-dev`), Down x7 to `Inspect Sessions`, Enter. Repeat with `Manage Grants`.
- **Expected:** the action connects `mysql-dev` by itself and shows its sessions/grants (5: actions on an offline connection connect by themselves), or says why it cannot.
- **Actual:** `Sessions on qa-pg` opens (a Postgres list with `t terminate`!) and `Manage Grants` shows the Postgres roles `PUBLIC / dexo / pg_read_all_stats`. mysql-dev is never connected, and nothing tells the user that the menu title and the target differ. A terminate or revoke from here would hit the wrong server.

##### [MINOR] Deleting a connection leaves its documents open and unbound; running one silently rebinds it to whatever connection is active
- **Where:** connection.delete
- **Steps:** document `second.sql` on `sqlite-shop` (SQLite SQL), delete `sqlite-shop` from Browse Connections (x, Delete). Click the editor, Ctrl+J.
- **Expected:** the dialog says how many open documents lose their connection, or the documents close/ask (5).
- **Actual:** the delete dialog only talks about the password, saved queries and notes. The tab loses its `sqlite-…·` prefix, and after Ctrl+J it becomes `qa-mysql·second.sql` (the active MySQL connection) and the SQLite statement is sent there.

##### [MINOR] Startup: focus is in the editor, so the first `n` typed (as the Welcome text says) creates a document instead of opening New Connection
- **Where:** first screen after the welcome, `n`
- **Steps:** start Dexo, dismiss the welcome (Enter), press `n`.
- **Expected:** the Welcome says "n in the explorer": the explorer has focus or the hint says to press Alt+1 first (3).
- **Actual:** focus is on the SQL pane (`▸ SQL` marker, no `▸` on Sidebar); `n` is typed into a new unbound document `query-1.sql*` (the text `n` plus the completion popup `now / nullif / NOT ...`), and closing it asks Save/Don't save (fine, but the user did not mean to type). Alt+1 focuses the sidebar. Sidebar Home/End/PageUp/PageDown do nothing (only Up/Down), so reaching row 25 of a long connection list is Down x24.

##### [MINOR] URL / CLI temporary connections
- **Where:** `dexo <URL>`
- **Steps:** `dexo postgres://dexo@127.0.0.1:99999/db`; `dexo /path/shop.sqlite3`; `dexo postgres://dexo@127.0.0.1:55601/qa5 --password-prompt`.
- **Actual:** port 99999 says `the port is not a number` (it is a number, out of range); a plain file path says `not a connection URL: expected scheme://, such as postgres://user@host/db` and does not suggest `sqlite:///path`; the `--password-prompt` line (`Password for dexo@127.0.0.1/qa5:`) echoes nothing at all while typing (no `*`). The temporary row is labelled `[temporary]` but the tag is cut off at the sidebar edge for long names (`● shop.sqlite3▾  [tempor`). Saving a temporary connection (palette > Save Connection...) works (name, driver, host, port, db, user and password prefilled; the saved copy connects and the password is kept), but the old `dexo@127.0.0.1/qa5` row stays in the sidebar next to the new one.

##### [COSMETIC] Layout details
- **Where:** toasts, stacked dialogs, narrow terminals, status bar
- **Actual:** (a) wide toasts start on top of the sidebar frame: `┌┌error────`, `││ pre-connect ...`, `│└────` at 120 columns. (b) When one dialog is opened over another (Add connection over Browse Connections, any dialog at 60x20) the lower dialog's bottom border stays visible as a second `└────┘` line under the top one. (c) At 60x20 the status bar says `ctrl+p  F1` in lower case, at 120 columns `Ctrl+P  F1`. (d) `[offline]` / `[temporary]` / `[production]`-style tags are cut at the sidebar edge (`[offl`, `[off`) on long names. (e) Browse Connections says `connected Idle` for one connection and `active Idle` for another, and shows `active Idle` for a connection the sidebar draws as offline (`○`). (f) After a restart the sidebar sorts alphabetically; within one session a new connection is appended at the end. (g) After saving an edit that renames a connection the selection jumps to the first row. (h) Esc on the Add/Edit form with typed input discards it without asking (acceptable per 1, noted because the form has up to 25 fields).

#### Checked and fine
- `connection.new` (`n` in the sidebar, palette "New Connection"): opens the Add form with name focused; Tab/Shift+Tab order name, driver, host, port, database, username, password, Advanced, [Submit], [Cancel] wraps; Esc closes; clicking every field except the driver, and both buttons, works.
- Driver cycling with Left/Right (PostgreSQL, MySQL, MariaDB, SQLite, DuckDB, wraps); SQLite/DuckDB show only `path`; adding a DuckDB (CSV) and a SQLite connection with a good path works and connects, a wrong SQLite path gives `cannot open <path>: unable to open database file`.
- Single-line inputs in the form: Ctrl+A selects all (reverse video), typing replaces it, Ctrl+Left, Ctrl+W, Ctrl+Backspace, Home, End work; Alt+B is not typed as text; the password field masks with `*`.
- Add form from the Docker list (`qa-mysql`, `qa-pg`): prefilled name/driver/host/port/database/user/password; connects after Submit.
- `connection.edit` (`e` in the sidebar and palette): opens with Advanced pre-expanded when it has values, saves, keeps group/env; `e` on a database child row edits its connection.
- `connection.duplicate`: creates `<name> (copy)` at once (password caveat in findings).
- `connection.delete`: the confirmation names the connection and says what else goes (password, queries, notes); Cancel is the default; Esc, [Cancel] and [Delete] by mouse work; Browse Connections `x` works.
- `connection.test`: palette, Browse `t`, actions menu; follows the selected row; `x2 ok`, wrong password (`password authentication failed for user "dexo"`), missing secret, MySQL TLS error, bad SQLite path all report.
- `connection.close_session` (Shift+D in the sidebar, palette "Disconnect Connection", Browse `c`): disconnects; on an offline connection a warn toast says it is already disconnected.
- `connection.browse` (palette): list with environment, state and `ro`; Up/Down scroll with the selection visible at 60x20; Enter connects, `n`/`e`/`d`/`t`/`x`/`c`/`r` work; Esc steps back one dialog.
- `connection.find_docker`: opens Browse with a "Running in Docker" section (see the hidden-container finding); `r` refreshes.
- `connection.save_temporary`: temporary `dexo@127.0.0.1/qa5` (URL + `--password-prompt` + typed password), `sqlite://<path>` and `--demo` open, are marked `[temporary]`, and Save Connection... saves them; on `--demo` it refuses with a clear sentence; bad URL schemes, missing user and missing file give one-line errors.
- Auto-connect: running Ctrl+J in a document whose connection is offline connects it by itself (also for a document restored after a restart, and after a project switch); tab switching changes the active connection, results and header; drafts and their connection bindings survive Ctrl+Q and restart.
- `project.create`, `project.switch` (Enter or double click), `project.rename`, `project.delete` of a non-active project with a typed name and Alt+C toggle (`connections:detach` / `connections:delete`); the project name survives restart.
- `config.transfer`: export writes a TOML with all connections and no password (the real password string occurs 0 times; `secret_ref = ""`), import with conflicts offers per-row Skip/Rename (`r`), Backspace/Left go up a folder in the file picker, typing a path in `name:` works.
- 80x24 and 60x20: the Add/Edit form fits (width adapts at 60), scrolls with focus and keeps [Submit]/[Cancel] visible; Browse Connections and the Projects dialog fit.
- Ctrl+Q quit cleanly (exit 0); no crash or freeze at any point (`alive` was `running` throughout).

#### Not testable
- The 25-field Advanced section against real TLS/SSH/proxy servers: the QA servers have no TLS (`ssl = off`), no SSH daemon and no proxy, so only the saving, the error messages and (for MySQL) `verify_full` against the self-signed certificate could be seen.
- `ca_file`, `client_cert`, `client_key`, `ssh_key`, `read_only`, `confirm_destructive`, `require_verified_tls`, `max_rows`, `timeout_secs`: fields reached by Tab and typed into; effect not verified (no hint tells which values are valid).
- Keychain behaviour beyond "password typed in Add is stored, typed in Edit is not" (checked through `dexo connections test`); the OS keyring is shared with the user's session, so I deleted my test connections (their secrets) at the end.
- Connect-time Secret prompt focus order for the CLI `--password-prompt` flow beyond the first prompt; Windows/macOS keychains.
- The very first state seen (Add form opened with `host: customers` and an error already shown right after a sidebar click) happened once and could not be reproduced.
