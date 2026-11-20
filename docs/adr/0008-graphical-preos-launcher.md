# 0008 — Graphical pre-OS launcher in the bridge

Status: Proposed (owner request, 2026-11-19) | Date: 2026-11-19

## Context

The owner wants the bridge to look and feel modern — a clear break with the legacy BIOS look — and to:
- choose between **booting the installed OS from internal storage** (default) and **booting another USB
  key to install an OS**;
- show the machine's specs: CPU, memory size and type (DDR2/DDR3…), and more.

The OpenCore text picker works (E1) but is plain text. The mandate's GUI stack (Slint) was planned for
the diagnostics Linux, not for the pre-OS bridge.

## Options considered

1. **OpenCanopy** (OpenCore's graphical picker, themes). Pretty icons, but cannot show hardware specs
   or custom layouts; themes come from a separate resource pack.
2. **Our own UEFI application in Rust**, started by OpenCore as the default entry: draws on the GOP
   framebuffer with Slint's software renderer (`no_std` platform), reads specs from SMBIOS (system,
   CPU, memory devices incl. DDR type and speed), CPUID, the UEFI memory map, PCI (GPU, storage
   controllers) and BlockIo/filesystems (boot candidates), then starts the chosen loader with
   `LoadImage`/`StartImage`. OpenCore's drivers (OpenRuntime, keyboard) stay loaded.
3. Graphics only in the diagnostics Linux — does not meet the request: the user would see it only on
   the diagnostics path.

## Decision (proposed)

Option 2, with strict fallbacks:
- OpenCore starts first; the launcher is its default entry. Any launcher failure (no GOP, panic, timeout)
  returns to the OpenCore text picker, which stays fully usable (UI invariant in `agent.md`).
- Keyboard-first; default = internal OS with a visible countdown that any key stops; "Install from USB"
  lists every bootable removable device found.
- Specs shown are measured, never guessed: an unreadable value is shown as "unknown" (grey).
- Universal resolution (owner, 2026-11-20): EDID native ≤ 1080p, else current mode, else ≤ 1080p; layout scaled by
  discrete steps from 1080p down to 800×600 (see ADR-0008).

## To verify before Accepted

- Slint on bare UEFI: the Slint repository ships a UEFI demo — confirm it builds with the pinned
  versions and the `x86_64-unknown-uefi` target, and measure binary size.
- OpenDuet exposes SMBIOS from the legacy BIOS tables (needed for memory type / speed).
- GOP modes available under OpenDuet on the Aurora R1 (E1 showed a full-HD text console).
- Starting shim/GRUB from our app works as it does from OpenCore.

## Consequences

- New crate `crates/launcher-uefi` (`no_std`), sharing data types with `hardware-probe`.
- Rust toolchain with the `x86_64-unknown-uefi` target required on the build machine.
- The UI skill (`legacy2uefi-ui`) covers both the pre-OS launcher and the Linux GUI/TUI.

## Progress — 2026-11-20: v0.1 built (not yet run on hardware)

- Crate `crates/launcher-uefi` (own Cargo workspace, `no_std`, target `x86_64-unknown-uefi`), Rust 1.100.0 pinned
  in `rust-toolchain.toml`, Slint **1.19.0** (latest published; 1.20 is unreleased), `uefi` 0.41, font Inter (OFL).
- v0.1 scope: measured specs panel (SMBIOS 2.x/3.x: machine, board, BIOS, memory devices; CPUID: brand and
  x86-64 level v1–v4 with the first missing feature; GOP resolution; firmware vendor/revision), "Démarrage" panel
  with only the working action (return to the OpenCore picker, Enter/Escape); the two boot actions are shown as
  "next version". Unknown values are shown as "inconnu" in grey; the DDR type is shown only if SMBIOS declares it.
- Fallbacks: no GOP, Slint init failure or event-loop error → plain-text summary on the console, then return.
- Installed on the key as OpenCore tool `Legacy2UEFI` (`EFI/OC/Tools/L2U-Launcher.efi`, 537 600 bytes); `ocvalidate` clean.
- Build on Windows: `crates/launcher-uefi/build-launcher.ps1` (rustup GNU host + WinLibs MinGW-w64 16.2.0 r3 for
  `dlltool`/`as`, needed by host build scripts).

## Revision — 2026-11-20 (owner decision after testing v0.1): the launcher IS the boot menu

