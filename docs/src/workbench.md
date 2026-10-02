# Workbench

The TUI is keyboard-first: explorer, SQL editor, results, inspector, and a command palette. Layouts persist per project. Compact mode hides extra panes on small terminals.

SQL execution streams result pages. Manual transactions stay visible. Closing a tab does not silently abandon a running query.

Theme, keymap, mouse capture, Unicode, and animation persist in a local settings file and apply immediately. Mouse clicks map to the same commands as the keyboard.

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
| Ctrl+/ | Comment the line or selection out with `--`, or back in |
| Ctrl+Shift+D | Duplicate the line or selection |
| Ctrl+Shift+Up / Down | Move the line or selection up or down |
| Ctrl+E | Edit the document in `$VISUAL` or `$EDITOR` (`ctrl+x ctrl+e` in the Emacs keymap) |

The editor underlines what is wrong as you type: a statement that does not parse, and, once the catalog has been read whole, a table or a `alias.column` the database does not have -- never one it simply has not loaded. With the cursor on an underline the status line says what it is. When a run fails and the server says where, that spot is underlined and the cursor goes to it.

Lines starting with a backslash are psql's commands, answered by Dexo from the catalog on every database and never sent to the server: `\dt`, `\dv`, `\di`, `\dn` and `\df` with an optional pattern (`*` and `?` wildcards), `\d name` for a table's columns, keys and indexes, `\l` for databases, `\x` for one field per line, and `\?` for the list.

### Vim mode

With the Vim keymap (Settings), the editor is modal: Normal, Insert, Visual (`v`) and Visual-line (`V`), with the mode on the status line and a block cursor outside Insert. Normal mode takes counts and the motions `h j k l w b e 0 ^ $ gg G`; the operators `d c y` with a motion, or doubled for lines (`dd yy cc`); `x p P u` and Ctrl+R; `i a I A o O` into Insert; `.` to repeat the last change; `/` to search, `n` and `N` for the next and previous match; `:s/old/new/[g]` on the line, `:%s` on the document; `:w`, `:q`, `:wq`, `:x` (which writes only what changed) and `:<line>`. The text objects `iw` and `aw` work after an operator (`diw`, `ciw`) and in Visual mode, where `X`, `D`, `C` and `Y` take whole lines. Every change, an Insert session included, is one undo step, and a count on `.` replaces the change's own. Ctrl chords -- the palette, running a statement, quitting -- work in every mode; Esc then a key typed fast is two keys, as in Vim, not an Alt chord.

## Results

Above a table's rows, or the result of a statement that only reads, two bars take SQL of your own: `w` focuses WHERE, `o` focuses ORDER BY, Enter runs the grid again with them, Esc puts back what last ran, Tab moves between them; a click focuses one. What they hold must be a condition and a list of sort keys -- one more statement, a locking read, a function with side effects or an executable comment is refused before anything is sent -- and the run happens where it cannot write: in a read-only transaction of its own, or inside your open transaction behind a savepoint undone after it. A run that fails puts the rows that were there back.

`s` sorts by the current column (Left and Right move it, the header marks it), `S` adds it to the sort; clicking a header sorts by it, ascending, then descending, then off, and Shift+click, Alt+click or a right click adds it. The sort is the ORDER BY bar's text, and the headers show its order. A result run again with the bars is a page: `n` and `p` turn it.

The title says how many rows there are: exactly when they all came, `~4.3M` from the server's statistics for a table's first page, `100+` when more may follow. `t` counts them exactly -- a table's on a connection of its own, a result's on its session -- and `t` again stops the count; the count stays while the grid shows the rows it counted.

`f` on a row opens Related rows: each foreign key from or to the table, followed in a document of its own filtered to the rows on the other end; `b` closes that document and goes back to the row. Enter on a row lists what can be done with it, with each action's key.

## Explain

F7 shows the estimated plan of the statement under the cursor, Shift+F7 runs it with ANALYZE after asking; `v` steps through the tree, the table and a summary, and a second plan of the same statement is compared with the first. On Postgres with the hypopg extension, `i` tries an index before anyone builds it: type its definition (`CREATE INDEX ON orders (customer_id)`) and the statement is planned as if it existed, compared with its plan without it. The index exists only for that plan, on that session; `dexo explain --index "CREATE INDEX ON …"` does the same from the command line. Without hypopg, Dexo says so: install the package on the server, then `CREATE EXTENSION hypopg`.

## Saved queries

Save Query As (Alt+S) keeps the selection, or the whole document, under a name, for the project and the connection; the same name replaces that query, and says so. Open Saved Query (Alt+O) searches names and SQL, shows the query, opens it in a new document on its connection, renames it (F2) and deletes it (Delete, then confirm). A saved query belongs to a saved connection and goes with it; a temporary connection asks to be saved first.
