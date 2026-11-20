# P0 experiment E1 — install OpenDuet + OpenCore test files on ONE identified USB key.
# Reproduces the three writes of upstream BootInstallBase.sh (MBR code, FAT32 PBR, /boot file)
# but selects the target by serial number + size + bus, never by a typed device name (threat T1).
# Must run elevated. Everything is logged to build\install-e1.log.
param(
  [Parameter(Mandatory)] [string] $Serial,
  [Parameter(Mandatory)] [uint64] $ExpectedSize,
  [Parameter(Mandatory)] [string] $Stage,      # build\stage-e1
  [Parameter(Mandatory)] [string] $LegacyBoot, # upstream Utilities\LegacyBoot
  [Parameter(Mandatory)] [string] $Log,
  [string] $Label = 'L2U-E1'
)
$ErrorActionPreference = 'Stop'
Start-Transcript -LiteralPath $Log -Force | Out-Null
try {
  if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
      [Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Must run as administrator.' }

  function Get-TargetDisk {
    $d = @(Get-Disk | Where-Object { $_.SerialNumber -and $_.SerialNumber.Trim() -eq $Serial })
    if ($d.Count -ne 1) { throw "Expected exactly one disk with serial $Serial, found $($d.Count)." }
    $d = $d[0]
    if ($d.BusType -ne 'USB') { throw "Disk $($d.Number) is not USB ($($d.BusType))." }
    if ($d.Size -ne $ExpectedSize) { throw "Disk $($d.Number) size $($d.Size) != expected $ExpectedSize." }
    if ($d.IsBoot -or $d.IsSystem -or $d.Number -eq 0) { throw "Disk $($d.Number) is a boot/system disk." }
    if ($d.LogicalSectorSize -ne 512) { throw "Unexpected sector size $($d.LogicalSectorSize)." }
    $d
  }

  $boot0 = [IO.File]::ReadAllBytes((Join-Path $LegacyBoot 'boot0'))
  $boot1 = [IO.File]::ReadAllBytes((Join-Path $LegacyBoot 'boot1f32'))
  if ($boot0.Length -ne 512 -or $boot1.Length -ne 512) { throw 'boot0/boot1f32 must be 512 bytes.' }
  if (-not (Test-Path (Join-Path $Stage 'boot'))) { throw "Stage folder incomplete: $Stage" }

  $disk = Get-TargetDisk
  Write-Host ''
  Write-Host '=== DISQUE CIBLE ===' -ForegroundColor Yellow
  $disk | Format-List Number, FriendlyName, SerialNumber, BusType, Size, PartitionStyle | Out-Host
  Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Format-Table PartitionNumber, DriveLetter, Size | Out-Host
  Write-Host "Ce disque va etre ENTIEREMENT EFFACE." -ForegroundColor Red
  $ans = Read-Host 'Tape EFFACER pour continuer (autre chose = annuler)'
  if ($ans -cne 'EFFACER') { throw 'Cancelled by user.' }

  $n = $disk.Number
  Write-Host "[1/6] Clearing disk $n"
  Clear-Disk -Number $n -RemoveData -RemoveOEM -Confirm:$false
  Write-Host "[2/6] MBR + one active FAT32 partition"
  Update-HostStorageCache
  $style = (Get-Disk -Number $n).PartitionStyle
  if ($style -eq 'RAW') { Initialize-Disk -Number $n -PartitionStyle MBR }
  elseif ($style -ne 'MBR') { Set-Disk -Number $n -PartitionStyle MBR }   # Clear-Disk keeps GPT disks initialised
  Update-HostStorageCache
  if ((Get-Disk -Number $n).PartitionStyle -ne 'MBR') { throw 'Disk is not MBR after conversion.' }
  $part = New-Partition -DiskNumber $n -UseMaximumSize -MbrType FAT32 -IsActive -AssignDriveLetter
  Start-Sleep -Seconds 2
  $vol = Format-Volume -Partition $part -FileSystem FAT32 -NewFileSystemLabel $Label -Force -Confirm:$false
  $part = Get-Partition -DiskNumber $n -PartitionNumber $part.PartitionNumber
  $letter = $part.DriveLetter
  if (-not $letter) { throw 'No drive letter assigned.' }
  Write-Host "      volume ${letter}: $($vol.FileSystem) $($vol.FileSystemLabel)"

  $null = Get-TargetDisk   # identity re-check right before raw writes

  Write-Host "[3/6] Writing boot0 to MBR (446 bytes, partition table kept)"
  $fs = [IO.File]::Open("\\.\PhysicalDrive$n", 'Open', 'ReadWrite', 'ReadWrite')
  try {
    $mbr = New-Object byte[] 512; [void]$fs.Read($mbr, 0, 512)
    if ($mbr[510] -ne 0x55 -or $mbr[511] -ne 0xAA) { throw 'MBR signature missing after initialisation.' }
    [Array]::Copy($boot0, 0, $mbr, 0, 446)
    $fs.Position = 0; $fs.Write($mbr, 0, 512); $fs.Flush()
    $chk = New-Object byte[] 512; $fs.Position = 0; [void]$fs.Read($chk, 0, 512)
    if ([Convert]::ToBase64String($chk) -ne [Convert]::ToBase64String($mbr)) { throw 'MBR read-back mismatch.' }
  } finally { $fs.Close() }

  Write-Host "[4/6] Writing boot1f32 to the FAT32 boot sector (BPB kept)"
  & fsutil volume dismount "${letter}:" | Out-Host
  $fs = [IO.File]::Open("\\.\${letter}:", 'Open', 'ReadWrite', 'ReadWrite')
  try {
    $orig = New-Object byte[] 512; [void]$fs.Read($orig, 0, 512)
    $pbr = [byte[]]$boot1.Clone()
    [Array]::Copy($orig, 3, $pbr, 3, 87)                       # OEM name + BPB/EBPB from the formatted volume
    $rnd = New-Object byte[] 14; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($rnd)
    [Array]::Copy($rnd, 0, $pbr, 496, 14)                      # same as upstream BootInstallBase.sh
    $fs.Position = 0; $fs.Write($pbr, 0, 512); $fs.Flush()
    $chk = New-Object byte[] 512; $fs.Position = 0; [void]$fs.Read($chk, 0, 512)
    if ([Convert]::ToBase64String($chk) -ne [Convert]::ToBase64String($pbr)) { throw 'PBR read-back mismatch.' }
  } finally { $fs.Close() }

  Write-Host "[5/6] Copying OpenDuet /boot and EFI tree"
  Start-Sleep -Seconds 2
  Copy-Item -Path (Join-Path $Stage '*') -Destination "${letter}:\" -Recurse -Force
  @"
LEGACY2UEFI test key — experiment E1 (P0)
created: $(Get-Date -Format s)
disk serial: $Serial
contents: OpenDuet bootX64 + OpenCore 1.0.7 RELEASE, test config (no OS boot, UEFI Shell)
"@ | Set-Content -LiteralPath "${letter}:\LEGACY2UEFI-KEY.txt" -Encoding UTF8

  Write-Host "[6/6] Done. Files on ${letter}:" -ForegroundColor Green
  Get-ChildItem "${letter}:\" -Recurse -File | Select-Object FullName, Length | Format-Table -AutoSize | Out-Host
}
catch {
  Write-Host "ERREUR: $($_.Exception.Message)" -ForegroundColor Red
  $failed = $true
}
finally {
  Stop-Transcript | Out-Null
  Read-Host 'Appuie sur Entree pour fermer cette fenetre'
}
if ($failed) { exit 1 }
