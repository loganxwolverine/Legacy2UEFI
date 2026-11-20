---
name: legacy2uefi-domain
description: Core concepts, invariants and prohibitions of the Legacy2UEFI project (BIOS vs UEFI vs OpenDuet, role of OpenCore, boot architecture, compatibility levels, no-disk-write rule, OpenCore version policy). Read before any Legacy2UEFI work, before the specialised skills. Primary conceptual source; other legacy2uefi-* skills defer to it.
metadata:
  version: "0.1.0-draft"
  status: DRAFT — to be reviewed against the original mandate and golden reference
---

# Legacy2UEFI — domain

## 1. What the project is (one sentence)

> **Legacy2UEFI builds a software UEFI environment on top of a BIOS Legacy boot.**

Wrong, and forbidden in docs, UI, commits: "Legacy2UEFI turns the BIOS into UEFI",
"converts the firmware", "adds UEFI to the motherboard". Nothing in ROM changes. Remove the key
and the machine is exactly as before.

### What it is for (owner vision, 2026-11-19)

- A **universal, self-contained key**: plug it into any BIOS-only x86-64 PC, it adapts to the hardware.
- **No OS required** on the machine. Primary use case: **install a UEFI-only OS** (reference: Bazzite)
  from a second installer USB key through the bridge, then boot that OS through the bridge.
- The key **stays plugged in** afterwards (ADR-0007). No bridge on the internal disk.
- A **modern graphical pre-OS launcher** (ADR-0008, proposed) shows specs and boot choices;
  the OpenCore text picker is the fallback.
- The OS installer, not Legacy2UEFI, writes the internal disk; the UI warns before that step.

## 2. Vocabulary

| Term | Meaning in this project |
|---|---|
| BIOS Legacy | The machine's real firmware. Runs in 16-bit real mode, boots sector 0 (MBR) of the selected device via INT 13h. We never modify it. |
| UEFI | A firmware *interface* (boot services, runtime services, protocols, GOP, Block I/O, FAT ESP, `\EFI\BOOT\BOOTX64.EFI`). Operating systems ship EFI loaders that require it. |
| OpenDuet | OpenCorePkg's DuetPkg build: a UEFI implementation (built on EDK II) loaded *from disk* by a BIOS. Files: `boot0` (MBR code), `boot1f32` (FAT32 PBR code), `bootX64` / `bootIA32` (the environment, copied as `/boot`), plus `-blockio` variants. |
| OpenCore | UEFI boot manager running inside that environment: picker, drivers, ACPI/SMBIOS patching, launches OS EFI loaders. Upstream: acidanthera/OpenCorePkg. |
| Bridge path | The boot chain BIOS → OpenDuet → OpenCore → OS loader. |
| Diagnostics path | A separate boot of a minimal Linux running the Rust engine, TUI and optional GUI. |
| Profile | A versioned, reviewed set of OpenCore settings + drivers chosen from detected capabilities. |
| Reference machine | The machine every change is regression-tested on. Currently the **Alienware Aurora R1** (owner decision 2026-11-19). The Dell OptiPlex 980 named in the mandate is deferred, not dropped. The current reference is always the one named in `agent.md`. |

## 3. Boot architecture (current proposal — ADR-0002)

```
BIOS ─► USB MBR boot0 ─► FAT32 PBR boot1f32 ─► /boot (OpenDuet) ─► \EFI\BOOT\BOOTx64.efi (OpenCore)
                                                                   └─► OS EFI loader (e.g. shim/GRUB, Windows Boot Manager)
```

Diagnostics run in a *separate* boot (Linux cannot hand control back to BIOS real-mode services
to start OpenDuet in the same boot). The engine's decision is persisted on the key, then the user
reboots into the bridge path. OpenCore reads a single `\EFI\OC\config.plist`: switching profile
means the diagnostics side atomically replaces that file **on the key** (with a backup).

## 4. Safety invariants (non-negotiable)

1. **No internal disk write by default** (ADR-0005). Internal disks are read-only, including
   MBR/PBR/GPT/ESP. Mount with `ro` and open block devices `O_RDONLY`.
2. The only write target is the Legacy2UEFI key, identified by an identity marker set at build time,
   re-checked immediately before every write.
3. No firmware flashing, no NVRAM writes to real firmware, no automatic MBR→GPT.
4. UNKNOWN is never shown or treated as compatible.
5. VERIFIED requires a physical test with evidence (see `legacy2uefi-testing`).
6. No runtime download. Third-party binaries are pinned by version and SHA-256.
7. The GUI is optional; the engine and the TUI are mandatory.

## 5. Compatibility levels

| Level | Meaning | Colour |
|---|---|---|
| VERIFIED | Physically tested end-to-end on this exact model (+ firmware range), evidence stored | GREEN |
| EXPERIMENTAL | Expected to work from capabilities or virtual tests; not physically proven | AMBER |
| UNTESTED | Nothing known either way | GREY |
| FAILED | Tested and did not work; keep the evidence | RED |
| UNSUPPORTED | Known blocking capability gap (e.g. no x86-64 for an X64-only profile) | RED |

Virtual success (QEMU/SeaBIOS) can raise UNTESTED to EXPERIMENTAL, never to VERIFIED.

## 6. OS-agnostic support

The bridge launches standard EFI loaders; it must not embed OS-specific hacks in the generic path.
Anything OS-specific (e.g. Bazzite/Fedora shim behaviour) lives in an explicitly named profile layer
and is documented as such. macOS-specific OpenCore features (kexts, SMBIOS spoofing for Apple models)
are out of scope unless an ADR says otherwise.

## 7. OpenCore version policy

- Exactly one OpenCore version is current; it is written in `third_party/opencore/version` and in
  `agent.md` (CI checks they match).
- Upgrades are deliberate: read the changelog, re-run `ocvalidate` on every profile, re-run the
  reference-machine regression, update the ADR if behaviour changed.
- config.plist schema follows the pinned version's `Docs/Sample.plist` / `Configuration.pdf`,
  never a forum template.

## 8. Prohibitions (also in agent.md)

BIOS flashing; internal partition changes; automatic MBR→GPT; VERIFIED without proof; silent
downloads; unreviewed config.plist reuse; brand-based logic where capability detection is possible;
running `BootInstall*.tool` interactively against a device name typed by hand.

## 9. Where knowledge goes

machine-specific → compatibility database · hardware family → `legacy2uefi-hardware` ·
product behaviour → this skill · test method → `legacy2uefi-testing` · UI → `legacy2uefi-ui` ·
decision → `docs/adr/` · current state → `agent.md`.

## References

- `references/` — to hold: excerpts/links of pinned OpenCore docs, the mandate, glossary sources.
