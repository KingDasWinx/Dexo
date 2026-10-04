# 1.4.2 Section 9: Launch Assets Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the README answers the questions a launch thread asks before anyone asks them, shows the guardrails at work, and every subcommand's `--help` says what it does.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 9 and "Positioning". Index and release-wide rules: `README.md` here.

## Rulings made from the code

- **The pitch** is the spec's: "A terminal database workbench with guardrails for AI agents."
- **`dexo --demo` at the top**, under the pitch, with the install line before it.
- **The comparison** covers rainfrog, harlequin, lazysql and sqlit from each project's README, documentation and changelog (and rainfrog's write checks from its code); a feature not found there is a dash, and the table says so and when it was read.
- **The GIF** is recorded from the real binary: a `DELETE` without `WHERE` on a production connection waits for the connection's name, then an agent's `UPDATE` through MCP waits in Agent Activity until it is approved. Frames are captured from tmux and drawn as the terminal shows them.
- **`--help`**: every subcommand, at every level, has a one-line description that says what it does, no more than it does; a test walks the command tree.

## Tasks

### Task 1: `--help`
- Descriptions on every subcommand; the help fixture updated; a test that no subcommand lacks one.
- Commit: `docs(cli): every subcommand says in one line what it does`.

### Task 2: README
- Pitch, quick try, the launch questions (psql/pgcli, DataGrip, rainfrog, passwords, Python, read-only), the comparison, the guardrails GIF; the light-theme screenshot the README already pointed at.
- Commit: `docs(readme): the pitch, a try in one line, the questions a launch asks, a comparison and the guardrails at work`.

## Review Focus

1. Every claim about Dexo in the README is true of this release; every claim about another project is in its README.
2. Every description in `--help` matches what the command does.
