# Troubleshooting

- `dexo doctor --json` — paths, schema version, keychain availability.
- Invalid TOML names the file and field; unknown keys are preserved.
- After a crash, the recovery screen offers restore or discard. Open transactions are never restored as `active`.
- Settings parse failures keep the last valid file and a `.bak` copy.
- `NO_COLOR` disables color in the CLI.
