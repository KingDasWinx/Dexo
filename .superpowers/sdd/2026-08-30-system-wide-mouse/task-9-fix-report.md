# Task 9 follow-up — Schema Diff and Security viewport synchronization

## Cleanup

- Moved the Task 9 report from the repository root to this SDD directory.
- Commit: `967211d` — `chore: remove misplaced Task 9 report from repo root`

## Fix

- Schema Diff now renders a viewport containing the selected filtered entry and
  registers hit regions with their original list indices.
- Security now renders a viewport containing the selected principal and maps
  visible row hits back to their original indices.
- Schema Diff clamps the selection after filters change.

## Regression coverage

- Added a mouse-wheel test with 30 Schema Diff entries and 30 Security
  principals. It confirms each selected row retains an in-popup hit region
  after scrolling past the initial viewport.
- Added a Schema Diff unit test for selection clamping after filtering.

## Verification

- `cargo test -p dexo-tui --test mouse_accessibility` — 24 passed.
- `cargo test -p dexo-tui screens::schema_diff::tests` — 2 passed.
- `rustfmt --check` reports pre-existing formatting differences in
  `crates/dexo-tui/src/render.rs` outside this follow-up.
- `cargo clippy -p dexo-tui --tests -- -D warnings` is blocked by existing
  lint errors in `mouse_workbench_flow.rs` and an existing field-reassignment
  lint in `mouse_accessibility.rs`.
