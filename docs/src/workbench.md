# Workbench

The TUI is keyboard-first: explorer, SQL editor, results, inspector, and a command palette. Layouts persist per project. Compact mode hides extra panes on small terminals.

SQL execution streams result pages. Manual transactions stay visible. Closing a tab does not silently abandon a running query.

Theme, keymap, mouse capture, Unicode, and animation persist in a local settings file and apply immediately. Mouse clicks map to the same commands as the keyboard.

## SQL editor

| Key | Action |
| --- | --- |
| Ctrl+F / Ctrl+H | Find / find and replace in the document. Enter or F3 next, Shift+Enter previous, Alt+C case, Alt+W whole word, Alt+A replace all, Esc close. Where the terminal sends Ctrl+Backspace as Ctrl+H, Alt+R in the find bar opens the replace row. |
| Ctrl+/ | Comment the line or selection out with `--`, or back in |
| Ctrl+Shift+D | Duplicate the line or selection |
| Ctrl+Shift+Up / Down | Move the line or selection up or down |
| Ctrl+E | Edit the document in `$VISUAL` or `$EDITOR` (`ctrl+x ctrl+e` in the Emacs keymap) |

Lines starting with a backslash are psql's commands, answered by Dexo from the catalog on every database and never sent to the server: `\dt`, `\dv`, `\di`, `\dn` and `\df` with an optional pattern (`*` and `?` wildcards), `\d name` for a table's columns, keys and indexes, `\l` for databases, `\x` for one field per line, and `\?` for the list.
