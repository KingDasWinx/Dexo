# Dexo Sprint 15: Release Hardening 1.0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Fechar integralmente a spec, comprovar budgets, segurança e compatibilidade, publicar artefatos reproduzíveis e declarar Dexo 1.0 completo.

**Architecture:** No new product subsystem is introduced. This sprint converts every specification requirement into an automated release gate or an explicit verified manual checklist and fixes all discovered gaps before tagging 1.0.

**Tech Stack:** cargo-nextest, cargo-fuzz, cargo-deny/RustSec, Criterion benchmarks, cargo-dist, CycloneDX/SPDX SBOM, GitHub Actions native matrix, mdBook.

---

## File map

- Create: `benches/*`, `fuzz/*`, `tests/e2e/*`
- Create: `.github/workflows/{integration.yml,release.yml,security.yml}`
- Create: `dist-workspace.toml`, `release.toml`, `docs/book.toml`, `docs/src/*`
- Create: `scripts/verify-release.ps1`, `scripts/verify-release.sh`
- Create: `docs/testing/{compatibility-matrix.md,performance-baseline.md,release-checklist.md}`
- Modify: every crate only where a failed gate exposes a spec gap.

### Task 1: Audit spec coverage mechanically

- [x] **Step 1:** Create `docs/testing/release-checklist.md` with one checkbox for every bullet in spec sections 5–19 and columns `test/manual`, `path`, `result`.
- [x] **Step 2:** Link each checkbox to an existing test/command/screenshot; run `rg '\[ \]' docs/superpowers/plans docs/testing/release-checklist.md` and record uncovered requirements.
- [x] **Step 3:** For every uncovered requirement, add a failing test in the owning crate, implement the minimal missing behavior, rerun it and commit one requirement group at a time.
- [x] **Step 4:** Run the scan again; expected no unchecked product requirement before proceeding.
- [x] **Step 5:** Commit with `git commit -m "test(release): map every spec requirement to evidence"`.

### Task 2: Lock the supported database/platform matrix

- [x] **Step 1:** Discover vendor-supported PostgreSQL/MySQL releases at implementation time and record exact image digests in `compatibility-matrix.md`; include oldest supported, vendor-recommended/LTS and newest stable.
- [x] **Step 2:** Add CI matrix running driver catalog/query/mutation/DDL/diff/explain/admin contracts against every recorded image on Linux, plus client/TUI/storage/keychain smoke tests on native macOS/Windows.
- [x] **Step 3:** Add handshake tests marking outside-matrix versions `unverified` and feature detection by capability, not number alone.
- [x] **Step 4:** Run the complete matrix; fix every failure or remove a version only with vendor end-of-support evidence in the same commit.
- [x] **Step 5:** Commit with `git commit -m "test(release): verify database and OS compatibility matrix"`.

### Task 3: Enforce performance budgets

- [x] **Step 1:** Add benchmarks for startup-to-first-frame, input-to-frame under active query, 100k catalog search, 100k grid viewport, incremental parse, 1m-row export memory and schema diff.
- [x] **Step 2:** Run on documented reference hardware with release profile and store raw results plus environment in `performance-baseline.md`.
- [x] **Step 3:** Add a release script comparing medians/p95 against spec budgets: first frame <=300ms, input <=50ms, catalog search <=100ms; streaming tests assert bounded bytes instead of timing.
- [x] **Step 4:** Optimize failed budget with profiling evidence, rerun and preserve before/after results.
- [x] **Step 5:** Commit with `git commit -m "perf: satisfy Dexo 1.0 performance budgets"`.

### Task 4: Complete fuzzing and security review

- [x] **Step 1:** Add fuzz targets for statement splitter/classifier, driver value decoders, identifier quoting, transfer codecs, config/migrations, MCP schemas/selectors and secret redaction.
- [x] **Step 2:** Run each target for at least 30 minutes in scheduled CI and a 5-minute release smoke; save crashing corpus as tests and fix every crash.
- [x] **Step 3:** Run `cargo deny check`, RustSec audit, license/SBOM validation, secret-sentinel E2E, unsafe-code inventory and dependency source audit. Deny new unreviewed git dependencies.
- [x] **Step 4:** Perform threat-check tests for SQL scope bypass, MCP enumeration/exfiltration, stdout injection, SSH host-key change, TLS downgrade, temp-file permissions and native-tool argument injection.
- [x] **Step 5:** Commit with `git commit -m "security: harden all external input boundaries"`.

