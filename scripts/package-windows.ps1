$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Root

$VersionLine = Get-Content Cargo.toml | Where-Object { $_ -match '^\s*version\s*=' } | Select-Object -First 1
if (-not $VersionLine) {
    throw "Could not read package version from Cargo.toml"
}
$Version = ($VersionLine -split '=', 2)[1].Trim().Trim('"')
$Target = if ($env:CARGO_BUILD_TARGET) { $env:CARGO_BUILD_TARGET } else { (rustc -vV | Select-String '^host:').ToString().Split(':', 2)[1].Trim() }
$Arch = ($Target -split '-', 2)[0]
$OutDir = if ($args.Count -gt 0) { $args[0] } else { "dist" }
$Asset = "claude_clone-v$Version-windows-$Arch.exe"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
cargo build --release
Copy-Item -Force "target\release\claude_clone.exe" (Join-Path $OutDir $Asset)

Write-Output (Join-Path $OutDir $Asset)
