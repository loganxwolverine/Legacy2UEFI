# Builds a Legacy2UEFI release: key image (.img), zipped image for balenaEtcher, SHA-256 sums.
# Inputs are pinned: OpenCore release archive (SHA-256 checked), Rust toolchain (rust-toolchain.toml).
# Nothing is written to any disk: the output is files in build\release\<version>\.
param(
  [string] $Version = '0.1.0-alpha.1',
  [string] $OpenCoreZip = "$env:USERPROFILE\Downloads\OpenCore-1.0.7-RELEASE.zip",
  [int] $SizeMiB = 128
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot\..\.."
$ocSha = '2FFAB6EBF58C7AEFB0BCB3A1A385D207746823D6DD87D44BD666E1286939943E'   # OpenCore-1.0.7-RELEASE.zip
$out = "$root\build\release\$Version"
$up = "$root\build\upstream\OpenCore-1.0.7-RELEASE"
$stage = "$out\stage"
$mingw = "$env:USERPROFILE\.l2u-tools\winlibs-16.2.0-r3\mingw64\bin"
$env:Path = "$env:USERPROFILE\.cargo\bin;$mingw;$env:Path"

function Step($m) { Write-Host "== $m" }

Step 'OpenCore archive'
if ((Get-FileHash $OpenCoreZip -Algorithm SHA256).Hash -ne $ocSha) { throw "OpenCore archive hash mismatch: $OpenCoreZip" }
if (-not (Test-Path "$up\Utilities\LegacyBoot\boot0")) {
  Expand-Archive -LiteralPath $OpenCoreZip -DestinationPath $up -Force
}

Step "launcher version $Version"
$lc = "$root\crates\launcher-uefi\Cargo.toml"
$t = [IO.File]::ReadAllText($lc)
$t = [regex]::Replace($t, '(?m)^version = "[^"]+"', "version = `"$Version`"", 1)
[IO.File]::WriteAllText($lc, $t, (New-Object Text.UTF8Encoding($false)))
& "$root\crates\launcher-uefi\build-launcher.ps1" | Out-Host
$launcher = "$root\crates\launcher-uefi\target\x86_64-unknown-uefi\release\l2u-launcher.efi"

Step 'image builder'
Push-Location "$root\crates\image-builder"
$ErrorActionPreference = 'Continue'
cargo build --release 2>&1 | ForEach-Object { "$_" } | Out-Host
$code = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
Pop-Location
if ($code -ne 0) { throw 'image builder build failed' }
$builder = "$root\crates\image-builder\target\release\l2u-image-builder.exe"

Step 'stage'
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
foreach ($d in 'EFI\BOOT', 'EFI\OC', 'EFI\L2U\drivers', 'EFI\OpenCore\Drivers', 'EFI\OpenCore\Tools', 'EFI\OpenCore\ACPI', 'EFI\OpenCore\Kexts') {
  New-Item -ItemType Directory -Force "$stage\$d" | Out-Null
}
Copy-Item "$up\Utilities\LegacyBoot\bootX64" "$stage\boot"
Copy-Item $launcher "$stage\EFI\OC\OpenCore.efi"          # OpenDuet only starts this path (ADR-0008)
Copy-Item $launcher "$stage\EFI\BOOT\BOOTx64.efi"
Copy-Item "$up\X64\EFI\OC\Drivers\OpenUsbKbDxe.efi" "$stage\EFI\L2U\drivers\"   # USB keyboard required (ADR-0006)
Copy-Item "$up\X64\EFI\OC\OpenCore.efi" "$stage\EFI\OpenCore\OpenCore.efi"      # text fallback
Copy-Item "$up\X64\EFI\OC\Drivers\OpenRuntime.efi", "$up\X64\EFI\OC\Drivers\OpenUsbKbDxe.efi" "$stage\EFI\OpenCore\Drivers\"
Copy-Item "$up\X64\EFI\OC\Tools\OpenShell.efi" "$stage\EFI\OpenCore\Tools\"
& "$root\tools\p0-experiments\make-test-config.ps1" -Sample "$up\Docs\Sample.plist" -Out "$stage\EFI\OpenCore\config.plist" -Keyboard Usb | Out-Null
$v = & "$up\Utilities\ocvalidate\ocvalidate.exe" "$stage\EFI\OpenCore\config.plist"
if ($LASTEXITCODE -ne 0) { $v | Out-Host; throw 'ocvalidate failed' }
@"
LEGACY2UEFI key
version: $Version
built: $(Get-Date -Format s)
"@ | Set-Content "$stage\LEGACY2UEFI-KEY.txt" -Encoding ASCII
# Licences travel with the binaries (GPL-3.0-or-later for Legacy2UEFI, third-party notices and texts).
New-Item -ItemType Directory -Force "$stage\LICENSES" | Out-Null
Copy-Item "$root\LICENSE" "$stage\LICENSES\GPL-3.0.txt"
Copy-Item "$root\THIRD-PARTY-NOTICES.md" "$stage\LICENSES\THIRD-PARTY-NOTICES.md"
Copy-Item "$root\third_party\opencore\LICENSE.txt" "$stage\LICENSES\OpenCorePkg-BSD-3-Clause.txt"
Copy-Item "$root\crates\launcher-uefi\ui\fonts\Inter-LICENSE.txt" "$stage\LICENSES\Inter-OFL-1.1.txt"
Copy-Item "$root\docs\licenses\rust-crates-launcher.txt" "$stage\LICENSES\rust-crates.txt"
$readme = [IO.File]::ReadAllText("$PSScriptRoot\LISEZMOI-cle.txt").Replace('{VERSION}', $Version)
[IO.File]::WriteAllText("$stage\LISEZMOI.txt", $readme.Replace("`n", "`r`n").Replace("`r`r`n", "`r`n"), (New-Object Text.UTF8Encoding($true)))

Step 'image'
$img = "$out\legacy2uefi-$Version.img"
& $builder --stage $stage --legacyboot "$up\Utilities\LegacyBoot" --out $img --size-mib $SizeMiB --label LEGACY2UEFI
if ($LASTEXITCODE -ne 0) { throw 'image build failed' }
$zip = "$out\legacy2uefi-$Version.zip"
Compress-Archive -LiteralPath $img -DestinationPath $zip -CompressionLevel Optimal
$sums = foreach ($f in @($img, $zip, $launcher)) { "{0}  {1}" -f (Get-FileHash $f -Algorithm SHA256).Hash.ToLower(), (Split-Path $f -Leaf) }
$sums | Set-Content "$out\SHA256SUMS.txt" -Encoding ASCII
$sums | Out-Host
Get-ChildItem $out -File | Select-Object Name, Length | Format-Table -AutoSize | Out-Host
