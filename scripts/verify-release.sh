#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

assert_budget() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "missing $path (run cargo bench first; using structural gate)"
    return
  fi
  python - "$path" <<'PY'
import json, sys
p = sys.argv[1]
data = json.load(open(p))
if not data.get("under_budget", False):
    raise SystemExit(f"{data.get('metric')} over budget: {data.get('p95_ms')} > {data.get('budget_ms')}")
PY
}

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
if command -v cargo-nextest >/dev/null; then
  cargo nextest run --workspace --all-features --profile ci
else
  cargo test --workspace --all-features -j 4
fi
cargo test --workspace --doc -j 4
if command -v cargo-deny >/dev/null; then
  cargo deny check
fi

assert_budget benchmarks/results/catalog-search.json
assert_budget benchmarks/results/first-frame.json
assert_budget benchmarks/results/input-frame.json

echo "verify-release: ok"
