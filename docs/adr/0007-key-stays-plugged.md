# 0007 — The key stays plugged in after OS installation

Status: Accepted (owner decision, 2026-11-19) | Date: 2026-11-19

## Context

The primary use case is installing a UEFI-only OS (e.g. Bazzite) on a BIOS-only PC. After installation
the BIOS still cannot start a UEFI loader, so every boot must go through the bridge.

## Options considered

1. The Legacy2UEFI key stays plugged in (or is plugged in at each boot).
2. Install the bridge (OpenDuet boot sectors + OpenCore) on the internal disk.

## Decision

Option 1. Option 2 contradicts ADR-0005 (no internal disk write) and is out of scope until a future ADR
explicitly allows it, with user confirmation and device-identity checks.

## Consequences

- User documentation: "keep the key plugged in; without it the installed OS will not start".
- The bridge must find and launch the installed OS loader on the internal ESP automatically (OpenCore scan).
- The key becomes a permanent boot device: its integrity matters (backup / recovery path, threat T3).
- The OS installer writes to the internal disk; that is the user's explicit action, warned in the UI.
