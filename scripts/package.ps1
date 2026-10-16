# Windows packaging script for Autome 2.0.
# Technical design §18, milestone M4.
$ErrorActionPreference = "Stop"

$root = (Get-Item "$PSScriptRoot\..").FullName
$desktop = "$root\apps\desktop"
$staging = "$desktop\core"

Write-Host "==> cargo build --release"
cargo build --release --manifest-path "$root\Cargo.toml" -p automed
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Write-Host "==> staging the core binary"
# Keep staging inside the checkout and reject junctions/symlinks before writing.
$expected = [IO.Path]::GetFullPath((Join-Path $root 'apps\desktop\core'))
if ([IO.Path]::GetFullPath($staging) -ne $expected) { throw 'Unexpected staging path' }
foreach ($directory in @($desktop, $staging)) {
    if (Test-Path -LiteralPath $directory) {
        if ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing redirected staging directory: $directory"
        }
    }
}
New-Item -ItemType Directory -Force -Path $staging | Out-Null
Copy-Item "$root\target\release\automed.exe" "$staging\automed.exe"

Set-Location $desktop

if (-not (Test-Path "$desktop\node_modules\electron-builder")) {
    Write-Host "==> installing electron-builder"
    npm install --no-save electron-builder@26
    if ($LASTEXITCODE -ne 0) { throw "npm install electron-builder failed" }
}

$env:CSC_IDENTITY_AUTO_DISCOVERY = "false"
Write-Host "==> packaging Windows installer (NSIS)"
npx electron-builder --win --publish never
if ($LASTEXITCODE -ne 0) { throw "electron-builder failed" }

Write-Host "==> done: $desktop\dist"
