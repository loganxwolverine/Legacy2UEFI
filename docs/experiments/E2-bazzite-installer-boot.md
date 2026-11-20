# E2 — Boot the Bazzite installer through the bridge (P0)

Status: **planned** · Machine: Alienware Aurora R1 · Date: 2026-11-19

## Question

Can a UEFI-only OS installer (Bazzite) start on a BIOS-only PC through BIOS → OpenDuet → OpenCore,
from a **second** USB key, up to the installer's first screen?

## Scope

- **No installation.** Stop at the installer's welcome screen, then power off. The internal disk is not touched.
- Key 1: Legacy2UEFI test key `L2U-E1` (USB keyboard variant, INFO logging).
- Key 2: Bazzite installer USB key, made the standard way (e.g. Fedora Media Writer) from the official ISO.
  Record the Bazzite version and image variant (GPU flavour).

## Protocol

1. Plug both keys and a USB keyboard. Note the ports.
2. F12 → choose **key 1** (`L2U-E1`), not the Bazzite key.
3. In the OpenCore picker, choose the entry of key 2 (EFI loader of the installer: shim/GRUB).
4. Note each stage reached: GRUB menu → kernel start → graphical installer welcome screen.
5. Photo/capture at the last stage reached. Power off without installing.
6. Bring key 1 back: OpenCore log.

## What could fail (and what it would tell us)

| Symptom | Likely area |
|---|---|
| Key 2 not in the picker | OpenDuet USB enumeration with two keys, or ScanPolicy |
| GRUB starts, kernel hangs / black screen | GOP / framebuffer handoff, memory map, ACPI under OpenDuet |
| Installer starts without graphics | GPU driver support of that card in Bazzite (not a bridge issue) |
| CPU refused | CPU instruction-set baseline required by Bazzite (to check, not assumed) |

## Result — 2026-11-20 — **SUCCESS (Live session reached, nothing installed)**

Path that worked: **dd-flashed Bazzite USB key** (not Ventoy).

1. BIOS → `L2U-E1` → OpenDuet → OpenCore picker: `NO NAME (external)` (Bazzite key ESP), `L2U-E1 (external)`,
   `NO NAME` (internal ESP), `UEFI Shell`.
2. Selected `NO NAME (external)` → Bazzite boot splash → **Bazzite Live KDE desktop**, "Install to Hard Drive" icon present.
3. The Live session's own system panel reports:

| Item | Value shown by Bazzite |
|---|---|
| Build / image | `45.20261117.1` / `bazzite:stable` |
| CPU | Intel Core i7 920 @ 2.67 GHz |
| GPU | AMD Ellesmere (Radeon RX 470/480/570/580/590) |
| RAM | 12 GB |
| **UEFI** | **YES** — the Linux kernel runs in UEFI mode on a BIOS-only machine |
| EFI | LIMINE |
| Storage | sda 238.5 G (internal), sdb 3.8 G (L2U-E1), sdc 57.3 G (Bazzite key) |

Ventoy path: the owner reports it did not work (details to collect: where it stopped). Ventoy adds its own
UEFI layer on top of OpenDuet; not needed for the use case, kept as a known limitation to investigate later.

Not yet verified: the install itself, the bootloader step (installers write an EFI boot entry through UEFI
variables — under OpenDuet these are emulated), and booting the installed system afterwards (ADR-0007).

Evidence: `tests/evidence/alienware-aurora-r1/2026-11-20/e2-picker-bazzite-key.png`,
`e2-bazzite-boot-splash.png`, `e2-bazzite-live-desktop.webp`.

Status: Aurora R1 stays **EXPERIMENTAL** (VERIFIED needs an installed OS booting through the bridge).

## Live-session report — 2026-11-20 07:45 UTC (`tools/p0-experiments/linux/l2u-check.sh`)

Full report: `tests/evidence/alienware-aurora-r1/2026-11-20/l2u-report-20261120-074515.txt` (read-only collection).

UEFI as seen by Linux 7.3.4 (Fedora 45 base):
- `efi: EFI v2.7 by Acidanthera`, `fw_platform_size = 64`, `runtime` and `runtime-map` present → 64-bit UEFI with runtime services.
- SMBIOS 2.5 and ACPI tables passed through (`ACPI=0xbf59d000 SMBIOS=0xbf59c000`).
- `efivarfs` mounted **rw**, 28 variables. `efibootmgr`: no Boot entries, no BootOrder. Variable *reads* work;
  *writes* were not tested. Under OpenDuet the NVRAM is emulated: an entry written by the installer may not
  survive a reboot. OpenCore's own scan must therefore find the installed loader (risk for E3, see below).

