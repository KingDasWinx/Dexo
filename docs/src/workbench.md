# Workbench

The TUI is keyboard-first: explorer, SQL editor, results, inspector, and a command palette. Layouts persist per project. Compact mode hides extra panes on small terminals; below 20x8 Dexo says the terminal is too small instead of drawing pieces of it.

SQL execution streams result pages. Manual transactions stay visible. Closing a tab does not silently abandon a running query, and quitting with an open transaction or grid edits not yet applied says what would be lost and asks first.

Theme, keymap, mouse capture, Unicode, and animation persist in a local settings file and apply immediately. Mouse clicks map to the same commands as the keyboard.

Every single-line field -- the palette, the F1 search, the WHERE and ORDER BY bars, form fields, names and paths, the typed confirmations -- edits the same way: Ctrl+A selects the whole text, shown in reverse, and what you type next replaces it; Ctrl+Left and Ctrl+Right move by words, accents included; Ctrl+Backspace, Alt+Backspace or Ctrl+W delete the word before the cursor and Ctrl+Delete the one after it; Home and End go to either end. Deleting a project asks for its name; Alt+C there chooses whether its connections go with it.

## Screens

The workbench -- explorer, editor and results -- is one of six screens. The others are places for work that outgrew a dialog: **Connections**, **Agents**, **Server**, **Compare** and **History**. The top line lists them, the current one in brackets, beside the project, connection and schema; a number after a name is work waiting there, such as agent writes waiting for approval.

`Ctrl+G` then a letter goes to a screen: `w` workbench, `c` connections, `a` agents, `s` server, `d` compare, `h` history; `Ctrl+G Ctrl+G` goes back and forth between the last two. After `Ctrl+G` a small list shows what can follow, as it does for any chord that waits for a second key. A click on a name and the palette's "Go to …" commands go there too, and the keys and commands that opened the old dialogs -- `s` in the sidebar, Search History, MCP Profiles -- open their screen. Esc first clears what the screen has open, a search or a form, then goes back to the screen you came from. A screen keeps its state while Dexo runs; Dexo starts on the workbench. On another screen, a key that acts on the workbench (Ctrl+W, Ctrl+S) says so instead of acting on what is hidden; the palette's workbench commands go to the workbench and run there.

A screen's actions are buttons. Those for the picked item sit over its detail; those for the whole screen sit on the row under the top line, beside its views, its search and its filters. A button shows its key -- `[e Edit]` -- and a click and the key do the same; from the detail, Left and Right walk the buttons and Enter presses the one marked `>`. A button that cannot act now is dimmed, and pressing it says why. `/` searches the list: lower case matches either case, a capital makes the case count. A filter shows what it is set to, `Env: prod`, and is lit when it is not at its default; Esc clears the search, then the filters, then goes back. Details are fields under headings, and SQL in them has the editor's colours. Under eighty columns a screen shows its list or its detail, not both: Enter shows the pick's detail, Esc goes back to the list. A toolbar button that does not fit is named on the status line.

## Keys

A key does one thing where you press it. The palette (Ctrl+P), help (F1), quitting (Ctrl+Q) and the screens (Ctrl+G) work everywhere. On the workbench, what acts on the document or the connection works from any pane: new, open, save and close a document (Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+W; a new one is for the connection in view, and Up then Left/Right in its dialog picks another), the next and previous one (Ctrl+Tab, Alt+Left and Alt+Right), the panes (Alt+0 to Alt+3), the transactions (Alt+B, Alt+C, Alt+Z), cancelling a run (Ctrl+F2), explaining (F7). The rest belongs to the pane: F2 renames the document in the editor and on the tab strip, edits the cell under the cursor in a table's rows, and does nothing elsewhere; in the explorer Alt+Left and Alt+Right size the explorer instead; on a table's Structure, DDL or Privileges, a plan or the log, the grid's keys say they work on the grid view. Each other screen has its own letters, shown on its status line, and its panes take the workbench's pane keys: Alt+1 goes to its list, Alt+2 to the detail beside it, where Up/Down, PgUp/PgDn, Home and End read the detail while the letters still act on the pick. A click in a pane does the same. A form or a question open in a pane keeps the keys until it is answered or closed. F1 lists every key by where it works.

