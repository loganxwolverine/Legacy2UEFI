# P0 experiment E1 — derive a neutral OpenCore test config from the pinned Sample.plist.
# Goal of E1: BIOS -> OpenDuet -> OpenCore picker -> UEFI Shell on a BIOS-only reference machine.
# No OS boot, no macOS features, no SMBIOS spoofing. Throwaway tooling (Windows/PowerShell because
# the P0 workstation is Windows); the product build will be POSIX sh + Rust (mandate 0A).
param(
  [Parameter(Mandatory)] [string] $Sample,   # Docs/Sample.plist of the pinned OpenCore release
  [Parameter(Mandatory)] [string] $Out,      # destination config.plist
  [ValidateSet('Usb', 'Ps2')] [string] $Keyboard = 'Usb'
)
$ErrorActionPreference = 'Stop'
[xml]$x = Get-Content -Raw -LiteralPath $Sample

function Get-Val($dict, [string]$key) {
  $kids = @($dict.ChildNodes | Where-Object { $_.NodeType -eq 'Element' })
  for ($i = 0; $i -lt $kids.Count - 1; $i++) {
    if ($kids[$i].Name -eq 'key' -and $kids[$i].InnerText -eq $key) { return $kids[$i + 1] }
  }
  throw "key not found: $key"
}
function Set-Val($dict, [string]$key, [string]$type, [string]$text = '') {
  $old = Get-Val $dict $key
  $new = $x.CreateElement($type)
  if ($type -notin 'true', 'false') { $new.InnerText = $text }
  [void]$dict.ReplaceChild($new, $old)
}
function Path($p) { $n = $x.plist.dict; foreach ($k in $p.Split('/')) { $n = Get-Val $n $k }; $n }

# --- Security: no vault (needs signed vault.plist), no Apple secure boot model
$sec = Path 'Misc/Security'
Set-Val $sec 'Vault' 'string' 'Optional'
Set-Val $sec 'SecureBootModel' 'string' 'Disabled'
Set-Val $sec 'ScanPolicy' 'integer' '0'          # show every bootable entry (scan is read-only)

# --- Boot picker: wait for the user, show auxiliary entries (UEFI Shell)
$boot = Path 'Misc/Boot'
Set-Val $boot 'Timeout' 'integer' '0'
Set-Val $boot 'ShowPicker' 'true'
Set-Val $boot 'HideAuxiliary' 'false'

# --- Debug: log to a file at the key's root (Target 0x41 = enabled + file)
$dbg = Path 'Misc/Debug'
Set-Val $dbg 'Target' 'integer' '65'
# Sample's DisplayLevel 0x80000002 keeps only errors + warnings: E1 runs 1-2 produced all-zero log
# files. Add DEBUG_INFO (0x80000042) so a normal boot leaves a readable log.
Set-Val $dbg 'DisplayLevel' 'integer' '2147483714'
# Note (E1 run 3): DEBUG_INFO is only emitted by DEBUG/NOOPT builds; with RELEASE this has no effect.

# --- Find installed OS loaders on ANY volume without relying on UEFI boot entries.
# Under OpenDuet the NVRAM is emulated: entries written by an OS installer are not reliable after power-off.
# BlessOverride adds relative paths that OpenCore checks on every scanned volume. Distro shims first:
# booting \EFI\<distro>\shim directly also bypasses \EFI\BOOT\BOOTX64.EFI + fbx64.efi (fallback reboot loop).
$bless = Path 'Misc/BlessOverride'
$bless.RemoveAll()
foreach ($p in @(
    '\EFI\fedora\shimx64.efi',       # Fedora, Bazzite, Nobara, other Fedora Atomic images
    '\EFI\ubuntu\shimx64.efi',       # Ubuntu and derivatives (Mint, Pop!_OS with shim)
    '\EFI\debian\shimx64.efi',
    '\EFI\opensuse\shim.efi',
    '\EFI\rocky\shimx64.efi',
    '\EFI\almalinux\shimx64.efi',
    '\EFI\redhat\shimx64.efi',
    '\EFI\centos\shimx64.efi',
    '\EFI\systemd\systemd-bootx64.efi', # systemd-boot installs (Arch, Pop!_OS, EndeavourOS…)
    '\EFI\GRUB\grubx64.efi',         # grub-install default bootloader id (Arch, Gentoo, manual installs)
    '\EFI\arch\grubx64.efi',
    '\EFI\Manjaro\grubx64.efi',
    '\EFI\refind\refind_x64.efi'
  )) {
  $e = $x.CreateElement('string'); $e.InnerText = $p; [void]$bless.AppendChild($e)
}

