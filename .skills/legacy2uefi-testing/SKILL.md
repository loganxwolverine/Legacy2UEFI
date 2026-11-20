---
name: legacy2uefi-testing
description: Validation discipline for Legacy2UEFI — virtual tests (QEMU + SeaBIOS), physical tests, compatibility status rules, reference-machine regression (machine named in agent.md), and the automatic regression suite. Use before declaring any test result, changing a compatibility status, or touching the critical boot engine.
metadata:
  version: "0.1.0-draft"
  status: DRAFT
---

# Legacy2UEFI — testing

Status definitions and colours come from `legacy2uefi-domain` §5. This skill says how to earn them.

## Status transitions

```
UNTESTED ──virtual pass──► EXPERIMENTAL ──physical pass + evidence──► VERIFIED
   │                            │                                         │
   └──────── test fails ────────┴────────────► FAILED ◄──── regression ───┘
known blocking capability gap ──► UNSUPPORTED
```

- Never promote on a virtual result alone. Never promote without stored evidence.
- No single agent may set VERIFIED. A human who ran the physical test signs the evidence file.

## Evidence for a physical test (`tests/evidence/<model>/<date>/`)

- probe report (JSON, privacy-filtered)
- Legacy2UEFI version, OpenCore version, profile id + hash of the generated config.plist
- OpenCore log (`opencore-*.txt`, debug build if available)
- the OS that reached its login/desktop, and the boot path used
- photo or serial/console capture of the result (optional but recommended)
- tester name and date

## Virtual tests (QEMU)

- Machine: `qemu-system-x86_64` with default SeaBIOS (BIOS mode, no OVMF), `-machine pc` (i440FX/PIIX) and `-machine q35`.
- Media: the Legacy2UEFI USB image attached as `usb-storage` behind EHCI and xHCI, and as IDE/AHCI disk.
- Targets: an internal disk image with GPT+ESP+Linux, one with MBR, one empty; check the bridge reaches the OpenCore picker, the UEFI Shell, and an OS loader.
- Verify after each run that internal disk images are **byte-identical** (hash before/after) — this is the automated proof of the no-write invariant.
- OpenCorePkg's `Utilities/LegacyBoot/QemuBuild.command` is a reference for building a test image; read it, don't run it blindly.

## Reference-machine regression

The reference machine is the one named in `agent.md` (currently the Alienware Aurora R1; the Dell
OptiPlex 980 is deferred). Any change to probe, compatibility engine, profile engine, OpenCore config
generation or the image build must show that the reference machine's profile still generates a
config.plist **semantically equal** to the committed known-good one (diff with an explanation for every
intended change), and that it still passes `ocvalidate`. Physical re-test on the reference machine
before each release.

## Automatic regression suite (grows from P1)

| Area | Test |
|---|---|
| probe | parsers fed with recorded sysfs/SMBIOS/PCI fixtures from real machines |
| parser | MBR/GPT/FAT32/ext4 signature parsing on crafted + corrupted images (fuzz with cargo-fuzz) |
| profiles | capability set → expected profile, including UNKNOWN inputs → never GREEN |
| OpenCore config | generated plist vs golden; `ocvalidate` (pinned version) exit code 0 |
| EFI discovery | fixture disk images with known loaders |
| logs / privacy | no serial, UUID, MAC or username in exported bundles |
| recovery | interrupted config write leaves a bootable key (old or new config, never neither) |
| UI | TUI starts with no DRM/KMS/framebuffer; GUI failure falls back to TUI |

## Field lessons (P0)

- In the UEFI Shell, `fsN` numbers shift when devices are added. Always identify a volume by its
  device path (`USB(…)` = removable, `Sata(…)`/`NVMe(…)` = internal) before reading it.
- OpenCore RELEASE builds log only WARN/ERROR (`DEBUG_INFO` is DEBUG/NOOPT only): an error-free RELEASE
  boot leaves an all-zero log file. Use the DEBUG build when the log is the evidence.

## Rules

- A failing test is reported with its output, never silently skipped.
- Fixtures from real machines are privacy-filtered before commit.
- Python may generate fixtures; it is never needed on the key.