## Themes

Settings' Theme row (`e`, or "Cycle Theme" in a keymap) steps through Dexo's own theme, five presets -- Dracula, Gruvbox, Nord, Catppuccin and Tokyo Night -- and your own files, applying each as it is shown. Mode and Accent belong to Dexo's own theme, and changing either goes back to it.

A theme of your own is a TOML file in `themes/` in the data directory, chosen by its file name:

```toml
name = "Harbor"
mode = "dark"          # dark, light or low-color: the base the roles override

[roles]
background = "#0f1720"
foreground = "#d8dee9"
focus = "#88c0d0"
selection = "ansi:24"  # #RRGGBB, ansi:<0-255>, or a color name such as blue
```

The roles are `background`, `foreground`, `border`, `muted`, `production`, `staging`, `development`, `error`, `warning`, `success`, `selection`, `focus`, `zebra`, `on-focus` and `on-selection`; any left out keep the mode's color. A file that does not parse is left out, and the messages say which file and which line; Settings reads the folder again each time it opens.

## Keymap overlay

`keymap.toml` in the data directory changes keys over the keymap chosen in Settings, in the same format the built-in keymaps use: a section per place -- `global`, `editor`, `explorer`, `results`, `console`, `tabs`, `palette`, `modal` -- and `chord = "command id"`, with `""` to unbind:

```toml
[editor]
"ctrl+r" = "query.execute_document"
"ctrl+/" = ""

[global]
"f9" = "connection.test"
```

Command ids are the palette's. A chord that starts a longer one where both apply is refused, since Dexo would wait for the rest and the shorter would never fire; unbind the longer one first. A file with a problem is not used: the messages say its line, and the keymap is used as it is. The palette shows the keys the overlay gives.

## SQL editor

| Key | Action |
| --- | --- |
| Ctrl+F / Ctrl+H | Find / find and replace in the document. Enter or F3 next, Shift+Enter previous, Alt+C case, Alt+W whole word, Alt+A replace all, Esc close. Where the terminal sends Ctrl+Backspace as Ctrl+H, Alt+R in the find bar opens the replace row. |
| Ctrl+O | Open a SQL file: the recent ones first, then the folder's folders and SQL files. Typing finds them, in this folder and those below it (not in `.git`, `node_modules`, `target` and the like); a path typed (`~/sql/`, `reports/q.sql`) goes there; Left or Backspace with nothing typed goes up a folder; Alt+H shows hidden files. The save, export and import dialogs work the same, typing the file's name instead. |
| Ctrl+/ | Comment the line or selection out with `--`, or back in |
| Ctrl+Shift+D | Duplicate the line or selection |
| Ctrl+Shift+Up / Down | Move the line or selection up or down |
| Ctrl+E | Edit the document in `$VISUAL` or `$EDITOR` (`ctrl+x ctrl+e` in the Emacs keymap) |

The editor underlines what is wrong as you type: a statement that does not parse, and, once the catalog has been read whole, a table or a `alias.column` the database does not have -- never one it simply has not loaded. With the cursor on an underline the status line says what it is. When a run fails and the server says where, that spot is underlined and the cursor goes to it.

A `:name` in a statement asks for its value before the run. Each statement goes to the server with the database's own placeholders (`$1` on Postgres and DuckDB, `?` on MySQL and MariaDB, `?1` on SQLite) and only the values it names.

Lines starting with a backslash are psql's commands, answered by Dexo from the catalog on every database and never sent to the server: `\dt`, `\dv`, `\di`, `\dn` and `\df` with an optional pattern (`*` and `?` wildcards), `\d name` for a table's columns, keys and indexes, `\l` for databases, `\x` for one field per line, and `\?` for the list.

### Vim mode

