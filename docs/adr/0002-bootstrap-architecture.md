# 0002 — Two-stage bootstrap: bridge path and diagnostics path

Status: Proposed | Date: 2026-11-19

## Context

Mandate step 5 compares (A) OpenDuet directly, (B) Legacy bootstrap + minimal Linux + probe +
OpenDuet, (C) anything better.

Key technical fact: OpenDuet is entered from the BIOS through `boot0` (MBR) → `boot1f32` (FAT32 PBR)
→ `/boot`, in real mode, relying on BIOS services. Once a Linux kernel has taken over the machine,
BIOS services and hardware state are no longer what OpenDuet expects; there is no supported
"kexec into OpenDuet". So B as a *single* boot is not a realistic design unless an experiment
proves otherwise.

## Options considered

- **A. Bridge only:** BIOS → OpenDuet → OpenCore. Minimal, proven on the golden reference. No diagnostics, no TUI.
- **B1. Linux then OpenDuet in the same boot:** rejected (above).
- **B2. Two boots:** a boot menu on the key offers (1) the bridge and (2) a diagnostics Linux. Linux
  probes, the user picks a profile, the engine writes `config.plist` *on the key*, reboot into (1).
- **C. Probe inside UEFI:** a Rust `no_std` UEFI application started by OpenCore reads
  CPUID/SMBIOS/PCI and selects a profile in the same boot. One boot, but no Ratatui/Slint, and it
  only runs once OpenDuet already works on that machine.

## Decision (proposed)

B2 as the product architecture, with A as the default entry: the machine boots the bridge with the
last known-good profile, no Linux needed. C is recorded as a P3+ research option.

**Open question for P0 experiments — how the key offers both entries:**

1. A legacy boot manager first (SYSLINUX or GRUB i386-pc in the MBR), with one entry chainloading the
   FAT32 PBR (`boot1f32` → OpenDuet) and one booting the Linux kernel in BIOS mode.
2. `boot0` stays in the MBR (bridge by default) and Linux is started *from OpenCore* as a UEFI
   entry. Simpler, but Linux then runs in UEFI mode under OpenDuet: the probe sees OpenDuet's
   tables, not the BIOS, and diagnostics are unavailable precisely when the bridge fails.

Option 1 is preferred because diagnostics must work when the bridge does not. Both must be tried in
QEMU + SeaBIOS before this ADR is Accepted.

## Consequences

- Profile switching = rewriting `\EFI\OC\config.plist` on the key → atomic write + backup (`recovery` crate, writing through `image-writer`).
- The diagnostics Linux is also the recovery path when the bridge fails.
- Two boot entries to test on every machine.
