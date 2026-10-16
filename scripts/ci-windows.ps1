param([switch]$Offline, [switch]$CoreOnly)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $cargoOptions = @()
    if ($Offline) { $cargoOptions += '--offline' }
    & cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Rust format failed' }
    & cargo build @cargoOptions -p automed
    if ($LASTEXITCODE -ne 0) { throw 'Core build failed' }
    & cargo clippy @cargoOptions --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
    # Build the real binary before tests: the Job keeper runs independently
    # from the core, and integration tests use the real render-stream filter.
    & cargo test @cargoOptions --workspace --all-targets -- --test-threads=4
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
    if (-not $CoreOnly) {
        Push-Location apps/desktop
        try {
            & npm test
            if ($LASTEXITCODE -ne 0) { throw 'Desktop tests failed' }
        } finally { Pop-Location }
    }
} finally { Pop-Location }
