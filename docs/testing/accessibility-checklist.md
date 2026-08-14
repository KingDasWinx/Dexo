# Terminal accessibility checklist

Run from the repository root. Expected screens are text-only; color and Unicode are optional.

1. `cargo test -p dexo-tui --test sprint14_screens -- --nocapture`
   - Expected: settings, recovery, MCP audit snapshots; palette contains every command id.
2. `NO_COLOR=1 cargo test -p dexo-tui --test theme_snapshots`
   - Expected: `[PROD]`, `[ERR]`, `>` remain distinct with `color=none`.
3. Open command palette (`Ctrl+P` / `settings.open` via palette).
   - Expected: every action listed with a reason when disabled. Mouse is not required.
4. Disable mouse in Settings (`mouse=false`).
   - Expected: clicks ignored; Tab/arrows/palette still move focus.
5. ASCII / `DEXO_ASCII=1`.
   - Expected: production shows `[PROD]`, errors `[ERR]`, selection `>`.
6. Resize below 60x24.
   - Expected: compact single-panel layout; no clipped-only controls.
7. Terminal capabilities (`diagnostics.export` preview).
   - Expected: color depth, unicode, mouse flags shown as text.
8. Crash recovery.
   - Expected: recover/discard offered; transaction is `unknown`, never `active`.
