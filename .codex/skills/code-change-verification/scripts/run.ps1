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
Invoke-Step -Label "cargo test --manifest-path vendor/pingora-header-serde/Cargo.toml" -Command {
    cargo test --manifest-path vendor/pingora-header-serde/Cargo.toml
}
Invoke-Step -Label "cargo test --manifest-path vendor/sqlx-sqlite/Cargo.toml --features bundled (self-patch)" -Command {
    cargo test --manifest-path vendor/sqlx-sqlite/Cargo.toml --features bundled --config 'patch.crates-io.sqlx-sqlite.path="vendor/sqlx-sqlite"'
}
# The standalone check resolves a temporary test graph; audit the workspace lock.
Remove-Item -LiteralPath "vendor/sqlx-sqlite/Cargo.lock" -ErrorAction SilentlyContinue

Write-Host "code-change-verification: all commands passed."