With the Vim keymap (Settings), the editor is modal: Normal, Insert, Visual (`v`) and Visual-line (`V`), with the mode on the status line and a block cursor outside Insert. Normal mode takes counts and the motions `h j k l w b e 0 ^ $ gg G`; the operators `d c y` with a motion, or doubled for lines (`dd yy cc`); `x p P u` and Ctrl+R; `i a I A o O` into Insert; `.` to repeat the last change; `/` to search, `n` and `N` for the next and previous match; `:s/old/new/[g]` on the line, `:%s` on the document; `:w`, `:q`, `:wq`, `:x` (which writes only what changed) and `:<line>`. The text objects `iw` and `aw` work after an operator (`diw`, `ciw`) and in Visual mode, where `X`, `D`, `C` and `Y` take whole lines. Every change, an Insert session included, is one undo step, and a count on `.` replaces the change's own. Ctrl chords -- the palette, running a statement, quitting -- work in every mode; Esc then a key typed fast is two keys, as in Vim, not an Alt chord.

## Results

Above a table's rows, or the result of a statement that only reads, two bars take SQL of your own: `w` focuses WHERE, `o` focuses ORDER BY, Enter runs the grid again with them, Esc puts back what last ran, Tab moves between them; a click focuses one. What they hold must be a condition and a list of sort keys -- one more statement, a locking read, a function with side effects or an executable comment is refused before anything is sent -- and the run happens where it cannot write: in a read-only transaction of its own, or inside your open transaction behind a savepoint undone after it. A run that fails puts the rows that were there back.

`s` sorts by the current column (Left and Right move it, the header marks it), `S` adds it to the sort; clicking a header sorts by it, ascending, then descending, then off, and Shift+click, Alt+click or a right click adds it. The sort is the ORDER BY bar's text, and the headers show its order. A result run again with the bars is a page: `n` and `p` turn it.

The title says how many rows there are: exactly when they all came, `~4.3M` from the server's statistics for a table's first page, `100+` when more may follow. `t` counts them exactly -- a table's on a connection of its own, a result's on its session -- and `t` again stops the count; the count stays while the grid shows the rows it counted.

The grid keeps every column at the width its values need and scrolls sideways: Left and Right move the current column, Home and End go to the first and last, Ctrl+Home and Ctrl+End to the first and last row. `x` shows one record at a time, a field to a line, with a cursor you move with Up and Down (Left and Right turn records); a record taller than the pane scrolls. NULL is drawn dim and slanted, apart from the text `NULL`, and an empty string as `""`. Enter on a cell, or Ctrl+C, copies: "Copy as ..." takes the row (or the rows selected), "Copy cell" the value; CSV is quoted as an export is, and an INSERT names the table the rows came from. Inspect Value reads a whole value, wrapped, and scrolls it.

`f` on a row opens Related rows: each foreign key from or to the table, followed in a document of its own filtered to the rows on the other end; `b` closes that document and goes back to the row. Enter on a row lists what can be done with it, with each action's key.

## Explain

F7 shows the estimated plan of the statement under the cursor, Shift+F7 runs it with ANALYZE after asking; `v` steps through the tree, the table and a summary, and a second plan of the same statement is compared with the first. On Postgres with the hypopg extension, `i` tries an index before anyone builds it: type its definition (`CREATE INDEX ON orders (customer_id)`) and the statement is planned as if it existed, compared with its plan without it. The index exists only for that plan, on that session; `dexo explain --index "CREATE INDEX ON …"` does the same from the command line. Without hypopg, Dexo says so: install the package on the server, then `CREATE EXTENSION hypopg`.

A statement with parameters (`$1`, `?`) has a plan that depends on their values. On Postgres 16 and later, F7 shows the plan the server would pick for any value (`EXPLAIN (GENERIC_PLAN)`); on older Postgres, MySQL, MariaDB, SQLite and DuckDB, and for Analyze, which runs the statement, Dexo says the plan needs the values: write them into the statement to explain it. Analyze rolls back what it ran (inside a transaction of your own, to a savepoint on Postgres, MySQL and MariaDB), though a sequence or an auto-increment counter keeps its advance. On MySQL and MariaDB it refuses a write that touches a table with no transactions (MyISAM, Aria, MEMORY), which no rollback would undo, or a table it cannot tell about.

## Table views and the object inspector

A table's or view's document has its own views beside its rows: **Structure** (columns, keys, indexes, owner, size, note), **DDL** and **Privileges**. `i`, `d` and `g` on a table in the explorer open its document on Structure, DDL and Privileges; `v` walks the views, and a click on one picks it. Privileges lists the roles, what the picked one may do on that table -- its grants elsewhere are counted, not listed -- and Enter previews a grant of SELECT to it.

