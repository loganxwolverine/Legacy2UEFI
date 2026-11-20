# 0003 — OpenDuet: upstream binaries, pinned, not forked

Status: Proposed | Date: 2026-11-19

## Context

OpenCorePkg release archives (checked: 1.0.7) ship `Utilities/LegacyBoot/` — `boot0`, `boot1f32`,
`bootX64`, `bootIA32`, `bootX64-blockio`, `bootIA32-blockio`, `BootInstall*.tool`,
`BootInstallBase.sh`, `QemuBuild.command` — and `Utilities/ocvalidate/` (Linux, macOS, Windows).
The mandate forbids rewriting OpenDuet/OpenCore.

## Decision

Consume upstream release artefacts only, pinned by version + SHA-256 in `third_party/opencore/`.

Do **not** use `BootInstall*.tool` in the product. `BootInstallBase.sh` asks for a device name typed
by hand, `dd`s `boot0` into that disk's MBR, rewrites the chosen partition's boot sector, and may set
the active flag with `fdisk`. One typo targets an internal disk. Legacy2UEFI performs the same three
writes (446 bytes of MBR code, PBR with the BPB preserved, the `/boot` file) in Rust, **only** on an
image file or on a device that carries the Legacy2UEFI identity marker.

Evaluate during P0/P1 when to use `bootX64` versus `bootX64-blockio` (disk access through BIOS INT 13h),
based on capabilities.

## Consequences

Building OpenCorePkg from source for reproducibility is a later option; until then upstream release
hashes are trusted and recorded.
