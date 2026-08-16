# CLI

`dexo` with no subcommand starts the TUI. Subcommands reuse the same app layer.

Help text is golden-tested in `crates/dexo-cli/tests/help.rs`. Snippets:

```text
dexo connections add --name NAME --driver postgres --host 127.0.0.1 --username USER --database DB
dexo connections list
dexo query --connection NAME --sql "select 1" --format jsonl --non-interactive
dexo schema diff
dexo mcp serve --profile assistant
dexo doctor --json
```

`--non-interactive` never prompts. Destructive actions need an explicit confirm flag.
