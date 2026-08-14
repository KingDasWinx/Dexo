# Quick start

1. `dexo` opens the TUI.
2. Create a connection. Secrets go to the OS keychain, never SQLite.
3. Run `select 1` from the workbench or `dexo query --connection NAME --sql "select 1" --non-interactive`.
4. `dexo doctor --json` prints local health without opening the TUI.
