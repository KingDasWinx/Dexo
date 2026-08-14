# Performance baseline

Reference hardware (developer workstation, 2026-08-14): Windows 10 x64, release profile, `cargo bench -j 4`.

Budgets from spec section 15:

| Metric | Budget | Evidence | Result |
| --- | --- | --- | --- |
| Catalog search 100k | p95 <= 100ms | `crates/dexo-app/benches/catalog_search.rs` | gated in verify-release |
| Grid viewport 100k | bounded visible slice | `crates/dexo-tui/src/widgets/grid.rs` viewport tests | pass |
| Incremental parse | no full reparse required | `crates/dexo-sql` ParserService | pass |
| 1m-row export | streaming, no collect | `crates/dexo-app/src/transfer/export.rs` | pass |
| First frame | <= 300ms | `crates/dexo-tui/benches/first_frame.rs` | gated |
| Input to frame | <= 50ms | `crates/dexo-tui/benches/input_frame.rs` | gated |

Raw JSON is written under `benchmarks/results/`.
