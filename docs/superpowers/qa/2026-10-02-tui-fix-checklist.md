# TUI fix checklist (from the 2026-10-02 user test)

One line per finding of [the user test](2026-10-02-tui-user-test.md), worst first within each area. `ID` is the area code and the finding's position in its area of that file.

Mark `[x]` when fixed, with the commit; `[=]` when another fix resolved it (name its ID); `[-]` when it is not a defect after all, with the reason.

| Code | Area |
| --- | --- |
| PC | Palette, documents, running SQL, recovery |
| ED | SQL editor |
| RD | Results grid and table data |
| ES | Explorer, schema tools, explain |
| CP | Connections and projects |
| AT | Transactions, sessions, import, export, backup |
| MA | MCP and agents |
| SL | Settings, layout, mouse |

## PC: Palette, documents, running SQL, recovery

- [x] **PC-18** `BLOCKER` Save picker overwrites an existing file without asking (data loss)
- [x] **PC-19** `BLOCKER` Opening a binary file fails with a raw error and still creates a document bound to that file; saving it destroys the file
- [x] **PC-01** `MAJOR` Palette search ranks loose subsequence matches above the obvious prefix/word match
- [x] **PC-02** `MAJOR` PageUp / PageDown do nothing in the palette list
- [x] **PC-05** `MAJOR` Most commands of the registry are missing from the palette (not searchable, not listed)
- [x] **PC-11** `MAJOR` Alt+Left / Alt+Right switch the document but not the session: header and status bar keep the old connection, no reconnect
- [x] **PC-12** `MAJOR` Ctrl+Shift+Tab (Previous Document) does nothing
- [x] **PC-20** `MAJOR` Save/Open pickers show the start of the path, so the current folder is never visible
- [x] **PC-21** `MAJOR` Open file picker: in a folder with many entries the name field and the [Open]/[Cancel] buttons are pushed out of the dialog
- [x] **PC-25** `MAJOR` After a crash (kill) the recovered documents lose their connection and the "unsaved" marker
- [x] **PC-26** `MAJOR` Session recovery: no offer, "Session recovery" shows `key=value` debug text, and Recover / Discard cannot be run
- [x] **PC-28** `MAJOR` Resizing small and back leaves the explorer and results hidden, with focus on the invisible explorer
- [x] **PC-29** `MAJOR` A transaction opened with plain SQL (`begin;`) is not tracked: no indicator, and Ctrl+Q quits without asking
- [x] **PC-33** `MAJOR` Palette `save` puts `Open Saved Query…` first; transposed typos find nothing or the wrong command
- [x] **PC-35** `MAJOR` Ctrl+N creates the document on the "active" connection, not on the connection under the explorer cursor; the status bar names a third one
- [x] **PC-03** `MINOR` Palette with no match shows an empty box and no message
- [x] **PC-06** `MINOR` The empty palette lists commands that cannot work in the current context, unordered, with repeated group names
- [x] **PC-07** `MINOR` Hotkeys shown in the palette differ from the command table (or from the help)
- [x] **PC-08** `MINOR` F1 help: sections are sorted by key, not by purpose; layout keys appear under [Editor]
- [x] **PC-09** `MINOR` F1 search is a loose subsequence match and shows unrelated rows
- [x] **PC-13** `MINOR` Query running: no sign anywhere that something is running; a second Ctrl+Enter is silently queued
- [x] **PC-14** `MINOR` Cancelling a query is reported as an error, and a timeout hits after 30 s with no hint
- [x] **PC-15** `MINOR` Error toast never goes away by itself
- [x] **PC-16** `MINOR` Confirmation for an unparsable statement is titled "Run destructive statements"
- [x] **PC-22** `MINOR` Pickers: PageUp/PageDown/Home/End do not move the file list; there is no hint for Esc
- [x] **PC-23** `MINOR` Rename / New document dialogs: empty name closes silently; long names are clipped and the caret disappears
- [x] **PC-24** `MINOR` Execute Selection with no selection, and Ctrl+F2 with nothing running, give no feedback
- [ ] **PC-27** `MINOR` Tab strip / tab focus details
- [x] **PC-30** `MINOR` Running a document that has no connection says "session is closed"
- [x] **PC-34** `MINOR` Cursor does not jump to the failing statement on MySQL and SQLite; SQLite error says `SQLSTATE 1`
- [x] **PC-36** `MINOR` Ctrl+S with focus in the Results pane opens "Review changes" with raw text instead of saving the document
- [x] **PC-37** `MINOR` Ctrl+Q while a query is running quits at once and leaves the statement running on the server
- [x] **PC-38** `MINOR` Header and status bar stay on the old connection after closing a document too
- [x] **PC-04** `COSMETIC` "Reset layout" is not Title Case
- [ ] **PC-10** `COSMETIC` Welcome: the last hint is cut off at 60x20
- [x] **PC-17** `COSMETIC` Error underline covers the semicolon; void value shown as `\x`
- [x] **PC-31** `COSMETIC` Single-line inputs never scroll horizontally (palette query, New document, Rename, Save, name field)
- [x] **PC-32** `COSMETIC` Narrow status bar shows lower-case `ctrl+p  F1` before `Alt+1 connections  Ctrl+P commands`
- [ ] **PC-39** `COSMETIC` Diagnostics export: file name field starts empty, bundle is a ZIP whatever the name, log tail is empty, key=value text

