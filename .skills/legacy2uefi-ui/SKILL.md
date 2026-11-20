---
name: legacy2uefi-ui
description: UX rules for Legacy2UEFI's mandatory TUI (Rust + Ratatui + Crossterm) and optional GUI (Rust + Slint, LinuxKMS backend, software renderer). Use for any interface work. Guarantees the UI never compromises diagnosis or boot.
metadata:
  version: "0.1.0-draft"
  status: DRAFT
---

# Legacy2UEFI — UI

The GUI-optional invariant is defined in `legacy2uefi-domain` §4.7 and `agent.md`. This skill applies it.

## Architecture rule

The UI is a *client* of the engine. The engine (probe, compatibility, profiles, config generation,
logs, support bundle) is a library + CLI usable with no UI at all. No business logic in UI crates.

Start order on the diagnostics path:
1. engine runs probe and writes the report + log to the key (no UI needed);
2. GUI is attempted with a timeout; on any failure (no DRM, no KMS, no `/dev/fb*`, Slint init error, panic) →
3. TUI on the Linux console; if even the TUI cannot start → plain text summary on stdout.

## Pre-OS graphical launcher (ADR-0008, proposed)

The first screen the user sees after the BIOS. Its job is to make the break with the legacy BIOS
look obvious, while staying safe:
- No automatic boot (owner, 2026-11-20): the screen waits for the user's choice. No countdown.
- Second action: **boot another USB key to install an OS** (lists bootable removable devices found).
- Also: UEFI Shell, text mode (OpenCore picker), power off / restart.
- Specs panel: machine model, CPU **and its x86-64 level (v1–v4)**, RAM size + type (DDR2/DDR3…) + speed, GPU, storage, bridge status.
  Measured values only; unknown → "unknown" in grey.
- Rendered on GOP with a software renderer; any failure falls back to the OpenCore text picker.
- No internal OS found → the default becomes "Install from USB", and the screen says why.

## Mandatory TUI features (Ratatui + Crossterm)

hardware diagnosis · compatibility view · profile selection · log viewer · "boot the bridge"
(persist choice + reboot) · safe mode · diagnostics · UEFI Shell entry · support bundle export.

## GUI (Slint)

- Backend LinuxKMS, software renderer first; no OpenGL/Vulkan/X11/Wayland requirement.
- Framebuffer `/dev/fbX` fallback: confirm support in the pinned Slint version before relying on it.
- Usable at 1024×768; must still be readable at 800×600 (degrade layout, not text size).
- Animations: short, optional, never blocking an action.

## Interaction

- Keyboard first, every action reachable without a mouse; mouse optional.
- No artificial delays. No countdown that boots something without the user having seen the status.
- Beginner view by default; technical details one key away (e.g. `Tab` → details, `L` → log).

## Semantic colours

| Colour | Meaning |
|---|---|
| GREEN | compatible / VERIFIED |
| AMBER | EXPERIMENTAL |
| RED | UNSUPPORTED / FAILED / blocking error |
| GREY | UNKNOWN / UNTESTED |

- Colour is never the only signal: always a text label (`VERIFIED`, `EXPERIMENTAL`…) — monochrome consoles and colour-blind users.
- The UI must never render UNKNOWN as GREEN, nor round EXPERIMENTAL up to "OK".
- High contrast; test on the Linux VGA console (16 colours) and in monochrome.

## Wording

Say "starts a UEFI environment from this key", never "converts your BIOS to UEFI".
Every destructive-sounding action (writing the key) names the target device by model, size and label.

## Resolution (owner decision, 2026-11-20)

One design for every screen: mode = EDID native if ≤ 1920×1080, else current firmware mode if ≤ 1920×1080,
else the best mode ≤ 1920×1080. The layout is drawn for 1920×1080 and scaled by discrete steps
(1.0 … 0.4) so it fits 800×600 to 1080p. With Slint's software renderer, every font size must be
pre-embedded (`SLINT_FONT_SIZES` in build.rs) and characters outside ASCII must appear in the .slint file.