Hardware (from SMBIOS / PCI / USB):
- `Alienware Aurora / board 04VWF2`, BIOS **A11 (04/06/2010)**, X58 + ICH10R.
- CPU i7-920, 8 logical CPUs. RAM 3 × 4 GiB of 6 slots, 1333 MT/s. SMBIOS reports memory **Type: Other**
  (old BIOS); part numbers are DDR3 kits. → DDR type cannot be read from SMBIOS on this machine.
- Storage: ICH10R SATA in AHCI mode (`8086:3a22`) + Silicon Image SiI 3132 SATA controller (`1095:3132`).
- USB: 6 × UHCI + 2 × EHCI. The USB keyboard (Dell KB212-B) is on a **USB 1.1 (UHCI) bus** under Linux.
- GPU: AMD Ellesmere `1002:67df` (RX 470/480/570/580/590).
- Internal disk `sda` 238.5 G: **sda1 4 G vfat ESP + sda2 234.5 G btrfs** — an existing Linux installation
  (identity to confirm with the owner). This is the picker's internal `NO NAME` entry.

## Risks found for E3 (install)

1. **Boot entry persistence**: installers register their loader in UEFI variables; under OpenDuet these are
   emulated and probably lost at power-off. If the installed loader is reachable only via that entry, the
   installed OS will not appear after reboot. Mitigations to prepare: OpenCore scanning of `\EFI\BOOT\BOOTX64.EFI`,
   an explicit OpenCore entry, or the `OpenLinuxBoot` driver.
2. **shim fallback loop**: Fedora-family installs ship `\EFI\BOOT\BOOTX64.EFI` = shim + `fbx64.efi`, which
   recreates boot entries and resets. With non-persistent NVRAM this could loop. To check before E3.

### Mitigation applied on the key (2026-11-20, untested)

`Misc/BlessOverride` now lists 13 relative loader paths (distro shims first: `\EFI\fedora\shimx64.efi`,
Ubuntu, Debian, openSUSE, Rocky, Alma, RHEL, CentOS; then systemd-boot, `\EFI\GRUB`, Arch, Manjaro, rEFInd).
OpenCore checks them on every scanned volume, so an installed system is found without relying on emulated
NVRAM entries, and the distro shim is launched directly instead of the `\EFI\BOOT` fallback. `ocvalidate` clean.

### Internal installed OS through the bridge (2026-11-20)

Picker entry `NO NAME` (internal ESP, `sda1`) → **Limine boot menu of an installed CachyOS** (`linux-cachyos`,
`linux-cachyos-lts`, Snapshots, EFI fallback; kernel 7.3.4-1-cachyos, auto-boot countdown).
First installed OS bootloader started through the bridge. Limine is not in the BlessOverride list, so OpenCore
reached it through its default scan of that volume. Whether the OS reached its desktop: to be confirmed by the owner.
Evidence: `tests/evidence/alienware-aurora-r1/2026-11-20/e3-internal-cachyos-limine.png`.
Owner, same day: **CachyOS did not reach its desktop — the machine rebooted** after Limine started the kernel.
Not VERIFIED. Bazzite Live (Fedora kernel 7.3.4, also via Limine) booted fine through the same bridge on the
same machine, so the bridge → Linux kernel handoff works. Leading hypothesis (unproven): CPU instruction level.
The i7-920 (Nehalem) is x86-64-v2 (no AVX/AVX2); CachyOS installs x86-64-v3 optimised packages when installed
on a v3 CPU. A disk installed elsewhere would then crash/reboot early on this CPU. Next check: boot the
`linux-cachyos-lts` entry; if it reboots too, the cause is in the installed userspace, not in the kernel or the bridge.
Product consequence: the key should report the CPU's x86-64 level (v1/v2/v3/v4) before the user installs/boots an OS.
Result: `linux-cachyos-lts` **reboots too**. Two different kernels fail the same way while the Fedora/Bazzite
kernel boots through the same bridge → cause is in the installed CachyOS userspace (most likely x86-64-v3 code on
an x86-64-v2 CPU), not in the bridge. Recorded as an OS/CPU incompatibility, not a Legacy2UEFI failure.
