# E4 — Omarchy installed and booted through the 0.1.0-alpha.2 image (ThinkPad)

Status: **SUCCESS — 2026-11-20** (owner report: "Tout semble ok") · Machine: Lenovo ThinkPad 4414WBF
(BIOS 6AET58WW 2009, Core 2 Duo T6670 = x86-64-v1, 3 GB DDR2, 1024×768 GOP, internal SATA 500 GB)

## What was done (by the owner)

1. Key written with **balenaEtcher** from `legacy2uefi-0.1.0-alpha.2.zip` (first test of the alpha.2 image itself).
2. Booted the ThinkPad from the key → Legacy2UEFI screen → Omarchy installer USB key → **Omarchy installed** on the
   internal 500 GB SATA disk (owner's choice; installer writes the disk, not Legacy2UEFI).
3. Rebooted from the Legacy2UEFI key → selected the installed system → **Omarchy booted. Owner: "Tout semble ok."**

## What it proves

- The release image (not a hand-assembled test key) works end to end: BIOS → OpenDuet → Legacy2UEFI screen →
  installer → installed OS.
- Second machine, second distribution (Arch-based Omarchy after Fedora-based Bazzite), on an x86-64-v1 CPU and a
  4:3 1024×768 display.
- The owner did not report the "BOOT MISMATCH!" message seen with alpha.1 (fix in alpha.2).

## Evidence

Owner's report in the session (no screenshot). Launcher log `L2U-LAUNCHER-LOG.txt` on the key: to copy next time
the key is plugged into the workstation.
