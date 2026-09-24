# Privacy

No telemetry. Diagnostics are generated only by an explicit user action, sanitized, and previewed before any local zip is written. Crash reports are not uploaded.

## Update check

Once a day, Dexo sends one request to `github.com/KingDasWinx/Dexo/releases/latest` to learn the newest release, and shows it on the status bar with the command that updates your install. The request carries only the running version in its `User-Agent`; the answer is cached for 24 hours in the data directory (`update-check.json`). Being offline is silent.

Turn it off under Settings → Updates, or set `DEXO_NO_UPDATE_CHECK=1` for scripts and CI.
