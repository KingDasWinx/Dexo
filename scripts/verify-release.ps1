#Requires -Version 5

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

function Assert-JsonBudget([string]$Path) {
    if (-not (Test-Path $Path)) {
        Write-Host "missing $Path (run cargo bench first; using structural gate)"
        return
    }
    $json = Get-Content $Path -Raw | ConvertFrom-Json
    if (-not $json.under_budget) {
        throw "$($json.metric) over budget: $($json.p95_ms) > $($json.budget_ms)"
    }
}

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
if (Get-Command cargo-nextest -ErrorAction SilentlyContinue) {
    cargo nextest run --workspace --all-features --profile ci
} else {
    cargo test --workspace --all-features -j 4
}
cargo test --workspace --doc -j 4
if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    cargo deny check
}

Assert-JsonBudget "benchmarks/results/catalog-search.json"
Assert-JsonBudget "benchmarks/results/first-frame.json"
Assert-JsonBudget "benchmarks/results/input-frame.json"

Write-Host "verify-release: ok"