v0.1 test on the Aurora R1: GOP came up in 3840×2160; text was unreadable (the software renderer embeds
glyphs at the font sizes written in the .slint file, so runtime scaling enlarged boxes but not text).
Owner feedback: the graphical interface must be the boot experience itself, from power-on to the OS — not a
tool started from the OpenCore picker.

Revised decision:
- The launcher is the key's first UEFI program: `\EFI\BOOT\BOOTx64.efi` (loaded by OpenDuet). OpenCore stays
  on the key (`\EFI\OC\OpenCore.efi`) as the text-mode fallback, reachable from the launcher ("Menu OpenCore").
- Launcher steps: force GOP 1920×1080 (else the largest mode ≤ 1920×1080) → load `\EFI\L2U\drivers\*.efi`
  (keyboard driver per ADR-0006) and connect all controllers → scan every volume for loaders (same path list
  as BlessOverride + `\EFI\Microsoft\Boot\bootmgfw.efi` + `\EFI\BOOT\BOOTX64.EFI`) → show boot entries
  (internal first), tools (UEFI Shell, Menu OpenCore, restart, power off) and the specs panel → start the
  choice with LoadImage/StartImage. Default: first internal system after a 10 s countdown any key stops;
  no internal system → no countdown.
- UI drawn for 1080p with literal font sizes. Keys read from every Simple Text Input instance directly.
- Fallbacks: no GOP / Slint failure → text menu; panic → chain to OpenCore, else cold reset after 10 s.
- OpenCore text picker also set to 1920×1080 (`UEFI/Output/Resolution`).

Consequence: OpenCore no longer runs on the default path, so its ACPI/SMBIOS/quirk features are not applied
when booting through the launcher. With the current neutral profile they are all disabled anyway; any future
profile that needs them must boot through OpenCore. Status of this ADR stays **Proposed** until the v0.2
screen is validated on hardware.

v0.2 built: `l2u-launcher.efi` 843 264 bytes, Slint 1.19.0, uefi 0.41 (`alloc`, `global_allocator`; own panic handler).

## v0.2 test (2026-11-20) and v0.3

Owner test of v0.2 on the Aurora R1: no graphical screen; a text menu appeared and the Bazzite USB key was not
offered. The key afterwards held an OpenCore log stamped 09:01 UTC, so OpenCore ran — consistent with the
launcher's panic fallback (cause unknown: v0.2 had no log). Owner restated the requirement: one graphical screen
at power-on listing **every** way to boot (internal disks and USB keys, with understandable names), chosen with
the arrows; never a BIOS-like text menu.

v0.3 (858 624 bytes, on the key):
- Writes a step log to the key only, `\L2U-LAUNCHER-LOG.txt` (GOP modes, drivers, every volume scanned and why
  it was or was not listed, GUI creation result, panic message). Flushed at each stage and before starting a loader.
- Startup order changed: drivers + connect → scan → GOP mode → GUI (v0.2 switched GOP mode first).
- Device names: USB product from the device's string descriptors (e.g. "Clé USB SanDisk Ultra Fit · 57 Go"),
  internal disks as "Disque interne SATA · 238 Go"; system name from the loader path or from `\EFI\<dir>` names
  (CachyOS, Fedora / Bazzite, …) instead of "Système installé".
- Every whole disk is listed; disks with nothing bootable appear greyed ("Aucun système démarrable").

## v0.3 on hardware (2026-11-20) and v0.3.1

**First successful run of the graphical boot screen** on the Aurora R1 at power-on: 1920×1080, entries
"Linux (Limine) · Disque interne SATA 256 Go" (default, countdown) and "Fedora / Bazzite · Clé USB SanDisk
3.2Gen1 61 Go", tools row, specs panel with measured values. Evidence: `launcher-v0.3-screen.png`.
Defects seen: "É", "ç", "·" missing in text coming from Rust (Slint embeds only ASCII + characters written in
the .slint file); "USB USB" in the USB name; "Par défaut" badge stretched. A second capture shows the OpenCore
text picker afterwards (`launcher-v0.3-then-opencore.png`); how it was reached is not known yet (launcher log on the key to read).

v0.3.1 (995 328 bytes, staged, not yet on the key): hidden charset text in the .slint file, generic USB
manufacturer strings dropped, badge no longer stretched, system name read from Limine's configuration
(first `/` entry, e.g. "CachyOS").

