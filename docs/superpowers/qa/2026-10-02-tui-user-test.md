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

### Results grid and table data (results-data)



Binary: dexo 1.4.2 dev build. Terminal 120x36 unless noted.

#### Findings (appended as found)

##### [MAJOR] Copy as CSV never quotes fields: commas, quotes and newlines break the file
- **Where:** data.copy.csv (palette and Results actions menu "Copy as CSV")
- **Steps:** pg-dev, run `select * from customers where id > 198 order by id`, Alt+3, Down Down, Shift+Down Shift+Down (rows 201-203), Enter, pick "Copy as CSV", then `qa.sh clipboard`
- **Expected:** RFC 4180 CSV: `201,"Ana, ""the"" Silva",ana@example.com,,...` and `"multi\nline name"` wrapped in quotes.
- **Actual:** fields are written raw, so the name `Ana, "the" Silva` becomes two columns and the newline splits the record in two:
  ```
  201,Ana, "the" Silva,ana@example.com,\N,2026-10-02 15:18:03.414311+00,\N
  203,multi
  line name,ml@example.com,east,2026-10-02 15:18:03.414311+00,\N
  ```
  The jsonb column (`{"tags": ["a", "b"], "tier": 1}`) is also unquoted, so every row with a jsonb value has extra columns. NULL is written as `\N` (not an empty field), which Excel/Sheets show literally.

##### [MINOR] Copy as Text is space-separated; no tab-separated copy is reachable
- **Where:** data.copy.text
- **Steps:** same selection as above, C-p, "Copy as Text", `qa.sh clipboard`
- **Expected:** text that pastes into a spreadsheet as columns (tab-separated), or aligned columns.
- **Actual:** columns are joined by a single space: `201 Ana, "the" Silva ana@example.com \N 2026-10-02 15:18:03.414311+00 \N`, so values with spaces are indistinguishable from column breaks. NULL again `\N`. (The code has a Tsv format, but no palette command or menu item uses it.)

##### [MINOR] Copy as JSON reorders keys alphabetically
- **Where:** data.copy.json
- **Steps:** same selection, Enter, "Copy as JSON"
- **Expected:** keys in column order (id, name, email, region, created_at, profile).
- **Actual:** `created_at, email, id, name, profile, region` (alphabetical), `id` ends up in the middle.

##### [MINOR] Copy as SQL uses the placeholder table name `tbl`
- **Where:** data.copy.sql
- **Steps:** result of `select * from customers ...`, copy row as SQL
- **Expected:** `INSERT INTO customers (...)` (at least the table of a single-table query / of an opened table), or a prompt for the name.
- **Actual:** `INSERT INTO tbl ("id", "name", ...) VALUES (...)` and it is not schema-qualified. The user must edit every line. Same in a table-data tab where the table is known: open `products`, Enter -> Copy as SQL gives `INSERT INTO tbl ("id", "sku", ...) VALUES (1, 'SKU-1', 'Product 1', 3.17, '{tag1}', TRUE);`. Also the last statement has no trailing newline whereas the CSV/Markdown copies do.

##### [MINOR] Copy as Markdown does not escape a newline or a `|` inside a cell
- **Where:** data.copy.markdown
- **Steps:** same selection, "Copy as Markdown"
- **Expected:** the cell stays on one row (newline -> `<br>` or space; `|` escaped).
- **Actual:** the row is split: `| 203 | multi` / `line name | ml@example.com | ...`, the table is broken. A cell `a|b` is written as `| a|b |` (should be `a\|b`).

##### [MINOR] "copied to clipboard" toast does not say what was copied
- **Where:** Results actions menu -> Copy cell / Copy as ...
- **Steps:** Enter on a row, "Copy cell"
- **Expected:** "copied cell" / "copied 3 rows as CSV" (standard 7).
- **Actual:** toast `copied to clipboard`; for multi-line formats `copied 5 lines to clipboard` (lines, not rows: a 3-row selection says 5 lines, 26 lines for JSON).

##### [COSMETIC] A newline inside a cell shifts the rest of that row one column to the left
- **Where:** results grid
- **Steps:** `select * from customers where id > 198 order by id`; row 203 has name `multi\nline name`
- **Expected:** newline shown as a symbol/space; columns stay aligned.
- **Actual:** the newline occupies no cell, `multiline name` is displayed glued and the row's remaining columns are 1 column left of the other rows (selection highlight of that row also ends 1 column early):
  ```
  │▸ 202 Línea Ñandú        nandu@example.com  south    ...
  │▸ 203 multiline name    ml@example.com     east     ...
  ```

##### [COSMETIC] Results actions menu truncates "Add this column to the sort"
- **Where:** results.actions (Enter in the grid)
- **Actual:** `  Add this column to the … Shift+S` (menu is 36 cols wide although the Record panel next to it has 50 spare columns).

##### [MINOR] Row actions menu does not offer "Copy as Text"
- **Where:** results.actions
- **Actual:** the menu has Copy cell, JSON, CSV, Markdown, SQL, but not Text, which only exists in the palette. Title says "Row 5" even when 3 rows are selected (and the copy does use the 3 rows), so the menu never tells you how many rows an action applies to.

##### [MAJOR] No way to jump to the first/last row or column of a result; Home/End/G/Ctrl+End do nothing; "Results Top" is unreachable
- **Where:** results.top, results.pageup/pagedown, grid navigation
- **Steps:** `select * from events` (10,000 loaded rows), Alt+3, PageDown, then try Home, End, Ctrl+Home, Ctrl+End, Ctrl+Down, g, G; C-p, type "Results Top"
- **Expected:** Home/End (or Ctrl+Home/End) go to the first/last column or row; "Results Top" is runnable from the palette.
- **Actual:** none of those keys does anything. `Results Top` (results.top) is in the command list but the palette shows no match for "Results Top" or "top". Reaching row 10,000 needs ~590 PageDowns; going back needs the same in PageUp. The same applies to columns on the 40-column table: only Left/Right one by one. hjkl (vim keys) do nothing either (not documented, so only noted).

##### [MAJOR] After `t` (count) the title says "(20,000 rows)" but only 10,000 rows can be reached
- **Where:** results title / results.count
- **Steps:** `select * from events` -> title `Results (10,000+ rows, limit reached)`; Alt+3, `t` -> `Results (20,000 rows)`; hold PageDown to the end
- **Expected:** after the count the title keeps telling that only 10,000 are loaded (`10,000 of 20,000 rows`), and there is a way to get the rest (page, or a hint like "add LIMIT/OFFSET or use WHERE").
- **Actual:** title reads `Results (20,000 rows)`, the grid stops at row 10000 (`▸ 10000 k4 ...`) and PageDown/`n` do nothing, no message. The status bar advertises `n/p page` on this result but `n` is silently ignored.

##### [MINOR] Status bar and keybindings help advertise `n/p page` on results that cannot page (and `p` on the first page, `n` on the last, are silent)
- **Where:** status bar hint in Results; data.page_next / data.page_prev
- **Steps:** `select * from events` + Alt+3 + `n` (nothing, no message). Open table `orders`, `p` on page 1: nothing, no message.
- **Expected:** a toast such as "first page" / "not a paged result", or the hint absent.

##### [MAJOR] `n` on the last page (or on a table smaller than one page) loads a nonexistent empty page and blanks the grid
- **Where:** data.page_next, table data `orders` (1,000 rows)
- **Steps:** open `orders` (Explorer, `o`), press `n` 9 times (title `Results (1,000 rows) page:900+100`, no "more"), press `n` once more
- **Expected:** nothing happens (title already knows there is no "more"), or a "last page" message.
- **Actual:** title becomes `Results page:1000+100`, the grid is completely empty (no header either), Console logs `0 rows retrieved starting from 1001 in 0 ms`. `p` returns. The same happens on a table with fewer rows than a page: `products` (51 rows, title `Results (51 rows)`), `n` -> `Results page:100+100`, empty grid, Console `0 rows retrieved starting from 101`.

##### [MINOR] Page indicator is a raw offset+limit dump: `page:100+100 more`, `page:200+100`
- **Where:** results title of table data / filtered results
- **Actual:** `Results (~1.0K rows) page:100+100 more`, `Results (300+ rows) page:200+100`. Users expect "rows 101-200" or "page 2". `300+ rows` on page 3 is also odd.

##### [MINOR] Stale `page:200+100` label stays on later, unrelated results
- **Where:** results title
- **Steps:** `select * from events`, w -> `id > 15000` Enter, `n` `n` (title `page:200+100`); then edit the editor to `select 1 as a` and run; also Execute Document with 4 statements
- **Expected:** a plain `Results (1 row)`.
- **Actual:** title `Results (1 row) page:200+100` on `select 1 as a`, and on every one of the 4 result tabs (`Results (3 rows) page:200+100`).