### Task 5: Build reproducible signed artifacts

- [x] **Step 1:** Configure `dist-workspace.toml` for Linux/macOS/Windows archives, shell/PowerShell installers, Homebrew and MSI where supported; include completions, licenses and checksums.
- [x] **Step 2:** Add release workflow pinned by SHA that builds from tag, generates CycloneDX/SPDX SBOM, signs artifacts/checksums with the selected project signing mechanism and uploads no secrets to pull-request jobs.
- [x] **Step 3:** Build twice from the same commit in clean runners; compare normalized artifact hashes and document unavoidable platform-signature variance separately.
- [x] **Step 4:** Install each artifact on clean native runner; run `dexo --version`, `doctor --json`, CLI fixture query, TUI launch/restore and MCP initialize/conformance smoke.
- [x] **Step 5:** Commit with `git commit -m "build: produce reproducible Dexo installers"`.

### Task 6: Finish user and maintainer documentation

- [x] **Step 1:** Build mdBook sections for install, quick start, connections/TLS/SSH/keychain, workbench, data/schema/diff/transfer/admin, CLI reference, MCP profiles/grants/security, privacy, troubleshooting and driver development.
- [x] **Step 2:** Generate CLI help examples from the binary and fail CI when checked-in reference differs; validate every command snippet in a test harness.
- [x] **Step 3:** Add architecture/ADR, crate boundaries, capability contract, new-driver checklist, migration policy, security policy, contribution guide and code of conduct.
- [x] **Step 4:** Run link checker, `mdbook test`, secret scan and fresh-user tutorial on clean fixtures.
- [x] **Step 5:** Commit with `git commit -m "docs: complete Dexo 1.0 documentation"`.

### Task 7: Verify migrations and recovery from every released schema

- [x] **Step 1:** Store sanitized SQLite fixtures for schema versions 1–7 and config fixtures for every public format version.
- [x] **Step 2:** Test upgrade to current, backup creation before destructive migration, rollback/recovery after simulated disk-full and rejection of future unsupported format.
- [x] **Step 3:** Test project/config export-import, keychain missing/locked, crash document recovery and MCP grant expiration across restart.
- [x] **Step 4:** Run native filesystem tests on Linux/macOS/Windows.
- [x] **Step 5:** Commit with `git commit -m "test(storage): verify all migrations and recovery paths"`.

### Task 8: Execute the final 1.0 release gate

- [x] **Step 1:** Ensure `git status --short` is empty and every sprint exit checkbox plus `docs/testing/release-checklist.md` is checked.
- [x] **Step 2:** Run exact release commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features --profile ci
cargo test --workspace --doc
cargo deny check
cargo audit
```

Expected: all PASS, zero warnings/advisories without documented time-bounded exception.

- [x] **Step 3:** Run Docker compatibility matrix, native OS E2E, MCP conformance, fuzz smoke, performance gate, docs/link tests and artifact install tests. Expected: all PASS.

- [x] **Step 4:** Set workspace version `1.0.0`, update changelog/release notes, rerun all gates and create final commit:

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md docs
git commit -m "release: Dexo 1.0.0"
```

- [ ] **Step 5:** Create signed tag only from the clean green commit and let release workflow publish:

```bash
git tag -s v1.0.0 -m "Dexo 1.0.0"
git push origin HEAD --follow-tags
```

Expected: signed artifacts/checksums/SBOM/installers published and smoke-installed successfully.

## Sprint exit — project complete

- [x] Every spec section 5–19 maps to passing evidence.
- [ ] Every Sprint 00–15 checkbox is complete.
- [x] PostgreSQL/MySQL supported matrix is green.
- [x] Linux/macOS/Windows native gates are green.
- [x] Performance budgets are met on documented hardware.
- [x] Security, fuzz, secret, MCP conformance and dependency audits are green.
- [x] Documentation and all installers are verified from a clean machine.
- [ ] `v1.0.0` is signed, reproducible artifacts are published, and no required work remains.
