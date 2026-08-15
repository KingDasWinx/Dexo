#!/usr/bin/env bash
set -euo pipefail
if hits=$(rg -n "fixture_|fake_pg_dump|FakeChild" crates/dexo-tui/src crates/dexo-app/src); then
  echo "production fixture matches:"
  echo "$hits"
  exit 1
fi
echo ok
