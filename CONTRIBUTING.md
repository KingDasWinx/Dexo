# Contributing

Thanks for helping with Dexo. Bug reports, fixes, documentation, and ideas are all welcome.

## Before you start

- Found a bug? Use the [bug report form](https://forms.gle/gw1i6tGgVsJsgCxGA) or open an issue.
- For a new feature or a larger change, open an issue first so we can agree on the approach before you spend time on it.
- Security problems go through private reporting, never a public issue. See [SECURITY.md](SECURITY.md).

## Setup

- Rust 1.93 or later (the MSRV is set in the workspace `Cargo.toml`)
- Docker, optional, for the driver integration tests

```sh
git clone https://github.com/<your-user>/Dexo
cd Dexo
cargo run -p dexo
```

## Making a change

1. Fork the repository and branch off `development`.
2. Keep the change small and inside the crate that owns the behavior.
3. Add or update tests for what you changed.
4. Run the checks below.
5. Open the pull request against `development`.

## Checks

CI runs these on every pull request, on Linux, macOS, and Windows:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

On Linux, `cargo deny check` must pass too. Tests marked `#[ignore = "requires Docker"]` start real PostgreSQL and MySQL containers; run them with `cargo test -p <crate> -- --ignored` when you touch a driver.

## Commit messages and pull request titles

Use [Conventional Commits](https://www.conventionalcommits.org): `feat(scope): …`, `fix(scope): …`, `perf: …`, `docs: …`, `test: …`, `chore: …`. The changelog is generated from them, so write the subject for someone who uses Dexo:

```
fix(tui): the tab strip lights the open document
```

Pull requests are squash-merged, and the title becomes the commit, so give the title the same format.

## Rules

- Secrets never go in SQLite, TOML, argv, logs, or panic reports.
- The TUI, CLI, and MCP server go through `dexo-app`. Drivers do not import UI crates.
- New dependencies come from crates.io and must pass `cargo deny check`.

## License

Unless you state otherwise, any contribution you submit is dual-licensed under the MIT License or the Apache License 2.0, like the rest of Dexo, without any additional terms or conditions.

Everyone taking part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md).