# --- Text fallback picker in 1080p too (owner: 4K output was unreadable, E3 launcher test 2026-11-20)
Set-Val (Path 'UEFI/Output') 'Resolution' 'string' '1920x1080'

# --- Tools: OpenShell only. The Legacy2UEFI screen is the key's \EFI\BOOT\BOOTx64.efi (ADR-0008), which
# OpenCore already lists as the key's own entry; a separate "Legacy2UEFI" tool was a duplicate (owner asked
# the difference, 2026-11-20) and was removed.
$tools = Path 'Misc/Tools'
$tools.RemoveAll()
$entry = [xml]@'
<dict>
  <key>Arguments</key><string></string>
  <key>Auxiliary</key><false/>
  <key>Comment</key><string>UEFI Shell</string>
  <key>Enabled</key><true/>
  <key>Flavour</key><string>OpenShell:UEFIShell:Shell</string>
  <key>FullNvramAccess</key><false/>
  <key>Name</key><string>UEFI Shell</string>
  <key>Path</key><string>OpenShell.efi</string>
  <key>RealPath</key><false/>
  <key>TextMode</key><false/>
</dict>
'@
[void]$tools.AppendChild($x.ImportNode($entry.DocumentElement, $true))

# --- No macOS: disable all kexts, no device-property injection, no SMBIOS/DataHub/NVRAM spoofing
foreach ($k in (Path 'Kernel/Add').dict) { Set-Val $k 'Enabled' 'false' }
(Path 'DeviceProperties/Add').RemoveAll()
$pi = Path 'PlatformInfo'
Set-Val $pi 'UpdateDataHub' 'false'
Set-Val $pi 'UpdateNVRAM' 'false'
Set-Val $pi 'UpdateSMBIOS' 'false'

# --- Drivers: only what E1 needs. OpenDuet already provides emulated NVRAM.
# Keyboard (E1b finding): under OpenDuet a USB keyboard did not respond with Ps2KeyboardDxe alone,
# although USB storage works. ocvalidate 1.0.7 forbids OpenUsbKbDxe + Ps2KeyboardDxe together and
# requires KeySupport=true with Ps2KeyboardDxe. So the keyboard type is a profile decision.
if ($Keyboard -eq 'Usb') {
  $keep = @('OpenRuntime.efi', 'OpenUsbKbDxe.efi')
  Set-Val (Path 'UEFI/Input') 'KeySupport' 'false'
} else {
  $keep = @('OpenRuntime.efi', 'Ps2KeyboardDxe.efi')
  Set-Val (Path 'UEFI/Input') 'KeySupport' 'true'
}
foreach ($d in (Path 'UEFI/Drivers').dict) {
  $p = (Get-Val $d 'Path').InnerText
  Set-Val $d 'Enabled' ($(if ($keep -contains $p) { 'true' } else { 'false' }))
}

$settings = New-Object System.Xml.XmlWriterSettings
$settings.Indent = $true; $settings.IndentChars = "`t"; $settings.Encoding = New-Object System.Text.UTF8Encoding($false)
$w = [System.Xml.XmlWriter]::Create($Out, $settings); $x.Save($w); $w.Close()
"written: $Out"
