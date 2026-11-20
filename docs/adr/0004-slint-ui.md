# 0004 — UI: Ratatui TUI mandatory, Slint GUI optional

Status: Proposed | Date: 2026-11-19

## Decision

TUI: Ratatui + Crossterm on the Linux console. GUI: Slint with the LinuxKMS backend (no seat
manager) and the software renderer. A supervisor starts the GUI with a timeout; any failure falls
back to the TUI (see `.skills/legacy2uefi-ui`).

## To verify before Accepted

- Runtime dependencies of Slint LinuxKMS in the initramfs (libinput, libudev, xkbcommon, DRM dumb
  buffers) and the resulting image size.
- Whether the pinned Slint version falls back to `/dev/fb*` when DRM/KMS is unavailable.
- Software rendering speed at 1024×768 on a Nehalem-class CPU (reference machine, Aurora R1).
- Licence: Slint is offered under GPLv3, a royalty-free licence or a commercial licence; confirm
  compatibility with Legacy2UEFI's own licence (not chosen yet).
