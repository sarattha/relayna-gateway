Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$repoRoot = $null

try {
    $repoRoot = (& git -C $scriptDir rev-parse --show-toplevel 2>$null)
} catch {
    $repoRoot = $null
}

if (-not $repoRoot) {
    $repoRoot = Resolve-Path (Join-Path $scriptDir "..\..\..\..")
}

Set-Location $repoRoot

if (-not (Test-Path "Cargo.toml")) {
    Write-Host "code-change-verification: no Cargo.toml found; skipping Rust workspace checks."
    Write-Host "Add Cargo.toml before relying on this skip for runtime, test, or build changes."
    exit 0
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Label,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    Write-Host "Running $Label..."
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Write-Error "code-change-verification: $Label failed with exit code $LASTEXITCODE."
        exit $LASTEXITCODE
    }
}

Invoke-Step -Label "cargo fmt --all --check" -Command { cargo fmt --all --check }
Invoke-Step -Label "cargo clippy --workspace --all-targets --all-features -- -D warnings" -Command {
    cargo clippy --workspace --all-targets --all-features -- -D warnings
}
Invoke-Step -Label "cargo test --workspace --all-features" -Command { cargo test --workspace --all-features }

Write-Host "code-change-verification: all commands passed."
