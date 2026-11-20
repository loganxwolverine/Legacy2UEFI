# P0 — replace the EFI tree on an existing Legacy2UEFI test key. No repartitioning, no boot
# sector writes, no admin needed. Refuses any volume that is not the identified key.
param(
  [Parameter(Mandatory)] [string] $Serial,
  [Parameter(Mandatory)] [string] $Stage,
  [string] $Label = 'LEGACY2UEFI'   # 'L2U-E1' for the first hand-made test key
)
$ErrorActionPreference = 'Stop'
$disk = @(Get-Disk | Where-Object { $_.SerialNumber -and $_.SerialNumber.Trim() -eq $Serial -and $_.BusType -eq 'USB' })
if ($disk.Count -ne 1) { throw "Key with serial $Serial not found (or found $($disk.Count) times)." }
$part = @(Get-Partition -DiskNumber $disk[0].Number | Where-Object DriveLetter)
if ($part.Count -ne 1) { throw 'Expected exactly one lettered partition on the key.' }
$l = $part[0].DriveLetter
$vol = Get-Volume -DriveLetter $l
if ($vol.FileSystemLabel -ne $Label -or -not (Test-Path "${l}:\LEGACY2UEFI-KEY.txt")) {
  throw "Volume ${l}: is not the Legacy2UEFI test key (label '$($vol.FileSystemLabel)', marker missing)."
}
# Keep OpenCore logs from previous runs: move them to a local folder first
$logs = @(Get-ChildItem "${l}:\opencore-*.txt" -ErrorAction SilentlyContinue)
if ($logs) {
  $dst = Join-Path (Split-Path $Stage) 'key-logs'; New-Item -ItemType Directory -Force $dst | Out-Null
  $logs | ForEach-Object { Copy-Item $_.FullName $dst -Force; "saved log: $($_.Name)" }
}
Remove-Item "${l}:\EFI" -Recurse -Force
Copy-Item (Join-Path $Stage 'EFI') "${l}:\" -Recurse -Force
Copy-Item (Join-Path $Stage 'boot') "${l}:\boot" -Force
$bad = 0
Get-ChildItem $Stage -Recurse -File | ForEach-Object {
  $r = $_.FullName.Substring($Stage.Length)
  if ((Get-FileHash $_.FullName).Hash -ne (Get-FileHash "${l}:$r").Hash) { "MISMATCH $r"; $bad++ }
}
if ($bad) { throw "$bad file(s) differ after copy." }
"key ${l}: updated from $Stage"
