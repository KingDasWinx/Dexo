# CLI

`dexo` with no subcommand starts the TUI. Subcommands reuse the same app layer.

`dexo <url>` starts it connected to a URL -- `postgres://`, `postgresql://`, `mysql://`, `mariadb://`, `sqlite:///path` or, in a build with DuckDB, `duckdb:///path` (a DuckDB file, or a CSV, Parquet or JSON file) -- without saving a connection; the password stays in memory, and `--password-prompt` asks for it instead of reading it from the URL. "Save Connection…" in the palette keeps it. `dexo --demo` starts it on a sample shop in SQLite, recreated on every run.

Help text is golden-tested in `crates/dexo-cli/tests/help.rs`. Snippets:

```text
dexo connections add --name NAME --driver postgres --host 127.0.0.1 --username USER --database DB
dexo connections add --name NAME --driver sqlite --path FILE
dexo connections add --name NAME --driver duckdb --path FILE
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

`query`, `run`, `export`, `import` and `explain --analyze` hold the SQL to the connection's policy, as the editor does, before anything is dialled: a read-only connection refuses every statement that is not a read (and `import`, and an `EXPLAIN ANALYZE` of one); a production connection runs no write until `--confirm-target <connection>` names it; elsewhere a destructive statement -- `DELETE` or `UPDATE` without `WHERE`, `DROP`, `TRUNCATE`, `ALTER … DROP` -- waits for `--confirm` when the connection asks before them. Nothing is asked at the terminal: what is not confirmed is not run, and the error lists the statements and the flag. `export` runs only reads.

## Language server

`dexo lsp` brings Dexo's completion, diagnostics and formatting to any editor that speaks the Language Server Protocol, on stdin and stdout. It completes and checks against the catalog Dexo cached for a connection -- `--connection name`, or a file's first line `-- dexo: connection=name` -- and never dials the database itself, so open the connection once in Dexo, or run `dexo inspect --connection name --refresh`, to cache its catalog. A catalog cached or refreshed while the editor runs is picked up without restarting the server. Formatting follows the editor's tab size, or indents with tabs when it asks for them.

Neovim 0.11 and later:

```lua
vim.lsp.config("dexo", {
  cmd = { "dexo", "lsp", "--connection", "shop" },
  filetypes = { "sql" },
})
vim.lsp.enable("dexo")
```

Neovim 0.10, for every SQL buffer:

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = "sql",
  callback = function(args)
    vim.lsp.start({ name = "dexo", cmd = { "dexo", "lsp", "--connection", "shop" } }, { bufnr = args.buf })
  end,
})
```

Helix (`languages.toml`):

```toml
[language-server.dexo]
command = "dexo"
args = ["lsp", "--connection", "shop"]

[[language]]
name = "sql"
language-servers = ["dexo"]
```

VS Code has no setting for a language server of your own, so it takes an extension that starts one, such as Generic LSP Client (v2) (`zsol.vscode-glspc`). In `settings.json`:

```json
{
  "glspc.server.command": "dexo",
  "glspc.server.commandArguments": ["lsp", "--connection", "shop"],
  "glspc.server.languageId": ["sql"]
}
```
