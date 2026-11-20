# Builds the Legacy2UEFI launcher (UEFI app) on a Windows workstation with the rustup GNU toolchain.
# The host-side build scripts (slint-build) need MinGW's dlltool, shipped "self-contained" by rustup.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$toolchain = (Get-Content rust-toolchain.toml | Select-String 'channel\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
# rustup's self-contained dlltool cannot run without GNU `as`; use a full MinGW-w64 (WinLibs) instead.
# WinLibs 16.2.0 r3 (zip SHA-256 3c69305081e4a647144339a936244fcf7d3d9e558319e1ad7be2cd2886d4b6a1),
# unpacked in %USERPROFILE%\.l2u-tools. Override with $env:L2U_MINGW_BIN.
$mingw = if ($env:L2U_MINGW_BIN) { $env:L2U_MINGW_BIN } else { "$env:USERPROFILE\.l2u-tools\winlibs-16.2.0-r3\mingw64\bin" }
if (-not (Test-Path "$mingw\as.exe")) { throw "MinGW-w64 binutils not found in $mingw" }
$env:Path = "$env:USERPROFILE\.cargo\bin;$mingw;$env:Path"
Write-Host "toolchain $toolchain, mingw $mingw"
$ErrorActionPreference = 'Continue'   # cargo writes progress to stderr
cargo build --release 2>&1 | ForEach-Object { "$_" }
if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
$efi = Join-Path $PSScriptRoot 'target\x86_64-unknown-uefi\release\l2u-launcher.efi'
"built: $efi ($((Get-Item $efi).Length) bytes)"
