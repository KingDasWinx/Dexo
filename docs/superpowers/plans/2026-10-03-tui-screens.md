# Screens: implementation checklist

Spec: `docs/superpowers/specs/2026-10-03-tui-screens-design.md`. Decisions taken: `Ctrl+G` + letter,
Esc goes back, History is a screen, the work ships in the same release as the 1.4.2 fixes.

Status marks: `[ ]` to do, `[x]` done (with commit), `[~]` in progress.

## 1. Frame

- [x] F1 `Screen` enum (Workbench, Connections, Agents, Server, Compare, History) on the model, with
      `previous_screen`; actions `GoToScreen(Screen)` and `ScreenBack`.
- [x] F2 Header strip in row 1: screens on the left (current reversed + brackets without colour,
      waiting counts), context on the right; narrow widths keep the current screen and screens with
      work; a click on a name switches.
- [x] F3 Key routing: true overlays first, then the current screen's handler, then the keymap. On a
      screen other than the workbench only screen-safe commands run from keys (palette, quit, help,
      settings, screens); a workbench command from the palette goes to the workbench and runs there.
      Esc with nothing left to clear goes back.
- [x] F4 `ctrl+g w/c/a/s/d/h` and `ctrl+g ctrl+g` in the three keymaps; palette commands `screen.*`;
      a which-key popup listing what can follow a pending chord (also helps Emacs' `ctrl+x`).
- [x] F5 Status bar per screen (its name and its keys); F1 help gets a Screens section.
- [x] F6 Too-small guard: below 20x8 every screen says so (the smallest size the tests already cover).

## 2. Agents

- [x] A1 `mcp_audit` and `mcp_profiles` lose `open`; the Agents screen owns them, with a view
      (Approvals, Activity, Profiles) switched by `[`/`]` and `1`-`3`. Old commands (`mcp.audit`,
      `mcp.profiles`, `mcp.grant`, `mcp.revoke_all`, `Ctrl+Alt+A`) open the screen on the right view.
- [x] A2 Approvals: list on the left, the picked request in full on the right, approve/deny confirm
      in the detail pane.
- [x] A3 Activity: structured audit events (time, profile, tool, target, outcome, duration, rows) as a
      table; `/` filter; detail of the picked call.
- [x] A4 Profiles: list left, detail right; the grant form in the right pane instead of a dialog.
- [x] A5 Waiting requests: count on the strip, one toast naming `Ctrl+G a`; polling only while useful.

## 3. Server

- [x] S1 `admin` becomes the Server screen: sessions table at full width, detail pane (full query,
      blocking chain), refresh every 2 s while shown, `p` pause, `r` now, `c` pick the connection.

## 4. Connections

- [x] C1 Browse Connections becomes the screen: list grouped by group, Docker section, details pane.
- [x] C2 Add/Edit form in the right pane (Esc back to details); sidebar `n`/`e` open it there.

## 5. Compare

- [ ] D1 Schema diff as a screen: setup row, differences list with filters, DDL of the picked one,
      whole script on `s`, Enter opens it on the From connection.

## 6. History

- [ ] H1 History and Saved as one screen with two views, search, preview pane; Enter opens a new
      document and returns to the workbench.

## 7. Object views

- [ ] O1 Table document views Data / Structure / DDL / Privileges; Inspect opens Structure; Manage
      Grants becomes Privileges.

## 8. Finish

- [ ] Docs pages (`docs/src`): screens, keys, MCP, sessions, connections.
- [ ] Tests that named the old dialogs moved to the screens; snapshots reviewed.
- [ ] QA pass through `qa.sh` on every screen at 160x45, 120x36, 80x24, 60x20, 40x12.
- [ ] Full gate, `cargo deny check`, `cargo check --locked`.