##### [MINOR] Palette shows `\x` as the hotkey of Toggle Record View, but typing `\x` in the grid does nothing
- **Where:** results.record_view
- **Steps:** Alt+3 on a result, type `\` then `x` (as one string and as two keys); also `x`
- **Expected:** record view toggles (standard 2), or the palette shows no key / "type \x in the editor".
- **Actual:** nothing happens in the grid. `\x` only works as an SQL meta-command in the editor (Ctrl+Enter on a line `\x` -> toast `Expanded display is off.`), and running it clears the Results pane (`Results` with an empty grid) instead of leaving the last result.

##### [MINOR] Record view (one field per line): no field cursor, Left/Right do nothing, "Copy cell" copies an invisible column
- **Where:** results record view, results.actions
- **Steps:** Toggle Record View from the palette; Left/Right; Enter -> Copy cell
- **Expected:** a visible current field (or Copy cell disabled/relabelled).
- **Actual:** Left/Right ignored; the Row actions modal still offers "Copy cell", "Filter rows", "Sort by this column" with a column that is not shown.

##### [MINOR] After sorting (`s`, `S`, header click) the cursor resets to the first row and first column, so pressing `s` again sorts a different column
- **Where:** results.sort_column / results.sort_add_column
- **Steps:** result of `select id, name, region, created_at from customers where id between 195 and 203 order by id`, Alt+3, Right Right (cursor on region), `s` -> `ORDER BY region ASC`, `s` again
- **Expected:** `s` cycles ASC -> DESC -> none on the same column (as header clicks do).
- **Actual:** after the first `s` the header cursor jumps to `id`, so the 2nd `s` gives `ORDER BY id ASC`, the 3rd `ORDER BY id DESC`, the 4th clears. To flip region you must move back with Right Right each time. Row cursor also returns to row 1.

##### [COSMETIC] The ORDER BY bar is only ~34 columns wide and clips the start of the text
- **Where:** results ORDER BY bar
- **Steps:** sort by region, then `S` on name, then `S` on id
- **Actual:** `ORDER BY ion ASC, name ASC, id ASC` (the word `region` is cut at the left without an ellipsis) while the WHERE half of the same row has 55 columns and the line has ~20 spare columns.

##### [MAJOR] A failed WHERE/ORDER BY leaves the bar showing the rejected text while the grid still shows the previous filter
- **Where:** data.filter / data.sort bars on query results
- **Steps:** `w`, `id > 200` Enter (3 rows). `w`, Ctrl+A, `nonexistent = 1`, Enter
- **Expected:** the bar goes back to the active filter or marks itself as failed; Esc/re-edit keeps what is really applied.
- **Actual:** toast `column "nonexistent" does not exist`, results jump to the Messages tab, and when you return to Grid the bar reads `WHERE nonexistent = 1` while the 3 rows shown are those of `id > 200`. Same with `ORDER BY nope desc` (bar says nope, header arrows still show the old name/id sort), and `WHERE aaa ddd` ("Not applied: ... `ddd` follows it").

##### [MINOR] Filter/sort errors expose the internal wrapper query (`_dexo_derived`) and its column offsets
- **Where:** Messages tab after a bad WHERE / ORDER BY
- **Actual:**
  ```
  [12:30:36] error column "nonexistent" does not exist
                   SQLSTATE 42703 · line 1, column 133
                   LINE 1: SELECT * FROM (SELECT id, name, region, created_at FROM customers
  WHERE id BETWEEN 195 AND 203 ORDER BY id) AS _dexo_derived WHERE (nonexistent = 1)
  ```
  The caret and `column 133` refer to a query the user never wrote; the continuation line is also not indented like the others.

##### [MINOR] Messages tab and "Cycle Output View" order
- **Where:** results.cycle_view (`v`)
- **Actual:** `v` cycles Grid -> Explain (Tree) -> Explain (Table) -> Explain (Summary) -> Messages -> Grid, so getting from Grid to Messages takes 4 presses, and each tool-tip toast `copied ...` is added to the Messages tab (Messages(14) after a few copies).
  Also `v` does not show on the palette: Cycle Output View, Select Grid Row, Select Grid Column, Next/Previous Result Tab, Results Up/Down/Left/Right/PageUp/PageDown/Top, Extend Selection, Row Actions, Toggle Row Pick, Next/Prev Data Page and Toggle Row Delete are all absent from C-p (typing their title gives an empty list), so the command list and the palette disagree (standard 2).

##### [MAJOR] Inspect Value shows Rust debug text for integers and decimals (`I64(198)`, `Decimal("4477.50")`)
- **Where:** data.inspect (palette "Inspect Value", Results actions "Inspect value")
- **Steps:** open table `orders` (Explorer, `o`), Alt+2 (results), cursor on `customer_id`, C-p "Inspect Value" Enter; again on `total`
- **Expected:** `198` and `4477.50` (standard 7: no Rust Debug output).
- **Actual:** modal `Value` shows `I64(198)` and `Decimal("4477.50")`. Text, enum, timestamp, NULL and jsonb render correctly (`paid`, `2026-10-02 15:18:03.416165`, `NULL`, pretty JSON); `customers.id` shows `I64(1)`.

##### [MINOR] Pane hotkeys do not match their names in a table-data tab: Alt+3 "Focus Results" focuses the Console, Alt+2 "Focus Editor" focuses Results
- **Where:** focus.results / focus.editor, tab opened with Explorer `o` (orders)
- **Steps:** open `orders`; press Alt+3 (palette and F1 call it Focus Results); press Alt+2
- **Actual:** Alt+3 puts focus on the Console pane (title `▸ Console`), Alt+2 on Results. The status bar becomes just `○DEV pg-dev` (no hints). In a SQL tab Alt+2 = Editor, Alt+3 = Results, so the numbers move with the tab type. Typing "Focus" in the palette (from any pane) lists no Focus ... commands, so they cannot be run from the palette either.

##### [COSMETIC] "1 rows retrieved" in the Console and FK navigation title uses lowercase `where`
- **Where:** Console of table data tab; `Results (1 row) where id = 198`
- **Actual:** `[12:33:03] 1 rows retrieved starting from 1 in 1 ms`. After following a foreign key the WHERE bar still says `WHERE w to filter` although the rows are filtered (`where id = 198` only appears in the title).

##### [MAJOR] Review Changes shows placeholders, not the data: `DELETE ... WHERE id = $n`, `INSERT ... VALUES ($1)`, and long statements are cut at the modal edge
- **Where:** data.review (Ctrl+S), data.apply
- **Steps:** open `orders`, press Delete on two rows, Ctrl+S. Or `i`, fill customer_id=5, status=paid, total=12.34, note=text, Enter, Ctrl+S
- **Expected:** a diff that says which rows go (`DELETE ... id = 597`, `id = 593`) and which values are inserted; long lines wrapped or scrollable.
- **Actual:**
  ```
  ops: 2
  status: Pending
  ready
  DELETE FROM qa3.public.orders WHERE id = $n
  DELETE FROM qa3.public.orders WHERE id = $n
  ```
  Two identical lines: the user cannot tell which rows will be deleted. The INSERT is `INSERT INTO qa3.public.orders (customer_id, status, total, note) VALUE` and cut at the border (Right/End do not scroll it); with only id typed it is `(id) VALUES ($1)`. The modal also has stray lines `ready` and `status: Pending` with no explanation.

##### [MAJOR] Review Changes dialog has no buttons or key hints; Enter applies immediately, and the dialog stays open afterwards
- **Where:** data.review / data.apply (standard 1)
- **Steps:** mark a row with Delete, Ctrl+S, press Enter
- **Expected:** [Apply] [Revert] [Close] buttons or at least `Enter apply  Esc close`; focus visible; Tab/arrows walk the buttons.
- **Actual:** the modal shows only text. Tab/Up/Down/Left/Right do nothing. Enter runs the DELETEs at once (`status: Applied`), no confirmation, the dialog stays open showing the same statements, and the grid silently reloads on page 1 (a user on page 2 of a filtered+sorted view loses their place). Esc closes. On failure it shows `status: Failed` and `error: null value in column "customer_id" of relation "orders" violate` (cut at the border).

##### [MAJOR] Closing a table-data tab with pending changes (Ctrl+W) drops them without asking
- **Where:** document close on a table tab (product rule: unsaved close asks Save / Don't save / Cancel; standard 5)
- **Steps:** pg-dev, Explorer `o` on `orders`, Delete on row 1 (Ctrl+S shows `ops: 1`), Esc, Ctrl+W
- **Expected:** a prompt "Apply / Discard / Cancel" because rows are staged for deletion; the tab shows a modified marker.
- **Actual:** the tab closes at once, the staged DELETE is lost, no message. The tab title never shows a `*`/changes marker while changes are pending (`pg-dev·orders`), nor does the Results title or status bar show a count of pending changes.

##### [MINOR] Pending changes are almost invisible in the grid
- **Where:** data.toggle_delete, data.insert_row
- **Actual:** a row marked for deletion is only drawn in red text (no strike-through, no marker), and the cursor highlight (blue background) disappears on it, so you cannot see which row you are on except for `▸`. A pending INSERT is appended as the last row of the current page (on a 100-row page you do not see it unless you scroll to the bottom, so right after the form closes nothing seems to have happened) and is drawn exactly like a stored row, just with `NULL` in the id column (MySQL orders: `NULL   999   paid   5   NULL`); no colour/marker/`+`. No toast says it was queued (the form just closes). Palette items Apply/Revert/Discard are the only feedback channel: `Revert Changes` and `Discard All Pending Changes` (Ctrl+Shift+R) clear everything silently (no toast, Messages unchanged).

##### [MINOR] Review Changes with nothing pending opens an empty modal; Apply/Revert in the palette say "no pending changes" but Ctrl+S does not
- **Steps:** after Discard All, Ctrl+S
- **Actual:** `ops: 0`, `status: Pending`, `ready`, no statements. Expected a toast "No pending changes".

##### [MAJOR] There is no way to edit an existing cell's value
- **Where:** table data (orders, customers) grid and record/actions menu
- **Steps:** open `orders`, move to `status`/`total`/`note`; try Enter (menu has only Copy/Inspect/Filter/Sort/Count/Related/Back/Refresh), F2 (opens "Rename document" for the tab!), `e` (opens the Export "Transfer" dialog), double-click on the cell, typing a character
- **Expected:** Enter/F2/double-click edits the cell and queues an UPDATE for the Review screen (users expect it from any data grid). At minimum the status bar/menu should say so.
- **Actual:** none of these edits. Only Insert (`i`) and Delete are possible; to change a value you must write an UPDATE in the SQL editor. F2 renaming a table-data tab is surprising.

##### [MINOR] Insert form gives no type, default or nullability hints and does not validate
- **Where:** data.insert_row ("New row" dialog)
- **Steps:** `i`; fields id, customer_id, status, total, placed_at, note (no `*`, no types); put `abc` in total and Insert
- **Actual:** the dialog closes silently (no toast, nothing in the grid) and the invalid value is only discovered when applying: `invalid input syntax for type numeric: "abc"` (no column name). An empty field is omitted from the INSERT (defaults apply), which is good but not explained. The dialog works with mouse (fields and [Insert]/[Cancel] clickable, Esc cancels and the typed values are discarded, Enter inside a field submits the form).

##### [MINOR] Ctrl+N does nothing in a table-data grid; the hotkey the command list gives for Insert Row is not the real one
- **Where:** data.insert_row
- **Steps:** focus the grid of `orders`, press Ctrl+N
- **Expected:** the hotkey shown by the palette. Palette and F1 show `i`; the command list says Ctrl+N. Ctrl+N does nothing (no "New SQL" either, which the status bar advertises elsewhere).
- **Also:** the status bar in a table tab shows `Enter actions  v view  n/p page  Ctrl+W close` and never mentions Insert, Delete, Review (Ctrl+S), Refresh, WHERE/ORDER BY.

##### [COSMETIC] Table title after switching tabs loses the paging info
- **Steps:** open `customers` (title `Results (~203 rows) page:0+100 more`), open `orders` in another tab, come back to `customers` via the tab
- **Actual:** the title is now `Results (100 rows)` (and Console logs `SELECT * FROM qa3.public.customers LIMIT 100` again) although 103 more rows exist; `n` still works.

##### [MINOR] Messages tab opens at the oldest message and the newest ones are off screen
- **Where:** results.cycle_view -> Messages
- **Steps:** after ~20 messages press `v` until Messages
- **Actual:** the list starts at `Connecting to pg-dev…` (12:22) and ends at the 12:30 errors; the newer messages (apply errors, etc.) are below the pane with no scroll indicator; Down/mouse wheel scroll it, End/PageDown do not jump to the newest.

##### [MINOR] Export dialog (`e` in table data) shows raw `key=value` text
- **Where:** Transfer dialog opened from Results
- **Actual:** `format=csv (Ctrl+F to cycle) strategy=Stop` and `progress rows=0 bytes=0 running=false` (standard 7). (Probably covered by the transfer tester; noted because `e` is listed as "Export Data" in F1 under Results.)

##### [BLOCKER] Production apply confirmation can be satisfied by one mouse click, without typing the name; by keyboard it cannot be completed at all
- **Where:** data.review / data.apply on `pg-prod` (production)
- **Steps:** pg-prod, Explorer `o` on `orders`, Delete on a row, Ctrl+S, Enter. A warn toast says `type the target to confirm production apply` and the dialog shows `confirm production to apply`. (a) Type `pg-prod` (or `qa3.public.orders`), Enter: nothing is accepted, no input field is drawn, Tab/Down do nothing. (b) Instead click once on the line `confirm production to apply` (no typing): the line turns into `ready`; press Enter: `status: Applied`
- **Expected:** a visible input labelled with the connection name that has focus on open; applying only when the exact name is typed (standard 8: production asks for the connection's name before any write); keyboard-only users can do it.
- **Actual:** the confirmation field is invisible and not focused (typed text is lost and not echoed), the toast does not say which text is required ("the target": connection or table?), and a plain click on the label confirms it. Verified: row id 3 of `orders` was deleted from pg-prod by Delete, Ctrl+S, one click on the label, Enter, with nothing typed at all; row id 2 was deleted after typing the wrong word `wrong` (after the click).

##### [MAJOR] A table opened from one connection's tree can be bound to a different connection (safety guards of the wrong connection apply)
- **Where:** Explorer `o` (explorer.data) while another connection is the active one
- **Steps:** with `pg-readonly` active (status bar `pg-readonly`), Alt+1, move with Up/Down (without Enter) onto `no_pk` under `pg-dev`, press `o`
- **Expected:** a tab `pg-dev·no_pk` bound to pg-dev (documents belong to the connection of the tree node).
- **Actual:** the tab is `pg-read…·no_pk` and the status bar says `pg-readonly`; applying a change then fails with `connection is read-only` although the table was opened from the pg-dev node. (When the node's connection is activated by Enter first, the tab is bound correctly.) The binding follows the connection that was last connected/clicked, not the node: with `pg-dev` active, `o` on `orders` under the `pg-readonly` node just focuses the existing `pg-dev·orders` tab, so a user who believes they are on the read-only connection edits pg-dev (and the reverse: a user on a pg-dev node can end up on pg-prod or pg-readonly). Pressing Enter on a child node does not activate its connection; only Enter on the connection root does. Worse: with `sqlite-shop` active, `o` on `events` under the `pg-readonly` node opens a tab bound to sqlite-shop that runs the Postgres query `SELECT * FROM qa3.public.events LIMIT 100` against SQLite and fails with `unknown database 'public'`. Tab labels are also cut (`pg-read…·orders`, `mysql-d…·orders`) so the two `pg-read…` tabs and `pg-prod`/`pg-dev` are hard to tell apart.

##### [MAJOR] Insert on a table without key or on a view: the form opens, accepts input, closes, then says `table is read-only`; nothing is queued
- **Where:** data.insert_row on `no_pk` (no primary key) and on view `paid_orders`
- **Steps:** open `no_pk` (pg-dev), `i`, type 9 in `a`, Enter
- **Expected:** `i` refuses before the form (as Delete does) with an accurate reason, or the INSERT works (INSERT does not need a key).
- **Actual:** error toast `table is read-only` after the form closed, the Review shows `ops: 0`. Delete on the same table says `this table has no primary key or unique column, so rows cannot be deleted`, and on the view the same message says "table" (`paid_orders` is a view): `this table has no primary key...`. The wording differs per action, "read-only" is wrong for no_pk. Rows of `no_pk` with identical content (1/x, 1/x) are shown and cannot be told apart.

##### [MINOR] Read-only connection accepts staged changes and says `ready`; only Apply refuses
- **Where:** pg-readonly, data.toggle_delete / data.insert_row / data.review
- **Steps:** open `orders` on pg-readonly, Delete on a row (or `i` + customer_id + Enter), Ctrl+S, Enter
- **Actual:** the row is marked, the Review says `status: Pending` and `ready` (nothing says the connection is read-only), Enter then shows `warn: connection is read-only`. Refusal works, but late; the status bar shows `○DEV pg-readonly` with no read-only badge.

##### [MINOR] Review lists composite-key deletes as invalid SQL, and error toasts are cut at the edge
- **Where:** data.review on SQLite `order_items` (composite PK); MySQL FK error
- **Actual:** `DELETE FROM main.order_items WHERE order_id, product_id = $n`. MySQL insert with a bad FK: toast `...: a foreign key constraint fails (`qa3`.`orders`, CONSTRAINT `orders_ibfk_1` FOREIG│` cut with no ellipsis, and the Review `error:` line is cut at the dialog border (`error: Cannot add or update a child row: a foreign key constraint fail`), so the user cannot read what failed. The Review also says `ready` next to `status: Failed`.

##### [MAJOR] After switching to another tab and back, Delete says "this table has no primary key" on a table that has one (and Insert says "table is read-only")
- **Where:** data.toggle_delete / data.insert_row on a table-data tab (tab switch reloads the document)
- **Steps:** pg-dev, Explorer `o` on `orders`; Delete works (`ops: 1` in Ctrl+S). Discard (Ctrl+Shift+R). Click another tab (e.g. `mysql-d…·orders`), click `pg-dev·orders` again (title is now `Results (100 rows)`, console logs `SELECT * FROM qa3.public.orders LIMIT 100` again), Alt+2, Delete
- **Expected:** same behaviour as before the switch (orders has `orders_pkey`).
- **Actual:** toast `this table has no primary key or unique column, so rows cannot be deleted`, `Ctrl+S` shows `ops: 0`; `i` + Enter gives `table is read-only`. Ctrl+R does not fix it (title returns to `~1.0K rows page:0+100 more`, Delete still refused). Only pressing `o` on the table in the Explorer again (re-opening) restores editing. The same happened to `customers` (which has `customers_pkey` and a unique email). A user will think the table lost its key.

##### [MAJOR] A 40-column table is unreadable in the grid at 120 columns, and its record view cannot be scrolled
- **Where:** results grid / record view, `select * from wide` (id + c01..c40)
- **Steps:** pg-dev, `select * from wide limit 5`, Alt+3. Then palette "Toggle Record View"
- **Expected:** readable column widths with horizontal scrolling (as with `customers`), or at least headers; every field reachable.
- **Actual:** every column is squeezed to 1-2 characters: header `… … … … …` and values `… … …`, column names invisible; Right moves a hidden cursor column by column but the view never scrolls. Only at ~250 columns do names (`c01 c02 ...`) appear (values still `val…`). Record view shows 17 of 41 fields in a 36-row terminal (25 when the results pane is grown with Alt+Up x8); PageDown/Down move to the next record, the mouse wheel does nothing, so fields `c26..c40` can never be seen. Inspect Value works per cell but needs the invisible cursor.

##### [MAJOR] NULL and the text `NULL` look identical; empty string and a single space look identical
- **Where:** results grid and record view
- **Steps:** `select null as n, '' as empty, 'NULL' as txt, ' ' as sp, null::jsonb as nj, 'null'::jsonb as jnull`
- **Expected:** NULL drawn differently (dim/italic or `∅`) so `NULL` the text can be told apart (and an empty string from NULL).
- **Actual:** record view: `n │ NULL`, `txt │ NULL` both in the same colour/style (checked colours: identical); `empty │` and `sp │` both blank. jsonb `null` shows lowercase `null`, SQL NULL in a jsonb column `NULL`. In the grid the NULL cell shows `N…` when the column is narrow.

##### [MAJOR] Inspect Value shows only the first line, cut at the modal border: long text cannot be read
- **Where:** data.inspect on a text cell of 400 characters
- **Steps:** `select repeat('long text ', 40) as longtxt`, Alt+3, Inspect Value from the palette
- **Expected:** wrapped and scrollable (Down/PageDown/wheel) text, with the length.
- **Actual:** one line of ~70 characters (`long text long text ... long text `), the rest is lost; Down, End and the wheel do nothing. In the record view the same value is also cut at the border without wrap or `…`. Tab characters in values are dropped (`E'tab\there'` shows `tabhere`).

##### [MINOR] Row/column "select" commands give no visible feedback
- **Where:** results.select_row (`r`), results.select_column (`c`)
- **Steps:** Alt+2 on `orders`, Right Right, press `c`; then `r`
- **Actual:** nothing changes on screen after `c` (the column is not highlighted, only the header stays reversed), and `r` moves the column cursor to `id` while the row was already highlighted. They only reveal themselves when you copy: after `c`, Copy as CSV copies `status` plus all 100 rows (`copied 101 lines`); after `r`, the full row. The status bar does not say either mode is active. Shift+Left/Right, Shift+Home/End do nothing (selection is rows only).

##### [MINOR] Ctrl+C in the grid does nothing
- **Where:** results grid
- **Steps:** Alt+3 on a query result, Ctrl+C, `qa.sh clipboard`
- **Expected:** copy the current cell/selection like every grid (the editor binds Ctrl+C = Copy).
- **Actual:** silent no-op; the clipboard keeps its old content. Copying requires Enter -> Copy cell or the palette.

##### [MINOR] After over-shooting Alt+Up/Alt+Down on the Results pane, the grid cursor leaves the screen and the view stops following it
- **Where:** results grid after pane resizing (works normally in the default layout: the wheel moves the cursor one row per notch and the view follows)
- **Steps:** Results pane showing 9 rows (after Alt+Up x13 then Alt+Down x8, Alt+Up/Down are not clamped), run a 60-row query, Alt+3, press Down 14 times or wheel down 14 notches
- **Expected:** the view follows the selection (standard 6).
- **Actual:** rows 1..9 stay on screen and the `▸` cursor disappears below the pane (cursor row 15 invisible); another 30 wheel notches scroll the view to rows 23-31 with no visible cursor either. Related: Alt+Up/Alt+Down count presses beyond the limits (pressing Alt+Down 19 times leaves the pane with 3 lines: title, tabs, WHERE bar, zero grid rows, no bottom border, `└WHERE w to filter` overlapping the sidebar corner), so the first Alt+Up/Down presses after hitting a limit do nothing visible.

##### [COSMETIC] Status bar at 60x20 reorders and lower-cases hints; sidebar hides itself and does not come back
- **Where:** layout at small size
- **Steps:** resize 60x20 then 120x36
- **Actual:** the 60-col bar reads `pg-dev  ctrl+p  F1  Enter actions  v view  n/p page  Ctrl+W` (lowercase `ctrl+p`, `F1` moved before the hints, `Ctrl+W close` cut to `Ctrl+W`; at 80 cols it is `Ctrl+P  F1` right-aligned). At 60 columns the Explorer and Console panes are removed (fine, the grid and Review fit); back at 120x36 the Explorer stays hidden until Alt+E.

##### [COSMETIC] Empty result shows only the header and no "0 rows" text; Esc in the grid does not clear a multi-row selection
- **Where:** `select * from orders where false`; grid selection
- **Actual:** header row only, empty body (title says `(0 rows)`). After Shift+Down selection, Esc does nothing (Up/Down collapse it; Esc does not even move focus).

##### [MAJOR] After closing a document the status bar and the actions keep the closed tab's connection: the Insert form is empty and Delete says "no primary key"
- **Where:** tabs / data.insert_row / data.toggle_delete (standard 5: switching tabs switches the session)
- **Steps:** with `pg-dev·products` (table data, 51 rows) focused, C-p "New Document" Enter (a new tab `sqlite-…·New Docume…` appears and the status bar changes to `sqlite-shop` because the explorer cursor was on sqlite-shop, not on the active tab's connection), Ctrl+W to close it, back on `pg-dev·products`
- **Expected:** status bar `pg-dev`, and `i` shows the 6 fields of `products`.
- **Actual:** the header and the bottom bar stay `sqlite-shop` while the grid shows pg-dev rows; `i` opens a "New row" dialog with no fields at all (only `[Insert] [Cancel]`); Delete says `this table has no primary key or unique column`; Ctrl+S shows `target: qa3.public.products`, `ops: 0`. (Same family as "after switching tabs Delete says no primary key".)
- **Also:** Ctrl+N (listed as New Document in the palette) does nothing while the Results pane has focus (only the palette entry works), and the document it creates is bound to the Explorer's connection, not to the tab you are in, and is named `New Document.sql` instead of `query-N.sql`.

##### [MAJOR] "Copy as CSV/JSON/Markdown/SQL/Text" from the palette copies only the cursor cell; the same names in the Enter menu copy the whole row
- **Where:** data.copy.csv / .json / .markdown / .sql / .text (palette) vs results.actions menu
- **Steps:** `select 1 as a, 2 as b, 3 as c union all select 4,5,6 union all select 7,8,9`, Alt+3 (3 rows x 3 columns), Down (cursor on row 2, column a). C-p "Copy as JSON" Enter, `qa.sh clipboard`. Then Enter -> Copy as Markdown, `qa.sh clipboard`
- **Expected:** the same scope for the same command name (the selected rows, or the whole result), and a toast that says what was copied.
- **Actual:** palette: `[ { "a": 4 } ]` (one cell, one key); `Copy as CSV` gives `a` / `1`. Enter menu: `| a | b | c |` / `| 4 | 5 | 6 |` (the row). After `r` the palette copies the row (`x,y,z` / `a|b,q"r,k,l`), after `c` the whole column, and the mode survives into other results and tabs until another key resets it (a later query in another tab copied only column `x` of a 3-column result). The toast only says `copied 2 lines to clipboard`, so the user cannot tell they exported 1 of 9 cells.

##### [COSMETIC] Palette `New Document` asks for a name; Ctrl+N does not
- **Where:** palette "New Document" vs Ctrl+N in the editor
- **Actual:** the palette item opens `New document  name: ...  [Create] [Cancel]`; Ctrl+N creates `query-N.sql` at once. No hint in the palette tells them apart.


#### Checked and fine

- results.up / results.down / results.left / results.right (arrows): move the cursor, header of the current column reversed, selected row highlighted with a blue background and `▸`.
- results.pageup / results.pagedown: move one page (17 rows at 120x36), fast (61 PageDowns in <1 s on a 10,000-row result, no crash).
- results.extend_up / results.extend_down (Shift+Up/Down): rows 201-203 selected, selected range drawn in a darker blue, cursor row lighter; Up/Down collapse it; a copy of the selection exports all selected rows.
- results.toggle_pick (Ctrl+Enter): picks non-contiguous rows (rows 5 and 7 both marked `▸`).
- results.actions (Enter): modal with Record panel plus Actions; Copy cell / JSON / CSV / Markdown / SQL / Inspect / Filter / Sort / Sort by this column / Add this column / Count / Related / Back / Refresh all run; mouse click on an item runs it; Esc closes.
- results.record_view (palette): one field per line, values aligned, NULL/jsonb/arrays/booleans/unicode shown correctly; Up/Down switch record.
- results.cycle_view (`v`): Grid -> Explain (Tree/Table/Summary) -> Messages -> Grid; clicking Grid/Explain/Messages tabs works.
- results.next_tab / results.prev_tab (`]`, `[`) and clicking `result 1..4`: switch the 4 result tabs of an Execute Document run, wrap around.
- results.count (`t`): `10,000+ rows, limit reached` becomes `20,000 rows` on `events`; `~1.0K rows` becomes `996 rows` on orders.
- results.sort_column / results.sort_add_column (`s`, `S`) and header click / Shift+click on header (sent as SGR with the shift bit): ASC -> DESC -> none, second column adds `▲2`, sort is server-side and shows `ORDER BY ...`.
- data.filter (`w`) and data.sort (`o`) bars: typing, Enter applies, Esc restores the previous text, clearing the text and Enter removes the filter; Ctrl+A shows reverse video and typing replaces it, Ctrl+W, Ctrl+Backspace, Ctrl+Left/Right, Home/End work; clicking the bar focuses it; `;` is refused (`Not applied: a clause is one part of one statement: no ;`); quoted mixed-case column `"Label"` works.
- data.page_next / data.page_prev (`n`/`p`) on table data: 100-row pages, `page:100+100 more`, fast (50 pages of `events` in <3 s); `~20.0K rows` estimate for events.
- data.refresh (Ctrl+R): reloads; with pending changes it says `Apply or discard the pending changes before reloading the table.`
- data.related (`f`) and data.nav_back (`b`): modal lists `← order_items (order_id)` and `→ customers (customer_id)`; Enter opens the related rows in a new tab with the filter in the title; `b` closes it and restores the page, filter and sort; `b` on a tab not opened that way says `These rows were not opened from a related row; there is no way back.`; `f` on a query result says `Related rows are a table's; open a table's rows first.`
- data.inspect on text, enum, timestamp, NULL and jsonb (pretty-printed) values (integers and decimals are broken, see findings).
- data.toggle_delete (Delete) on orders / MySQL legacy_log / SQLite order_items (composite key): row marked in red, applied by Review; works on pg-dev, MySQL (MyISAM table too) and SQLite; refuses on `no_pk` with a clear message.
- data.insert_row (`i`) form: Up/Down walk fields, Esc cancels and discards, Enter submits, clicking fields and [Insert]/[Cancel] works; inserts with defaults omitted (id serial, placed_at now()), text with quotes and commas, array `{a,b}` and boolean values, mixed-case table `"MixedCase"`, MySQL.
- data.apply / data.revert / data.discard_all (Ctrl+Shift+R sent as CSI-u): apply works on pg-dev, MySQL, SQLite, DuckDB view is refused as read-only; failures show `status: Failed` plus the database error and keep the change pending; Revert/Discard clear it.
- pg-readonly: Apply refuses with `connection is read-only` (late, see findings).
- Production connection: apply asks for confirmation (but see the blocker).
- Mouse: clicking a cell selects row and column; clicking a tab switches; clicking the WHERE/ORDER BY bars; wheel scrolls the grid (moves the cursor one row per notch in the default layout); clicking Insert form fields/buttons and menu items.
- Small terminals: 80x24 grid and Review fit; 60x20 hides the sidebar/console, grid and Review still fit and work.
- `events` (20,000 rows) in a query: 10,000 loaded with `limit reached`, count button, PageDown speed fine; as table data paging is fast.
- No crash or freeze in the whole session (`qa.sh alive` always `running`).

#### Not testable

- Ctrl+Shift+R, Shift+click on headers: tmux cannot send them; sent as raw CSI-u / SGR sequences instead (worked).
- DuckDB `sales`: only filter, sort, paging, Delete/Insert refusal checked (it is a view over a CSV).
- Row actions on a result of a multi-statement run through every tab combination, and `results.top` (no palette entry, no key bound: reported).
- Copy via the real system clipboard (disabled): checked through OSC 52 and `qa.sh clipboard` only.
- Typing the production connection's name in the review dialog by keyboard: no input is shown or focused, so it could not be completed that way (see blocker).
- Behaviour of Delete/Insert on MySQL views and of MySQL/SQLite `WHERE` syntax errors beyond one FK error.
