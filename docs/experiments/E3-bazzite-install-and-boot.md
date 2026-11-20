# E3 — Install Bazzite through the key, then boot it through the key

Status: **SUCCESS — 2026-11-20** · Machine: Alienware Aurora R1 (BIOS A11 2010, X58, i7-920, 12 GB, Radeon RX 4xx/5xx)

## What was done (by the owner)

1. Booted the L2U-E1 key (OpenDuet → Legacy2UEFI boot screen v0.3.1), with the dd-flashed Bazzite USB key plugged in.
2. Started the Bazzite live session through the bridge and ran "Install to Hard Drive" on the internal 256 GB SATA disk
   (the previous CachyOS installation was replaced — owner's choice, outside Legacy2UEFI's scope).
3. Rebooted with only the L2U-E1 key: the Legacy2UEFI screen listed **"Fedora / Bazzite · Disque interne SATA · 256 Go"**
   (found through `\EFI\fedora\shimx64.efi`, no NVRAM boot entry needed).
4. Selected it: **the installed Bazzite booted. Owner: "Démarrage à partir de Bazzite installé sur le disque dur. Tout est ok."**

## What it proves

- A UEFI-only OS can be **installed** on a BIOS-only PC through Legacy2UEFI (the installer's bootloader step
  succeeded under OpenDuet's emulated UEFI environment).
- The installed OS is found by the boot screen's own scan (BlessOverride-style paths) and **boots through the key**,
  as designed in ADR-0007 (key stays plugged in).
- No automatic boot: the screen waited for the user's choice (owner decision, ADR-0008).

## Evidence (`tests/evidence/alienware-aurora-r1/2026-11-20/`)

- `e3-l2u-screen-bazzite-installed.png` — boot screen after installation, Bazzite on the internal SATA disk.
- `e3-bazzite-desktop.webp` — Bazzite session (`UEFI: YES`).
- `e3-opencore-menu-before.png`, `e3-opencore-menu-after.png` — OpenCore text menu (fallback) before/after.
- Launcher log of these runs: to copy from the key (`L2U-LAUNCHER-LOG.txt`) at the next opportunity.

## Not covered

Secure Boot (not available under OpenDuet), sleep/hibernate, Bazzite updates changing the bootloader, other machines.
