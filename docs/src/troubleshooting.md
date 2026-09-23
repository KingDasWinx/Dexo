# Troubleshooting

- `dexo doctor --json` — paths, schema version, keychain availability.
- Invalid TOML names the file and field; unknown keys are preserved.
- After a crash, the recovery screen offers restore or discard. Open transactions are never restored as `active`.
- Settings parse failures keep the last valid file and a `.bak` copy.
- If the mouse does not respond, check the status bar for `MOUSE OFF`. Press `Ctrl+P`, run `settings.mouse`, and restart Dexo if the terminal does not resume capture immediately.
- `NO_COLOR` disables color in the CLI.
