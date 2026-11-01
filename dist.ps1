# Builds Hilal and packs it into dist\HilalSetup-<version>.exe.
# Run from PowerShell (not Git Bash): .\dist.ps1
$ErrorActionPreference = 'Stop'
$env:RUSTUP_HOME = 'K:\rust\rustup'
$env:CARGO_HOME = 'K:\rust\cargo'
$env:PATH = "K:\rust\cargo\bin;$env:PATH"
Set-Location $PSScriptRoot

cargo build --release -p hilal
if ($LASTEXITCODE) { throw 'hilal build failed' }
$env:HILAL_EXE = (Resolve-Path 'target\release\hilal.exe').Path
cargo build --release -p hilal-setup
if ($LASTEXITCODE) { throw 'setup build failed' }

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
New-Item -ItemType Directory -Force 'dist' | Out-Null
$out = "dist\HilalSetup-$version.exe"
Copy-Item 'target\release\HilalSetup.exe' $out -Force
Get-Item $out | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } }
