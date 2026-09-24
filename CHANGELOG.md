# Changelog

## 1.3.0

### Features

- A Windows installer and a portable .exe
- Tell the user when a newer Dexo is out

## 1.2.0

### Features

- Ctrl+R on an offline table connects first
- Ctrl+R refreshes a table's data
- Clicking the console focuses it
- Closing unsaved work asks before losing it
- Closing the last document leaves nothing open
- The entrance comes up in the saved theme
- Colour the sidebar row the cursor points at
- Make the tab strip a pane instead of a second focus
- A document belongs to a connection
- Name the actions key where the sidebar names its other two
- Act on a sidebar node without leaving the sidebar
- Thin the command palette down to what it is for
- Complete names the sidebar has not loaded yet
- Walk a snippet's holes with Tab
- Open the popup where it belongs, and accept what it says
- Suggest the join condition the foreign key already knows
- Offer what the position asks for, best match first
- Read the cursor's context off the tokens
- Add a lexer, and stop completing inside strings and comments
- Add the Messages view to the output pane
- Give toasts a severity, and keep errors on screen
- Group catalog objects by class in the sidebar
- Split the light/dark mode from the system accent color
- Surface the actual mutation-conflict detail in the Review modal
- Let the export/import modal pick a real format, add a direct key
- Commit or discard pending row changes with Ctrl+S / Ctrl+Shift+R
- Add a Ctrl+N modal to insert a new row locally
- Mark/restore a grid row for deletion with the Delete key
- Add row-identity lookup and GridModel::remove_row
- Fetch and apply real primary-key metadata when a table opens
- Implement table_columns via information_schema
- Implement table_columns via pg_constraint
- Add a table_columns capability to DataMutator
- Open sidebar tables into their own grid-primary document tab
- Add keybinds to hide the explorer, results, and inspector panels
- Add search to the F1 keybindings overlay
- Make the settings modal a navigable form
- Open recent SQL files from the picker
- Name documents on create and rename with F2
- Add a reusable text input widget
- Fold connections into the explorer tree
- Align execution shortcuts with DataGrip
- Warn in the status bar when mouse capture is off
- Open the results action menu with a double-click
- Select SQL text by dragging with the mouse
- Resize workbench panes by dragging dividers
- Add sidebar New/Edit mouse affordances for connections
- Make inspector tabs and wheel scrolling mouse-reachable
- Close and create SQL document tabs with the mouse
- Render the first-run onboarding overlay
- Handle onboarding keys, mouse, and logo ticks
- Persist onboarding completion from the runtime
- Play entrance animation and open onboarding on first run
- Treat onboarding as a blocking overlay for mouse hits
- Add onboarding actions and model state
- Add entrance splash helpers and first-run completion marker
- Add a collapsible Advanced section to the connection form
- Make connection and SQL tab actions reachable without palette
- Autosave dirty SQL documents with a path on checkpoint
- Open per-connection console.sql on connect
- Show connections in sidebar with click-to-connect
- Add VS Code-style SQL document tab strip
- Bind editor documents to unique ids and connections
- Add per-connection SQL file directory helpers
- Give SQL, Data, DDL and Properties their own editor views
- Toggle specific result rows with Ctrl+Enter
- Group explorer catalog children into folders
- Grow explorer and inspector width with Alt+= when focused
- Wire help, layout controls, focus keys, and results row actions
- Track help, results menu, and grid cursor in the model
- Add layout presets and honor results height in compact mode
- Restyle the theme toward Yazi-like pane and grid colors
- Render highlights overlays hits and status messages
- Run ddl explain admin mcp and snippets via managers
- Dispatch unwired actions through existing effects
- Restore checkpoints and track file picker mode
- Add editor overlays and explorer inspector helpers
- Expose execute, tabs, and data commands in keymap and palette

### Fixes