## ED: SQL editor

- [ ] **ED-01** `MAJOR` Table completion inserts a bare name for a table outside the search path, so the accepted query fails
- [ ] **ED-05** `MAJOR` Ctrl+H (Find and Replace) does not open Replace; it deletes text
- [ ] **ED-09** `MAJOR` Paste fails when the system clipboard is unavailable, with a raw backend error
- [ ] **ED-13** `MAJOR` Picking an entry in History replaces the active document tab (no Save / Don't save prompt) and runs the statement
- [ ] **ED-14** `MAJOR` "Search History" has no search
- [ ] **ED-15** `MAJOR` Clear History: the confirmation is an empty box, and the command refuses when history was not loaded
- [ ] **ED-16** `MAJOR` Parameters prompt starts pre-filled with the last value typed anywhere, so typing appends to stale text
- [ ] **ED-25** `MAJOR` Unbound Alt+letter combinations are typed into the document
- [ ] **ED-26** `MAJOR` Emacs keymap is only a partial overlay: Emacs motion keys do other things
- [ ] **ED-30** `MAJOR` Writes on MySQL give no feedback at all (no "N rows affected", empty Results, nothing in Messages)
- [ ] **ED-02** `MINOR` `join ` table list ranks a table from another schema first and unrelated tables ahead of the FK target
- [ ] **ED-03** `MINOR` No live diagnostic for a mistyped keyword or other syntax error
- [ ] **ED-04** `MINOR` Enter right after typing a complete table name only accepts the completion, the newline is swallowed
- [ ] **ED-06** `MINOR` "Find and Replace" opens with the Replace field focused and nothing shows which field has focus
- [ ] **ED-07** `MINOR` Reopening Find keeps the previous term but does not select it
- [ ] **ED-10** `MINOR` Go To Definition says "no definition at cursor" unless the object is already loaded in the explorer
- [ ] **ED-12** `MINOR` Input sequence glued to a preceding Esc is typed into the document as text
- [ ] **ED-17** `MINOR` Parameter values are reused silently on the next run; the way to change them is a command called "Submit Parameters"
- [ ] **ED-18** `MINOR` A statement Dexo cannot read is called "destructive"
- [ ] **ED-19** `MINOR` Insert Snippet is a dead end
- [ ] **ED-21** `MINOR` Ctrl+A in an empty document swallows the next typed character
- [ ] **ED-23** `MINOR` Completion in a join inserts an ambiguous bare column name
- [ ] **ED-24** `MINOR` Clicking an item in the completion popup does not accept it
- [ ] **ED-27** `MINOR` History is not scoped to the connection, but "Clear History" is
- [ ] **ED-28** `MINOR` After the window was shrunk to 60x20, explorer and results stay hidden when it grows again
- [ ] **ED-32** `MINOR` Tabs of documents on an offline connection do not say which connection they belong to
- [ ] **ED-08** `COSMETIC` Find bar hint is cut off at the right edge
- [ ] **ED-11** `COSMETIC` Ctrl+Home / Ctrl+End do not go to the start / end of the document
- [ ] **ED-20** `COSMETIC` SQLite errors are printed with `SQLSTATE 1`
- [ ] **ED-22** `COSMETIC` Wheel-scrolling the Messages tab past the last message leaves an almost empty pane
- [ ] **ED-29** `COSMETIC` Completion popup is not repositioned to fit narrow terminals
- [ ] **ED-31** `COSMETIC` Welcome text names Ctrl+J, the palette and F1 name Ctrl+Enter for the same action

## RD: Results grid and table data

- [x] **RD-37** `BLOCKER` Production apply confirmation can be satisfied by one mouse click, without typing the name; by keyboard it cannot be completed at all
- [x] **RD-01** `MAJOR` Copy as CSV never quotes fields: commas, quotes and newlines break the file
- [x] **RD-10** `MAJOR` No way to jump to the first/last row or column of a result; Home/End/G/Ctrl+End do nothing; "Results Top" is unreachable
- [x] **RD-11** `MAJOR` After `t` (count) the title says "(20,000 rows)" but only 10,000 rows can be reached
- [x] **RD-13** `MAJOR` `n` on the last page (or on a table smaller than one page) loads a nonexistent empty page and blanks the grid
- [x] **RD-20** `MAJOR` A failed WHERE/ORDER BY leaves the bar showing the rejected text while the grid still shows the previous filter
- [x] **RD-23** `MAJOR` Inspect Value shows Rust debug text for integers and decimals (`I64(198)`, `Decimal("4477.50")`)
- [x] **RD-26** `MAJOR` Review Changes shows placeholders, not the data: `DELETE ... WHERE id = $n`, `INSERT ... VALUES ($1)`, and long statements are cut at the modal edge
- [x] **RD-27** `MAJOR` Review Changes dialog has no buttons or key hints; Enter applies immediately, and the dialog stays open afterwards
- [x] **RD-28** `MAJOR` Closing a table-data tab with pending changes (Ctrl+W) drops them without asking
- [x] **RD-31** `MAJOR` There is no way to edit an existing cell's value
- [x] **RD-38** `MAJOR` A table opened from one connection's tree can be bound to a different connection (safety guards of the wrong connection apply)
- [x] **RD-39** `MAJOR` Insert on a table without key or on a view: the form opens, accepts input, closes, then says `table is read-only`; nothing is queued
- [x] **RD-42** `MAJOR` After switching to another tab and back, Delete says "this table has no primary key" on a table that has one (and Insert says "table is read-only")
- [x] **RD-43** `MAJOR` A 40-column table is unreadable in the grid at 120 columns, and its record view cannot be scrolled
- [x] **RD-44** `MAJOR` NULL and the text `NULL` look identical; empty string and a single space look identical
- [x] **RD-45** `MAJOR` Inspect Value shows only the first line, cut at the modal border: long text cannot be read
- [x] **RD-51** `MAJOR` After closing a document the status bar and the actions keep the closed tab's connection: the Insert form is empty and Delete says "no primary key"
- [x] **RD-52** `MAJOR` "Copy as CSV/JSON/Markdown/SQL/Text" from the palette copies only the cursor cell; the same names in the Enter menu copy the whole row
- [x] **RD-02** `MINOR` Copy as Text is space-separated; no tab-separated copy is reachable
- [x] **RD-03** `MINOR` Copy as JSON reorders keys alphabetically
- [x] **RD-04** `MINOR` Copy as SQL uses the placeholder table name `tbl`
- [x] **RD-05** `MINOR` Copy as Markdown does not escape a newline or a `|` inside a cell
- [x] **RD-06** `MINOR` "copied to clipboard" toast does not say what was copied
- [x] **RD-09** `MINOR` Row actions menu does not offer "Copy as Text"
- [x] **RD-12** `MINOR` Status bar and keybindings help advertise `n/p page` on results that cannot page (and `p` on the first page, `n` on the last, are silent)
- [x] **RD-14** `MINOR` Page indicator is a raw offset+limit dump: `page:100+100 more`, `page:200+100`
- [x] **RD-15** `MINOR` Stale `page:200+100` label stays on later, unrelated results
- [x] **RD-16** `MINOR` Palette shows `\x` as the hotkey of Toggle Record View, but typing `\x` in the grid does nothing
- [x] **RD-17** `MINOR` Record view (one field per line): no field cursor, Left/Right do nothing, "Copy cell" copies an invisible column
- [x] **RD-18** `MINOR` After sorting (`s`, `S`, header click) the cursor resets to the first row and first column, so pressing `s` again sorts a different column
- [x] **RD-21** `MINOR` Filter/sort errors expose the internal wrapper query (`_dexo_derived`) and its column offsets
- [x] **RD-22** `MINOR` Messages tab and "Cycle Output View" order
- [x] **RD-24** `MINOR` Pane hotkeys do not match their names in a table-data tab: Alt+3 "Focus Results" focuses the Console, Alt+2 "Focus Editor" focuses Results
- [x] **RD-29** `MINOR` Pending changes are almost invisible in the grid
- [x] **RD-30** `MINOR` Review Changes with nothing pending opens an empty modal; Apply/Revert in the palette say "no pending changes" but Ctrl+S does not
- [x] **RD-32** `MINOR` Insert form gives no type, default or nullability hints and does not validate
- [x] **RD-33** `MINOR` Ctrl+N does nothing in a table-data grid; the hotkey the command list gives for Insert Row is not the real one
- [x] **RD-35** `MINOR` Messages tab opens at the oldest message and the newest ones are off screen
- [=] AT-33 **RD-36** `MINOR` Export dialog (`e` in table data) shows raw `key=value` text
- [x] **RD-40** `MINOR` Read-only connection accepts staged changes and says `ready`; only Apply refuses
- [x] **RD-41** `MINOR` Review lists composite-key deletes as invalid SQL, and error toasts are cut at the edge
- [x] **RD-46** `MINOR` Row/column "select" commands give no visible feedback
- [x] **RD-47** `MINOR` Ctrl+C in the grid does nothing
- [x] **RD-48** `MINOR` After over-shooting Alt+Up/Alt+Down on the Results pane, the grid cursor leaves the screen and the view stops following it
- [x] **RD-07** `COSMETIC` A newline inside a cell shifts the rest of that row one column to the left
- [x] **RD-08** `COSMETIC` Results actions menu truncates "Add this column to the sort"
- [x] **RD-19** `COSMETIC` The ORDER BY bar is only ~34 columns wide and clips the start of the text
- [x] **RD-25** `COSMETIC` "1 rows retrieved" in the Console and FK navigation title uses lowercase `where`
- [x] **RD-34** `COSMETIC` Table title after switching tabs loses the paging info
- [=] SL-16 **RD-49** `COSMETIC` Status bar at 60x20 reorders and lower-cases hints; sidebar hides itself and does not come back
- [x] **RD-50** `COSMETIC` Empty result shows only the header and no "0 rows" text; Esc in the grid does not clear a multi-row selection
- [x] **RD-53** `COSMETIC` Palette `New Document` asks for a name; Ctrl+N does not

## ES: Explorer, schema tools, explain

- [x] **ES-31** `BLOCKER` Production: the Schema form (Preview DDL > Apply) creates the table without asking for the connection's name
- [ ] **ES-04** `MAJOR` Postgres table DDL (Open Object DDL / Copy DDL) leaves out PK, NOT NULL, DEFAULT, UNIQUE, FKs, CHECK, indexes and comments
- [ ] **ES-09** `MAJOR` Inspect Object offers itself on constraints, functions, types, sequences and group nodes and answers "Select an object in Explorer."
- [ ] **ES-10** `MAJOR` Copy Object Name on a schema / database returns a doubled name
- [ ] **ES-11** `MAJOR` "Show Dependencies" is just the Inspect dialog with raw catalog ids
- [ ] **ES-15** `MAJOR` Manage Grants (Security panel): the "DDL preview" opens underneath the panel and cannot be read; Apply answers "ddl RolledBack"
- [ ] **ES-16** `MAJOR` Security panel is a 40-column box that truncates every grant and has no hints
- [ ] **ES-17** `MAJOR` Refresh Catalog (all) and `r` on a connection / group node do not refresh anything; no feedback either way
- [ ] **ES-20** `MAJOR` Hotkey `n` of "Edit Object Note…" does not work from the explorer: it opens "Add connection"
- [ ] **ES-24** `MAJOR` Preview DDL form: `defaults`, `indexes`, `constraints` and `foreign_keys` fields are ignored by the preview and by Apply
- [ ] **ES-25** `MAJOR` Preview DDL form: the columns field is a hidden mini-language; commas inside `numeric(10,2)` break the DDL, and unknown words are dropped silently
- [ ] **ES-26** `MAJOR` Applying DDL answers `ddl Committed` / `ddl RolledBack` (Rust enum names); a failure never says why
- [ ] **ES-32** `MAJOR` Apply Raw DDL dialog: raw `key=value` dump, leftover form values shown as removed lines, and ADD COLUMN labelled destructive
- [ ] **ES-33** `MAJOR` DDL preview cannot be scrolled: long DDL is cut with "…" and still offers [Apply]
- [ ] **ES-34** `MAJOR` After a restart, Preview DDL ran for the explorer's last connection (MySQL) while the visible document belonged to pg-readonly
- [ ] **ES-38** `MAJOR` Compare Schema cannot compare two different databases or a snapshot: it only ever diffs the current connection with itself
- [ ] **ES-39** `MAJOR` Schema diff dialog is an unlabelled raw dump with hidden keys and no buttons
- [x] **ES-41** `MAJOR` Explain Analyze of a write on a production connection asks no name, only "This is a production connection." with [Run] focused
- [ ] **ES-01** `MINOR` After the welcome dialog, focus is in the empty editor, not the explorer
- [ ] **ES-02** `MINOR` Explorer labels are cut at the pane edge with no ellipsis at the default width
- [ ] **ES-03** `MINOR` Inspect Object on a column shows internal ids and almost no column facts
- [ ] **ES-05** `MINOR` Tree: Left/Right/Space do nothing; clicking the disclosure arrow only selects; a double click is needed
- [ ] **ES-08** `MINOR` Palette fuzzy search: "favor" lists unrelated commands above the exact matches
- [ ] **ES-12** `MINOR` Inspect Object shows only 4 of the 7 table privileges and no owner / comment / size / columns / keys / indexes
- [ ] **ES-13** `MINOR` Single click on a connection row connects/toggles it, on every other node it only selects
- [ ] **ES-14** `MINOR` Actions menu on a connection has 18 entries and no keys for most of them
- [ ] **ES-18** `MINOR` Toggle System Objects gives no sign of its state, and turning it on is only half-applied
- [ ] **ES-19** `MINOR` Show Favorites Only: header is a raw filter string, and there is no way back except the palette
- [ ] **ES-21** `MINOR` MySQL tree shows two rows `mysql.users [restricted]` and `mysql.roles [restricted]` with no explanation
- [ ] **ES-22** `MINOR` Index / constraint names in Inspect have no table: `qa4.PRIMARY`, `qa4.customer_id`, `qa4.public.order_items_pkey`
- [ ] **ES-23** `MINOR` DDL dialog cuts long lines and cannot scroll sideways
- [ ] **ES-27** `MINOR` The Schema form opens prefilled with the name of an existing table (`public.orders`) and Apply happily tries to create it
- [ ] **ES-28** `MINOR` DDL preview: `risk: destructive=false lock=None` is a debug dump
- [ ] **ES-29** `MINOR` Schema form text fields do not scroll to the cursor and cut the text at the border
- [ ] **ES-30** `MINOR` Esc / Cancel in the DDL preview closes the whole form, there is no way back to edit
- [ ] **ES-35** `MINOR` pg-readonly: Apply refuses only after the preview; the refusal leaves the preview open
- [ ] **ES-36** `MINOR` There is no UI to alter a table (or create a view / routine / trigger / index): only the CREATE TABLE form is reachable
- [ ] **ES-37** `MINOR` After resizing 100x12 back to 120x36 the explorer and results panes stay hidden
- [ ] **ES-40** `MINOR` Editing a connected connection keeps the old session: `pg-b` (database changed to qa4b) kept showing `qa4` until Disconnect
- [ ] **ES-42** `MINOR` `:id` named parameter: F7 gives the server's `syntax error at or near ":"`; Analyze asks for confirmation first and then gives the same error
- [ ] **ES-43** `MINOR` Explain error toasts are wider than the screen and are cut at the right edge
- [ ] **ES-44** `MINOR` Explain: plan comparison says "now" without naming what it replaced; internal names leak into the node text
- [ ] **ES-45** `MINOR` Try an index: the dialog is offered everywhere and only refuses after you type the index
- [ ] **ES-46** `MINOR` Manage Grants panel is transparent and empty on MySQL; unsupported tools are offered in the menu of SQLite / DuckDB
- [ ] **ES-47** `MINOR` Tree keys: Home, End, PageUp, PageDown do nothing; Show Favorites Only is empty after a restart until the tree is expanded
- [ ] **ES-06** `COSMETIC` Object actions menu: half of the actions show no hotkey
- [ ] **ES-07** `COSMETIC` Copy toasts do not say what was copied

## CP: Connections and projects

- [x] **CP-01** `MAJOR` "[offline]" tags appear on every connection after Shift+D and stay, even on connected ones
- [x] **CP-02** `MAJOR` Changing the driver between server drivers does not change the port (MySQL on 5432, PostgreSQL on 3306)
- [x] **CP-03** `MAJOR` "Add connection": empty submit says only "password is required"; the error stays after the driver changes to one with no password
- [x] **CP-04** `MAJOR` The form lets you save a custom environment that then cannot connect; the error is internal jargon with no way out
- [x] **CP-05** `MAJOR` Submit saves the connection but the dialog stays open when the automatic connect fails; the next Submit says "already exists"
- [x] **CP-07** `MAJOR` tls_mode is a free-text field: any value is saved, a wrong one fails only at connect time with a serde error, and Postgres ignores the setting
- [x] **CP-08** `MAJOR` Typing a password in "Edit connection" says "saved" but the password is not stored
- [x] **CP-09** `MAJOR` Duplicate Connection silently drops the password
- [x] **CP-11** `MAJOR` No way to test a connection before saving; Submit always saves, and a failed connect leaves a broken saved connection and an open dialog
- [x] **CP-12** `MAJOR` "Unreachable host" error has no cause and no address
- [x] **CP-13** `MAJOR` Validation errors in the connection form are invisible while the Advanced section is scrolled
- [x] **CP-14** `MAJOR` pre_connect: the one error that explains it is cut off; the command must stay in the foreground
- [x] **CP-15** `MAJOR` The driver field of the connection form cannot be focused or changed with the mouse
- [x] **CP-17** `MAJOR` Config transfer import: the conflict list is clipped, shows Rust Debug text, has no key hints, and the import happens silently with a stale sidebar
- [x] **CP-20** `MAJOR` Config transfer dialog keeps the previous import's conflict list when reopened, and an export over an existing file overwrites it without asking
- [x] **CP-21** `MAJOR` Switch Project with an unsaved document: no confirmation prompt, raw enum text, a hidden key
- [x] **CP-22** `MAJOR` A document left unsaved across a project switch comes back with an internal id as its name
- [x] **CP-23** `MAJOR` Switching project disconnects every connection without saying so, and marks every row "[offline]"
- [x] **CP-24** `MAJOR` Deleting the active project leaves the app in a project-less state; its document stays on screen
- [x] **CP-25** `MAJOR` Delete Project confirmation: `foo=bar` dump, clipped text, an input with no field or buttons, jargon
- [x] **CP-26** `MAJOR` Projects dialog: the `create:` field is after the buttons in the Tab order and has no focus marker; the rename/create dialogs keep stale hint lines
- [x] **CP-27** `MAJOR` Renaming the active project does not update the header or the recent list
- [x] **CP-29** `MAJOR` After deleting the active project no project can be switched to until Dexo is restarted
- [x] **CP-30** `MAJOR` After a restart an unsaved document is restored as if it were saved: no `*`, and closing it throws the text away without asking
- [x] **CP-32** `MAJOR` SSH tunnel: `missing secret for ssh_password` and the form has no way to supply it
- [x] **CP-35** `MAJOR` Import hides its own safety warning: "need secret" and "runs on this machine when it connects -- read before applying" are clipped out of the dialog
- [x] **CP-37** `MAJOR` Sidebar actions menu runs on the active session, not on the connection it is opened for, and does not connect the selected one
- [x] **CP-06** `MINOR` Enter in a text field submits the whole form; nothing in the dialog says so, and the long form has no shortcut to Submit
- [x] **CP-10** `MINOR` The password prompt at connect time: "save to the keychain" checkbox is not in the Tab order; a rejected password is stored anyway; a wrong password closes the prompt
- [x] **CP-18** `MINOR` Export result is just "ok"; the path is cut off at the border
- [x] **CP-19** `MINOR` The export/import file picker opens in the process's working directory and has no "up to home / jump to path" key besides Backspace/Left
- [x] **CP-28** `MINOR` Projects list: which project is active is not shown; single click only selects, Enter/double-click switches; no hint line
- [x] **CP-31** `MINOR` After a restart the header and status bar show another connection than the visible document's
- [x] **CP-33** `MINOR` Proxy failures name neither the proxy nor the cause
- [x] **CP-34** `MINOR` Connection groups are never shown in the sidebar
- [x] **CP-36** `MINOR` Find Databases in Docker hides containers that already have a saved connection, without saying so
- [x] **CP-38** `MINOR` Deleting a connection leaves its documents open and unbound; running one silently rebinds it to whatever connection is active
- [x] **CP-39** `MINOR` Startup: focus is in the editor, so the first `n` typed (as the Welcome text says) creates a document instead of opening New Connection
- [x] **CP-40** `MINOR` URL / CLI temporary connections
- [x] **CP-16** `COSMETIC` The buttons jump one row down when the error line appears; a click made where the button was lands on the error text
- [ ] **CP-41** `COSMETIC` Layout details

## AT: Transactions, sessions, import, export, backup

- [ ] **AT-28** `BLOCKER` Inspect Sessions on a connection whose session is busy/blocked freezes the whole UI for up to a minute and then shows nothing
- [ ] **AT-41** `BLOCKER` Import Data always writes into a table named `tbl`; there is no way to choose the target table
- [ ] **AT-42** `BLOCKER` Dexo cannot import its own TSV export (tab delimiter is not applied)
- [x] **AT-50** `BLOCKER` Production guard is skipped by Import Data and Native Restore: no connection name is asked
- [ ] **AT-51** `BLOCKER` Native Restore (and Backup) freeze the whole UI for the full duration; Cancel and Esc do nothing and the process is not stopped
- [ ] **AT-53** `BLOCKER` Dexo's own backup cannot be restored by Dexo's own restore
- [ ] **AT-07** `MAJOR` Transaction commands on an offline connection refuse instead of connecting
- [ ] **AT-10** `MAJOR` MySQL UPDATE/DELETE gives no feedback (Results pane stays empty), while Postgres says "1 row affected"
- [ ] **AT-16** `MAJOR` After a session is terminated from the Sessions dialog, the document's connection stays broken: every run says "connection closed" and nothing reconnects it
- [ ] **AT-20** `MAJOR` Sessions list is a stale snapshot while the connection has an open transaction; `r` refresh changes nothing
- [ ] **AT-22** `MAJOR` A running (or blocked) query shows nothing: no "running" state, no elapsed time, no hint that Ctrl+F2 cancels
- [ ] **AT-24** `MAJOR` A statement's result is thrown away if it finishes while another document tab is active
- [ ] **AT-31** `MAJOR` Export overwrites an existing file without asking
- [ ] **AT-32** `MAJOR` Choosing a file in the picker starts the export; Submit re-runs it onto the same file
- [ ] **AT-33** `MAJOR` Dialog shows raw debug text instead of labelled fields
- [ ] **AT-34** `MAJOR` SQL export names the INSERT target after the FILE name, not the table
- [ ] **AT-35** `MAJOR` Export silently stops at the 10,000 loaded rows
- [ ] **AT-43** `MAJOR` SQL is offered as an import format (initial value) and the on-error strategy cannot be changed
- [ ] **AT-44** `MAJOR` Import errors name no file line, row or column; empty cells cannot be NULL
- [ ] **AT-45** `MAJOR` Sessions list does not scroll: the selection moves onto rows that are not visible, and `t` then targets an invisible session
- [ ] **AT-52** `MAJOR` Native Restore reports `error: status=Failed pg_restore --no-password --host ...` even though the data was restored; the real error is hidden
- [ ] **AT-54** `MAJOR` MySQL Native Backup / Native Restore hang forever with `running=true` and say nothing about mysqldump
- [ ] **AT-55** `MAJOR` Backup/Restore reuse the export dialog: irrelevant `format=` / `strategy=` / `rows=` fields, stale state, and a confirmation that carries over
- [ ] **AT-01** `MINOR` Transaction commands succeed silently: no toast, no Messages entry
- [ ] **AT-02** `MINOR` Status bar shows a raw `tx:active` and never changes for savepoints or an aborted transaction
- [ ] **AT-03** `MINOR` Error in an open transaction does not say what to do
- [ ] **AT-04** `MINOR` Transaction commands with no transaction: warning toast, but the palette stays open (and the editor loses focus)
- [ ] **AT-06** `MINOR` "Begin Transaction" twice says "session is not idle"
- [ ] **AT-08** `MINOR` Header and status bar keep the previous connection after Alt+Left / Alt+Right switch the document
- [ ] **AT-09** `MINOR` Sidebar connect while a document of another connection is on screen: header/status name the new connection but `tx:active` belongs to the document's
- [ ] **AT-11** `MINOR` Document tab truncates the connection name to 7 characters
- [ ] **AT-12** `MINOR` Error toasts never go away on their own and survive later successful actions
- [ ] **AT-14** `MINOR` Nothing on screen says a connection is read-only until a write is refused
- [ ] **AT-15** `MINOR` Destructive-statement guard is bypassed by a tautological WHERE
- [ ] **AT-17** `MINOR` Terminate success message is "signal sent"
- [ ] **AT-18** `MINOR` Sessions list ignores Home/End/PageUp/PageDown and the mouse wheel; ids sort as text
- [ ] **AT-19** `MINOR` Sessions list shows every database on the server and does not mark the user's own sessions
- [ ] **AT-21** `MINOR` Blocking line is cryptic: `419 blocks 764 · ShareLock on -`
- [ ] **AT-23** `MINOR` Sessions: `t` on a read-only connection is refused only after the dialog is already open; fine message, but the dialog is a full admin view
- [ ] **AT-25** `MINOR` User-pressed Cancel Query (Ctrl+F2) is reported as an error toast
- [x] **AT-27** `MINOR` `select pg_sleep(...)` on production asks for the name ("not a read-only statement"), `select now()` does not
- [ ] **AT-29** `MINOR` Ctrl+A in a brand-new empty document, then typing, drops the first character
- [ ] **AT-30** `MINOR` Statements Dexo cannot parse are listed as "Dexo could not read this statement"
- [ ] **AT-36** `MINOR` File extension and format are independent: `.sql` file with JSONL inside, `.csv` re-exported as another format
- [ ] **AT-37** `MINOR` Exporting into a directory that does not exist shows a raw OS error and keeps the previous success line
- [ ] **AT-38** `MINOR` JSON/JSONL export loses column order, writes jsonb/numeric as strings
- [ ] **AT-39** `MINOR` The Results hint line does not list `e` (Export) although `e` opens it
- [ ] **AT-40** `MINOR` Transfer keys are not in the keybindings help
- [ ] **AT-46** `MINOR` Import of a file that does not exist: `error: No such file or directory (os error 2)` without the file name
- [ ] **AT-48** `MINOR` Rollback on a non-transactional table (MySQL MyISAM) says nothing and keeps the rows
- [ ] **AT-49** `MINOR` Sessions on MySQL: rows in no clear order, different state vocabulary
- [ ] **AT-56** `MINOR` SQLite Native Restore shows the backup text: `a SQLite database is its file: copy the file to back it up`
- [ ] **AT-57** `MINOR` Backup/Restore/Import/Export hotkeys: none
- [ ] **AT-58** `MINOR` After shrinking the terminal to 60x20 and growing back, the sidebar and Results pane are not drawn until focus moves
- [ ] **AT-05** `COSMETIC` Savepoint dialog: one title for three actions, lowercase action line, a lot of empty space
- [ ] **AT-13** `COSMETIC` Long refusal toasts are cut off mid-word without an ellipsis
- [ ] **AT-26** `COSMETIC` `pg_sleep()` (void) shows as `\x` in the grid
- [ ] **AT-47** `COSMETIC` SQLite import errors are driver text: `UNIQUE constraint failed: tbl.id`, `datatype mismatch`

## MA: MCP and agents

- [x] **MA-02** `MAJOR` `g` (New MCP Grant) in the empty MCP Profiles screen opens a form that cannot succeed
- [x] **MA-05** `MAJOR` MCP Profiles shows raw debug lines (`profile NAME enabled=false`, `mcp profile=... enabled=false confirm=true`)
- [x] **MA-11** `MAJOR` MCP Profiles popup is fixed-height: with a few grants the status line and the hint line are cut off, so a pending confirmation is invisible
- [x] **MA-22** `MAJOR` New MCP Grant from the palette silently targets the first profile; the form cannot choose a profile
- [x] **MA-25** `MAJOR` Revoking grants denies the waiting request with the reason "a person denied this write"
- [x] **MA-26** `MAJOR` Agent Activity: with nothing waiting, PgDn scrolls for half a second and snaps back; only the newest 20 events can ever be seen
- [x] **MA-34** `MAJOR` MCP Profiles: with more than 11 profiles the selection scrolls out of sight, and `e`/`r` act on rows you cannot see
- [ ] **MA-41** `MAJOR` (layout, found while testing popups) After the terminal is made 20 rows high and back to 120x36, the Sidebar and the Results pane stay hidden
- [x] **MA-01** `MINOR` MCP Profiles with no profile: empty state gives no way forward
- [ ] **MA-03** `MINOR` Palette shows hotkey `g` for "New MCP Grant…" but it only works inside MCP Profiles; help does not list it
- [x] **MA-04** `MINOR` Agent Activity `r revoke all grants` closes Activity and opens the MCP Profiles screen with an unexplained pending "confirm revoke all grants"
- [x] **MA-07** `MINOR` Inconsistent revoke-all keys between screens
- [x] **MA-08** `MINOR` CLI: `dexo mcp profile create` accepts any name, including `bad name!`, and there is no way to delete a profile
- [x] **MA-09** `MINOR` CLI: creating a profile with an existing name shows the raw SQLite error
- [x] **MA-10** `MINOR` CLI: `profile show`/`policy` print Rust Debug names; empty answers are silent
- [x] **MA-12** `MINOR` MCP Profiles: key `r` is labelled "revoke" but revokes ALL grants of the selected profile (after a silent first press)
- [x] **MA-13** `MINOR` MCP Profiles detail uses internal words and units: `diff pg-dev allow qa7.public.orders`, `grant data_write data_update 1796s asks (120s)`
- [x] **MA-14** `MINOR` MCP Profiles: status line is sticky across openings and stale
- [x] **MA-15** `MINOR` MCP Profiles: list is a snapshot, it does not follow changes made by `dexo mcp` while it is open
- [x] **MA-16** `MINOR` Enabling in MCP Profiles needs a second `e`, but the prompt does not say so; selection resets after actions
- [x] **MA-17** `MINOR` New MCP Grant: first focus is `tools:`, not `connection:`; the connection is not prefilled even when the profile has one connection
- [x] **MA-18** `MINOR` New MCP Grant: clicking the "ask before each write" checkbox only moves focus, it does not toggle it
- [x] **MA-19** `MINOR` New MCP Grant: validation messages are terse, stale or misleading
- [ ] **MA-21** `MINOR` Form allows several tools in one grant, the TUI success message names only the tool and selector
- [x] **MA-23** `MINOR` Ctrl+P (palette) is swallowed inside MCP Profiles; typed text acts as hotkeys there
- [x] **MA-24** `MINOR` "Revoke All MCP Grants" (palette) only opens MCP Profiles with a pending confirmation, shows a stale snapshot, and says `1 grants`
- [x] **MA-27** `MINOR` Agent Activity: mouse is not supported
- [x] **MA-28** `MINOR` Agent Activity rows are raw, contradictory and carry no time
- [x] **MA-29** `MINOR` Agent Activity: waiting requests - only the selected one shows its SQL, the confirm line does not repeat it
- [ ] **MA-30** `MINOR` The "waiting" notice is a short toast titled "warn"; nothing persistent shows pending requests
- [x] **MA-31** `MINOR` A killed agent: the request lingers as "waiting" and a late approval says "Approved: the agent's write runs now"
- [-] **MA-32** `MINOR` Timeout while the confirm dialog is open: the dialog just disappears (not a defect: the vanished request is announced by a warning toast and the Recent list says it timed out)
- [x] **MA-35** `MINOR` The waiting request for a destructive DDL without `confirm_target` is queued for a person, who approves it for nothing
- [x] **MA-36** `MINOR` Agent Activity: an admin request (`admin_terminate_session`) says nothing about what it would terminate
- [x] **MA-37** `MINOR` `--expires`/`expires:` accepts `15m`, `2h` and a bare number (seconds), but not `12s`, `90s`, `1d`, `1h30m`; the error advertises `1s`
- [x] **MA-38** `MINOR` CLI messages and output of `dexo mcp grant` are raw
- [x] **MA-39** `MINOR` Agent side: raw Rust/serde and Debug text in errors and results
- [x] **MA-40** `MINOR` `list_connections` and the tool list disagree after a connection becomes production; the refusal reason is TLS, not the production rule
- [x] **MA-42** `MINOR` Object inspector (`i`, `n` note) shows internal ids
- [x] **MA-06** `COSMETIC` MCP Profiles selected row has no highlight
- [x] **MA-20** `COSMETIC` New MCP Grant: the checkbox label describes the unchecked state as a feature
- [-] **MA-33** `COSMETIC` Agent Activity: popup draws over the SQL pane border at 120x36 (`┌▸ SQL────┌Agent activity───┐─────────┐`) (not a defect: a popup overlays the panes beneath it; it draws nothing of its own over their borders)

## SL: Settings, layout, mouse

- [x] **SL-05** `MAJOR` "MOUSE OFF · Ctrl+P settings.mouse" shows an internal command id and points to a palette entry that does not exist
- [x] **SL-06** `MAJOR` Cycle Theme / Toggle Light-Dark Mode / Cycle Accent / Cycle Keymap / Toggle Mouse / Toggle Animation / Toggle Unicode / Reset Settings and Hide Explorer / Hide Results / Grow-Shrink Results / Grow-Shrink Explorer are not in the command palette
- [x] **SL-07** `MAJOR` Alt+[ (Shrink Explorer Pane) does nothing and swallows the next key
- [x] **SL-16** `MAJOR` Running Dexo in a small terminal permanently hides the explorer and results (compact mode overwrites the saved layout)
- [=] RD-23 **SL-17** `MAJOR` "Inspect value" shows raw Rust Debug text for numbers and booleans
- [x] **SL-01** `MINOR` Clicking an option in Settings cycles to the next value instead of choosing the clicked one
- [x] **SL-02** `MINOR` Empty-editor hint `Ctrl+N  new query / Ctrl+O  open a file` is hard-coded and wrong under the Emacs keymap
- [ ] **SL-03** `MINOR` Unicode = Off still draws non-ASCII glyphs
- [x] **SL-08** `MINOR` Alt+= / Alt+- mean different things depending on focus, but the hotkey is shown only as "Grow/Shrink Results Pane"
- [x] **SL-09** `MINOR` Panes can be shrunk until they are useless
- [x] **SL-10** `MINOR` Only one of the two border cells of a divider is draggable
- [x] **SL-18** `MINOR` "Toggle Light/Dark Mode" cycles through three modes
- [ ] **SL-19** `MINOR` The terminal's colour depth (TERM / COLORTERM) is ignored; only NO_COLOR works
- [x] **SL-20** `MINOR` Compact mode (< 80x24) shows one pane and the mouse cannot switch panes
- [x] **SL-21** `MINOR` Help: any click closes the overlay, even a click on the Search field
- [x] **SL-22** `MINOR` "Run destructive statements" is the title for a statement Dexo merely cannot parse, and it has no warning styling
- [ ] **SL-23** `MINOR` "Search History" is not searchable and shows duplicates
- [ ] **SL-24** `MINOR` Key hints behind and inside modals are inconsistent
- [ ] **SL-04** `COSMETIC` Modals sit at different heights and have different sizes
- [x] **SL-11** `COSMETIC` Layout and settings commands give no feedback about what they did
- [x] **SL-12** `COSMETIC` Command name case differs: "Reset layout" vs "Cycle Layout" / "Reset Settings"
- [x] **SL-13** `COSMETIC` Settings footer does not list `e` (cycle theme) which the docs describe
- [x] **SL-14** `COSMETIC` Explain placeholder is cut off at the pane edge instead of wrapped
- [ ] **SL-15** `COSMETIC` Right click: grid and tree respond, editor, tabs and status bar do not
- [ ] **SL-25** `COSMETIC` Button rows differ between dialogs
- [ ] **SL-26** `COSMETIC` Muted text and Light-mode accents have low contrast
- [ ] **SL-27** `COSMETIC` Compact status bar, 40x12 cut-offs
- [x] **SL-28** `COSMETIC` F10 cycles four unnamed layouts and one of them leaves 3 inner rows for results
