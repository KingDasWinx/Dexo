# Task 9 — Overlay wheel + hit accuracy sweep

## Commits

- `95ff78b` — `fix(tui): make overlay mouse hits and scrolling accurate`
- `5578cd5` — `style(tui): keep existing mouse test formatting unstaged`
- `62ce6f4` — `test(tui): satisfy mouse overlay lint checks`

## Verification

- `cargo test -p dexo-tui --test mouse_accessibility` — 23 passed
- A suíte completa mantém snapshots pendentes de tarefas anteriores; não foram
  atualizados fora do escopo da Task 9.
- O Clippy do pacote também encontra avisos preexistentes em testes não
  relacionados (`mouse_accessibility` e `mouse_workbench_flow`).

## Scope delivered

- Diagnostics exposes a dedicated Export control; diagnostic text no longer triggers export.
- Parameter footer is rendered and field clicks no longer submit.
- Mouse wheel reaches connection forms, schema diff, security, and transfer preview content.
- MCP profile rows are rendered and mapped to their actual profile indices.
- Label hit rectangles use terminal display-column widths.
