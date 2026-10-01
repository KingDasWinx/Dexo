# CLI

`dexo` with no subcommand starts the TUI. Subcommands reuse the same app layer.

`dexo <url>` starts it connected to a URL -- `postgres://`, `postgresql://`, `mysql://`, `mariadb://` or `sqlite:///path` -- without saving a connection; the password stays in memory, and `--password-prompt` asks for it instead of reading it from the URL. "Save Connection…" in the palette keeps it. `dexo --demo` starts it on a sample shop in SQLite, recreated on every run.

Help text is golden-tested in `crates/dexo-cli/tests/help.rs`. Snippets:

```text
dexo connections add --name NAME --driver postgres --host 127.0.0.1 --username USER --database DB
dexo connections add --name NAME --driver sqlite --path FILE
dexo connections list
dexo query --connection NAME --sql "select 1" --format jsonl --non-interactive
dexo schema diff
dexo mcp serve --profile assistant
dexo mcp profile set --name assistant --connection local --query-mode raw-read
dexo mcp allow --profile assistant --selector 'app.public.secrets' --deny --remove
dexo mcp config print --profile assistant --client claude-code
dexo doctor --json
```

`--non-interactive` never prompts. Destructive actions need an explicit confirm flag.
