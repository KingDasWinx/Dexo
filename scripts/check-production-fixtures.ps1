$ErrorActionPreference = "Stop"
$hits = rg -n "fixture_|fake_pg_dump|FakeChild" crates/dexo-tui/src crates/dexo-app/src
if ($hits) {
    Write-Error "production fixture matches:`n$hits"
    exit 1
}
Write-Output "ok"
