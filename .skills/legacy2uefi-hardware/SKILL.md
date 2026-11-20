---
name: legacy2uefi-hardware
description: How to detect and reason about hardware for Legacy2UEFI (CPU, SMBIOS/firmware, PCI, USB host controllers, storage controllers, GPU/VBIOS/GOP, read-only disk analysis). Use for any probe code, hardware report analysis, or profile decision. Defers to legacy2uefi-domain for invariants.
metadata:
  version: "0.1.0-draft"
  status: DRAFT
---

# Legacy2UEFI — hardware

Invariants (no disk write, UNKNOWN ≠ compatible) come from `legacy2uefi-domain`. Not repeated here.

## Rule zero: detect capabilities, not brands

```rust
// Forbidden when a capability can be measured:
if smbios.vendor == "Dell" { profile = DellProfile }
// Expected:
if cpu.long_mode && storage.controllers.iter().all(|c| c.reachable_by(Bridge::Duet)) { ... }
```

Vendor/model strings are allowed only as (a) a lookup key into the compatibility database, or
(b) a documented quirk with an ADR or database entry explaining why no capability test exists.

## Each probe result is a fact with provenance

Every value carries: source (CPUID, sysfs, SMBIOS table, PCI config space…), raw value, and
confidence. "Not readable" is a distinct state from "absent". Never default a missing value to
the optimistic one.

## CPU

- CPUID: vendor string, family/model/stepping (with extended family/model), brand string.
- Long mode (CPUID 0x80000001 EDX bit 29) decides X64 vs IA32 OpenDuet/OpenCore.
- Also record: NX, SSE2/SSE4.x, number of logical CPUs. Hypervisor bit (CPUID 1 ECX bit 31) → mark the report as virtual.
- Linux source: `/proc/cpuinfo` is acceptable for P1; direct `cpuid` via `core::arch::x86_64::__cpuid` preferred in Rust.
- **x86-64 microarchitecture level** (v1/v2/v3/v4) must be computed and shown: modern distros ship v2 or v3
  builds (e.g. CachyOS v3 when installed on a v3 CPU).
  Field case (Aurora R1, i7-920 = v2): an installed
  CachyOS rebooted on both kernels while Bazzite booted fine through the same bridge. v2 = SSE3/SSSE3/SSE4.1/
  SSE4.2/POPCNT/CX16/LAHF; v3 adds AVX/AVX2/BMI1/BMI2/FMA/MOVBE/LZCNT/F16C/XSAVE; v4 adds AVX-512F/BW/CD/DQ/VL.

## Firmware / SMBIOS

- `/sys/firmware/dmi/tables/` (raw) or `/sys/class/dmi/id/*`. Record BIOS vendor, version, release date, system manufacturer/product, board.
- Boot mode as seen by Linux: `/sys/firmware/efi` absent → BIOS boot (expected in the diagnostics path).
- Never read or write firmware flash.
- Field lesson (Aurora R1, BIOS A11 2010, SMBIOS 2.5): memory devices report `Type: Other` although they
  are DDR3. Old SMBIOS cannot be trusted for the DDR generation. Show "unknown" or an explicitly *inferred*
  value (e.g. "DDR3 — inferred from the X58 memory controller"), never a guessed one presented as measured.
  Speed (MT/s), size and slot population were reported correctly.

## PCI

- `/sys/bus/pci/devices/*/{vendor,device,class,subsystem_*}`; bridges by class `0x0604`.
- Chipset identity from the host bridge / LPC (class `0x0601`) device.
- Use `pci.ids` only for display; decisions use class codes and IDs.

## USB host controllers (class 0x0C03, prog-if)

| prog-if | Type | Note for the bridge |
|---|---|---|
| 0x00 | UHCI | USB 1.1 |
| 0x10 | OHCI | USB 1.1 |
| 0x20 | EHCI | USB 2.0 — key boot path on most legacy PCs |
| 0x30 | xHCI | USB 3; OpenCore ships `XhciDxe.efi` |

Record which controller the boot key sits behind (sysfs path of the block device).

## Storage controllers

- Class 0x0101 IDE, 0x0106 SATA (prog-if 0x01 = AHCI), 0x0104 RAID, 0x0108 NVMe.
- Also record the BIOS SATA mode (IDE/AHCI/RAID) as *observed* (class/prog-if), not assumed.
- NVMe on legacy boards (PCIe adapter): OpenCore ships `NvmExpressDxe.efi`; boot from it is a capability question.
- OpenDuet `-blockio` variants rely on BIOS INT 13h for disk access: candidate fallback for controllers with no UEFI driver. To be evaluated in P0 audit / P1 tests, not assumed.

## GPU

- PCI class 0x03xx; integrated vs discrete; presence of a legacy Option ROM (`/sys/bus/pci/devices/*/rom`, read-only, needs enabling — do it only in a reviewed code path).
- Under OpenDuet the console/GOP depends on what the environment provides; record GOP availability as observed from the bridge (OpenCore log), not guessed from Linux.

## Keyboard / input (first P0 field finding — E1, 2026-11-19)

Without a working keyboard the bridge is unusable (picker, Shell, recovery). Treat input as a
first-class capability, decided per machine:

- Detect what is actually connected: PS/2 keyboard (i8042 / `serio` in Linux) vs USB HID keyboard
  (`/sys/bus/usb/devices/*/bInterfaceClass == 03`, protocol 01) and which host controller it sits behind.
- Under OpenDuet a USB keyboard was dead with only `Ps2KeyboardDxe` loaded, while USB storage worked
  (Aurora R1, E1 run 1). With `OpenUsbKbDxe.efi` + `KeySupport=false` it worked in the picker and the
  UEFI Shell (E1 run 2). So OpenDuet has no USB HID keyboard driver of its own: `OpenUsbKbDxe` is required
  for USB keyboards. Confirmed on one machine only; USB 1.1 (UHCI) ports not yet tested.
- OpenCore 1.0.7 `ocvalidate` rules: `OpenUsbKbDxe` and `Ps2KeyboardDxe` must never coexist;
  `Ps2KeyboardDxe` requires `KeySupport=true`; `OpenUsbKbDxe` replaces `KeySupport` (set it false).
- So a profile must choose one keyboard path. If both a PS/2 and a USB keyboard are present, or the
  keyboard type is UNKNOWN, prefer the path verified on that machine and tell the user which keyboard
  to use. Never ship a key whose input path is unknown as GREEN.

## Disks — READ ONLY

- Partition table: MBR vs GPT (read LBA 0/1 only).
- ESP detection: GPT type `C12A7328-F81F-11D2-BA4B-00A0C93EC93B` or MBR type `0xEF`.
- Filesystems: identify via signatures; mount `ro,noload` (ext4) / `ro` otherwise, or parse without mounting.
- EFI loaders: `\EFI\BOOT\BOOTX64.EFI`, `\EFI\*\*.efi` (shim, grub, Windows `bootmgfw.efi`). Record path + size + hash; do not execute.
- Never fix, flag, resize or "repair" anything.

## Output

The probe produces one versioned JSON report (`schema_version`), privacy-filtered (serial numbers,
UUIDs, MAC addresses, user names in paths are hashed or removed by default — see `docs/SECURITY.md`).

## References

- `references/` — to hold: PCI class code table, golden reference raw reports, chipset notes.