Launcher log of the v0.3 run (`tests/evidence/alienware-aurora-r1/2026-11-20/launcher-v0.3-log.txt`): 1 driver
loaded (OpenUsbKbDxe); 3 volumes — Bazzite USB via `\EFI\fedora\shimx64.efi`, the key (skipped), internal SATA
via `\EFI\BOOT\BOOTX64.EFI`; GOP offers 640x480…1920x1080 and 3840x2160 (current was 1920x1080). The log stops
after the GOP step: the second flush (delete + recreate) failed silently on OpenDuet's FAT driver, so the result
of selecting Bazzite is unknown. v0.3.1 on the key: append-only log, and each choice + its outcome logged.

## Owner decision (2026-11-20): no automatic boot

The Legacy2UEFI screen is where the user chooses. The 10 s countdown to the internal system is removed;
the screen waits for a choice, cursor on the first bootable entry. (Supersedes "default = first internal
system after 10 s" above.)

## Boot path fix (2026-11-20): the screen is what OpenDuet starts

Finding: every launcher run in the v0.3.1 log was preceded by an OpenCore log — OpenCore was starting first and
the owner reached the screen through the picker. OpenDuet source (`Legacy/BootPlatform/BdsDxe/BdsEntry.c`,
OpenCorePkg master): BDS ignores BootOrder and `\EFI\BOOT\BOOTX64.EFI`; it only starts `\EFI\OC\OpenCore.efi`
(first on the disk carrying the BOOT1 signature, then on any volume).

Owner requirement: booting the PC from the L2U-E1 key must land on the Legacy2UEFI screen by default.

New key layout (`build/stage-l2u`, on the key):
- `\EFI\OC\OpenCore.efi` = **Legacy2UEFI screen** (the file OpenDuet starts).
- `\EFI\BOOT\BOOTx64.efi` = Legacy2UEFI screen too (removable-media default; also the key's own entry in OpenCore).
- `\EFI\OpenCore\` = real OpenCore 1.0.7 + config.plist + Drivers + Tools (text fallback, "Menu OpenCore").
- `\EFI\L2U\drivers\OpenUsbKbDxe.efi` = keyboard driver loaded by the screen.
To verify on hardware: OpenCore started from `\EFI\OpenCore\` finds its config.plist there (it is expected to use
its own directory as root, not a fixed `\EFI\OC`).

## Universal resolution (2026-11-20, second machine)

Test on a **Lenovo ThinkPad 4414WBF** (BIOS 6AET58WW 2009, Core 2 Duo T6670 = x86-64-v1, 3 GB at 667 MT/s):
the key booted straight to the Legacy2UEFI screen (boot path fix confirmed on a second machine) and listed a
Ventoy key, but the 1080p layout overflowed the laptop panel. Evidence: `tests/evidence/lenovo-thinkpad-4414wbf/`.

Owner requirement: one design that works and looks right on every screen. No single resolution does that
(a 1280×800 panel cannot show 1080p; 1024×768 is blurry and distorted on a 16:9 monitor). Decision:
- **Mode:** EDID native resolution if offered and ≤ 1920×1080; else keep the firmware's current mode if
  …
- **Layout:** designed at 1920×1080 and scaled by discrete steps (1.0, 0.85, 0.75, 0.667, 0.6, 0.533, 0.45, 0.4)
  = largest step ≤ min(w/1920, h/1080). Glyphs for every base size × step are embedded at build time
  (`SLINT_FONT_SIZES` computed in build.rs). Long texts elide or wrap.
v0.4 built (1 463 808 bytes, `build/stage-l2u`), not yet tested on hardware.
Correction from the ThinkPad log (`tests/evidence/lenovo-thinkpad-4414wbf/2026-11-20/launcher-log.txt`): its GOP
offers only 640x480, 800x600, 1024x768 and starts in 800x600. Rule 2 is therefore "the largest offered mode
≤ 1920×1080" (not "keep the current mode"). On that machine: 1024×768 at scale 0.533. v0.4 on the key.
The ThinkPad run also listed no internal disk at all (only the two USB volumes): to investigate.
ThinkPad v0.4 (2026-11-20): layout fits at 1024×768 (scale 0.533); SMBIOS declares DDR2. Evidence: launcher-v0.4-screen.png.
ThinkPad note (owner): no hard disk was installed, so no internal entry is expected — not a detection bug.