For any other object, `i` opens the inspector: its properties, what the connected user may do with it, and, with `d` in the explorer, its DDL. Up and Down scroll it. `n` -- in the inspector or on a table's Structure view -- writes a note on the object -- what a table or a column means, for you and for agents over [MCP](mcp.md): Enter or [Save] keeps it, Esc or [Cancel] drops it, and a blank note removes it. The note shows once it is saved. A note belongs to a saved connection's object; without one, the database's own comment is shown, marked as such. Edit Object Note… in the palette does the same for the object selected in the explorer.

## Agents

Agents (`Ctrl+G a`, or `Ctrl+Alt+A`) has four views, switched with `1`-`4` or `[` and `]`:

- **Approvals** lists the writes MCP agents are waiting to make under grants that ask before each write, oldest first, each with what it would do. The picked one is shown as fields -- profile, tool, connection, target, when it was asked and the time it has left -- over its statement in colour. `[a Approve]` asks a second time -- Cancel holds the focus, so an Enter out of habit decides nothing -- and `[d Deny]` refuses; PgUp and PgDn read a long statement, the question open or not. A request decided elsewhere or out of time closes its confirmation and settles nothing, and a write is approved only while its agent still waits for the answer. `[R Revoke all grants]` revokes every grant, which denies their waiting requests too.
- **Activity** is the audit log as a table -- outcome (`✓` ok, `✗` failed, `⊘` refused), time, profile, tool, target, duration and rows -- newest first. `/` searches it, `p` keeps one profile's calls and `o` one outcome; the picked call is shown in full under it.
- **Profiles** lists the MCP profiles -- enabled or not, how many connections, whether they read SQL, their grants -- beside the picked one's fields and grants. `[e Enable]` or `[e Disable]`; `[c Connections…]` checks the connections it may use (Space checks, Enter saves, Esc keeps them as they were); `[q Read SQL on]` or `off` lets it run read-only SQL or not; `[g Grant…]` makes a grant in that pane, with an "ask before each write" switch; `[r Revoke grants]` revokes its grants and `[x Delete]` deletes it. `[n New]` makes one in Setup.
- **Setup** points an agent at Dexo: Claude Code, Codex, Cursor, Claude Desktop, Gemini CLI, Windsurf and VS Code are listed as set up and with which profile, found on this machine -- its command on PATH, or its config's folder there -- or not found. The picked one shows its status, the file Setup writes and whether it exists, and its skill file, over the form: a new profile -- its name, the saved Postgres and MySQL connections it may use, and whether it may run read-only SQL -- or one already made, and whether to write the skill file. Enter takes the form; what a row is for is on the status line. `[s Set up]` makes the profile, enables it, and merges Dexo's entry into the agent's file, the old file kept beside it, and lists what it did and what is left: restarting the agent. `[y Copy command]` copies the agent's own command for the same thing, for Claude Code, Codex and Gemini CLI. With no profile yet, Agents opens here, and the palette's Set Up an Agent (MCP)… comes here too.

A write that starts waiting while you are elsewhere puts its count beside Agents on the top line and says so once. Grants are also made with `dexo mcp grant create`; see [MCP](mcp.md).

## Server

Server (`Ctrl+G s`, or `s` in the sidebar) shows the server the connection in use reaches, on Postgres, MySQL and MariaDB, in five views switched with `1`-`5`: **Sessions**, **Locks**, **Sizes** (largest first), **Stats** and **Settings**. `[c Server: …]` moves to another open connection's server. Sessions and Locks are read again every two seconds while they are on screen; `[p Pause]` stops that and `[r Refresh]` reads now. `/` searches every view. A view the server will not show says why.

Sessions lists what is running: idle sessions are hidden until `a` shows them, the longest running first, and `s` sorts by another column, marked `▼`. A blocked session is marked `⊘`, and this Dexo's own -- told by the id the server gave it, not by a shared login -- says `you`. The picked one is shown whole under the list: its query in colour, who and where it is, and what it blocks or waits for. `[k Cancel query]` stops its query and leaves the session, after asking; `[t Terminate…]` ends it once its id is typed; neither acts on Dexo's own session or on a read-only connection, and production asks as it does for any write. `[y Copy query]` copies the query and `[o Open in editor]` opens it in a new document on that connection.

