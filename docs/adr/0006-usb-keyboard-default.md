# 0006 — USB keyboard by default, PS/2 as an automatic fallback

Status: Accepted — USB keyboard REQUIRED, PS/2 dropped (owner, 2026-11-20) | Date: 2026-11-19

## Context

E1 run 1 (Alienware Aurora R1): the OpenCore picker was reached but a USB keyboard did not respond
with only `Ps2KeyboardDxe` loaded. OpenCore 1.0.7 `ocvalidate` forbids loading `OpenUsbKbDxe` and
`Ps2KeyboardDxe` together, so one key configuration supports one keyboard type.

A keyboard choice cannot be asked in the bridge: if the wrong driver is loaded, the user cannot answer.

## Options considered

1. Ask the user in the OpenCore picker — impossible with a dead keyboard.
2. PS/2 by default — PS/2 ports and keyboards are the rarer case today.
3. USB by default, PS/2 as a fallback selected without keyboard interaction.

## Decision

Option 3.
- Default profile: `OpenUsbKbDxe.efi`, `UEFI/Input/KeySupport = false`.
- Minimum requirement stated in user documentation: **a USB keyboard**.
- PS/2 variant (`Ps2KeyboardDxe.efi`, `KeySupport = true`) chosen only automatically: by the
  diagnostics Linux, which sees both keyboard types through the BIOS/kernel, or at key build time.
- Never both drivers in one configuration.

## Condition

Valid only if E1b shows a USB keyboard working under OpenDuet with `OpenUsbKbDxe` (picker + UEFI Shell).
If it fails — for example USB 1.1 (UHCI/OHCI) keyboards not driven by OpenDuet — this ADR returns to Proposed.

## Consequences

- The profile engine must detect the keyboard type (see `legacy2uefi-hardware`, Keyboard section).
- Test protocol: record which keyboard and which USB port/controller was used on every run.

## Owner decision (2026-11-20): USB keyboard required, no PS/2

The owner keeps USB only: a USB keyboard is a **requirement** of Legacy2UEFI (documented for users). PS/2 support,
including the automatic PS/2 fallback described above, is dropped. Verified on two machines with a USB keyboard
(Alienware Aurora R1: USB 1.1/UHCI port; Lenovo ThinkPad 4414WBF: external USB keyboard).
Consequence: on laptops, the built-in keyboard (usually PS/2-attached) is not expected to work — use a USB keyboard.
