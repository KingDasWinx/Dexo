# Final fix wave — system-wide mouse

## Fix

- Routed vertical mouse-wheel input through `top_overlay(model)`, matching
  overlay paint and click routing.
- The visible overlay now exclusively receives wheel input; lower overlays and
  the workbench remain unchanged.
- Horizontal wheel input is also blocked while any overlay is visible.

## Regression coverage

- Added a stacked palette/snippets test. With snippets as the topmost overlay,
  scrolling changes its selection and leaves the hidden palette selection
  unchanged.

## Verification

- `cargo test -p dexo-tui --test mouse_accessibility`
- `cargo clippy -p dexo-tui --tests -- -D warnings`
- `cargo fmt --check`