## Connections

Connections (`Ctrl+G c`) lists the saved connections as a table -- in use `◉`, connected `●` or not `○`, name, driver, environment and address -- those in no group first, then each group under its heading, which Left folds and Right unfolds; the databases running in Docker and not saved yet follow, marked `+`. `/` searches names, hosts, databases, groups and drivers; `o` keeps the connected ones and `v` one environment. The picked one is shown as fields: its address, database, user, where its password comes from, its group, its tunnels and its rules, the state of its session, and its last test.

Its buttons follow its state: `[⏎ Connect]` dials it, `[⏎ Use]` makes its session the one in use, and on the one in use `[⏎ Open SQL]` opens a new document on it. `[s New SQL]` opens a new document on it from any state, dialling it first; `[b Browse]` goes to the explorer on it once it is connected; `[c Disconnect]` closes its session; `[e Edit]`, `[d Duplicate]`, `[t Test]` -- shown live under its fields, then how long it took or why it failed -- `[y Copy URL]`, without the password, and `[x Delete]`, after asking. On a Docker database `[⏎ Add as connection]` fills a new connection from it. `r` looks for Docker again.

`[n New]` and `e` -- here or in the sidebar -- open the form in the detail pane; saving or cancelling a form opened from the sidebar goes back to the sidebar. `[u From URL]` opens it on its URL field: a URL pasted or typed there -- `postgres://user:password@host:5432/db` -- fills the rest, and is not kept, password included. In the form Enter goes to the next field, and on [Submit], [Test] or [Cancel] does what the button says; Left and Right pick a driver or another value from a list, and Space opens the advanced options.

## Compare

Compare (`Ctrl+G d`, or Compare Schema in the palette) compares two sources -- open connections, saved snapshots, a snapshot file -- and keeps the last comparison to read again. The sources stay on the row under the top line, `From ‹ pg-dev › ⇄ To ‹ pg-prod ›`: their arrows step them, `p` gives them the keys, `[s Swap]` exchanges them and compares again, and `[e Compare]` compares them as they are. The differences are listed under the kind of object they are about, each with its sign -- `+` added, `−` removed, `~` changed -- and its risk; `/` searches them, and their counts are filters, `a`, `r` and `c`. Beside them is the statement for the picked one in colour, or, with `[w Whole script]`, the script that makes the first like the second; `[y Copy]` copies it. `[⏎ Open script]` opens the script in a new document on the first connection, where it runs through that connection's own checks. Two schemas alike say so.

## History

History (`Ctrl+G h`, Search History, or `h` in the sidebar) lists every connection's statements, whether a connection is open or not, by day, newest first. Each statement run is kept on its own, failures and cancels too, with its outcome (`✓`, `✗`, `⊘`), when, how long it took, the rows it returned or changed, the error, and the database; the same statement on the same connection is listed once, its last run, with how many there were. `/` searches the SQL, `c` keeps one connection's -- the one in use first -- and `f` the failed or the succeeded. The picked one is shown in colour over its run's fields.

`[⏎ Open]` opens it in a new document on the connection it ran on; nothing runs. `[r Run again]` opens it and runs it, dialling the connection first. `[y Copy]` copies it, `[s Save…]` saves it as a query of that connection, and `[x Delete]` deletes it with its earlier runs. `[C Clear…]` clears what the filters show, after asking with how many go. Tab switches to the saved queries.

## Saved queries

Save Query As (Alt+S) keeps the selection, or the whole document, under a name, for the project and the connection; the same name replaces that query, and says so. Open Saved Query (Alt+O) is History's saved view: `/` searches names and SQL and `c` keeps one connection's; the picked query is shown in colour, and `[⏎ Open]` opens it in a new document on its connection, `[r Run]` runs it there, `[y Copy]` copies it, `[F2 Rename]` renames it and `[x Delete]` deletes it, after asking. A saved query belongs to a saved connection and goes with it; a temporary connection asks to be saved first.