- A file opened with Ctrl+O shows its text
- Scope the ttfx test's import to Unix
- Clear clippy warnings so CI's -D warnings passes
- The tab strip lights the document that is open
- Each table document keeps its own paging and edits
- Clicking a table's grid focuses the grid
- Popups paint in the theme's colours
- Document keys work from whichever pane holds the document
- Find placeholders with the lexer, not a byte scan
- One set of keys for every Submit/Cancel dialog
- Accepting a completion inserts the name, not an alias too
- Actually enable bracketed paste, and make Ctrl+V paste
- Paste as a paste, not as the keys it looks like
- Tell the terminal what colour to draw the caret
- Label the documents that were already there
- Each document keeps its own results
- Say something when a connection refuses
- Stop reopening every document the project ever held
- Let the editor see the columns of a table
- Keep the cursor row lined up with the header
- Give the grid columns room to breathe
- Send text parameters for the server to parse
- Stop reporting values it cannot decode as NULL
- Re-derive the row viewport when the document changes
- Give the model the terminal size it is actually drawn into
- Let the cursor reach the last row the pane paints
- Size every result set by the pane that draws it
- Give the grid the room a table document has
- Size the grid viewport to the rows it actually paints
- Let Alt+Up/Down resize the console pane
- Let Alt+Left/Right switch documents from any pane
- Stop opening a Properties modal when a table opens
- Make Alt+1/2/3 follow the panes a table document has
- Populate the MCP grants list from the ledger
- Reach per-profile grant revocation and profile disable
- Make config import/export and MCP enable actually reachable
- Re-arm the offline catalog capture after folding connections in
- Rebuild real hierarchy when restoring the offline catalog
- Apply a settings reset to the live state
- Restore every saved setting on startup
- Widen result columns as longer values scroll into view
- Stop narrow result columns from being truncated first
- Keep the sidebar focused when a connection opens
- Resolve the statement under the caret on unicode text
- Recover local database after schema downgrade
- Route wheel input to visible overlay
- Ignore non-left mouse buttons for activation clicks
- Keep schema and security selections visible
- Make overlay mouse hits and scrolling accurate
- Reset inspector scroll when changing tabs
- Block workbench mouse through completion and match overlay z-order
- Clear project recovery checkpoints after a document flush
- Clear recovery rows after documents save or flush
- Checkpoint workbench layout into session recovery
- Restore recovery documents automatically on bootstrap
- Clear the explorer when a session closes or a profile is deleted
- Keep the last results row visible under the column header
- Show group/name paths and shorten the connections footer
- Keep the Linux clipboard handle alive after copy
- Prevent stale document saves from closing newer edits
- Clear the clippy warnings this branch introduced
- Explain Edit Connection when nothing is selected
- Keep a dirty tab open until its save is acknowledged
- Line sidebar mouse hits up with the rows it draws
- Make write_sql_file durable again
- Keep `n` typable in the SQL editor
- Catalog-empty state and global n in emacs keymap
- Empty states and footer hints for sidebar workbench
- Scope sidebar footer shortcuts to connection focus
- Place autosave reducer arm correctly
- Restore autosave action variant boundaries
- Ignore stale connection SQL results
- Preserve connection overlay selection
- Keep sidebar connection navigation focused
- Preserve dirty document tabs on close
- Bind NewDocument connection_id to profile UUID
- Make destructive and diagnostic flows visible
- Connect editor and explain palette flows
- Load schema diff and security flows
- Dispatch import export backup and restore safely
- Validate transaction data and explorer commands
- Open visible project and connection flows
- Pick Add Connection driver with left/right instead of typing
- Show column names without the table prefix
- Embed JSON columns instead of escaping them as strings
- Scroll the file picker when arrow keys move past the window
- Anchor SQL completion to the cursor like vim
- Drop explorer footer chrome and hide leaf twisties
- Let Enter collapse an already open explorer folder
- Pan results columns with Left/Right

### Performance

- An arrow key costs the screen, not the script

### Other changes

- Tudo
- Pre work: restore parked row-detail results menu and drop superseded sprint docs
- Toma
- Add footer actions to TUI forms
- Improve explorer enter, scroll, and completion UX
- Expose SQL completion token APIs
- Improve results grid navigation and copy actions

## 1.1.0

Functional completion of the approved workbench: live schema/diff/transfer/explain, administration, settings, recovery, and policy-enforced multi-connection MCP.

- Driver-specific DDL planning, protected apply, and live/saved/file schema diff
- Streaming import/export, secure native backup/restore, EXPLAIN save/compare
- Live admin views, durable settings, crash recovery, and local diagnostics
- MCP connection router, expiring grants, and production fixture prohibition
- Release artifacts include checksums and an SBOM derived from the lockfile

## 1.0.0

First production release of Dexo: PostgreSQL and MySQL workbench, CLI, and local MCP server.

- Local-first SQLite state (schema v7) with keychain secrets
- TUI workbench, schema diff, transfer, explain, and admin
- MCP stdio adapter with grants and sanitized audit
- Release gates: fmt, clippy, tests, deny, fuzz smoke, performance budgets
