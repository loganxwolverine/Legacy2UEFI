# agent.md — Legacy2UEFI operational memory

> Single source of truth for the short-term state of the project.
> `AGENTS.md` and `CLAUDE.md` only point here. Never copy content into them.
> Read order before any intervention: this file → mandate → `.skills/legacy2uefi-domain` →
> the specialised skill → relevant ADRs → code → tests. The repository is authoritative, not
> any agent's conversational memory.

<!-- machine-checked keys: keep the format `KEY: value` (see tools/check-agent-md.sh) -->
PROJECT_PHASE: P0 — Architecture / feasibility
LEGACY2UEFI_VERSION: 0.1.0-alpha.3 (alpha.2 + licences on the key; alpha.2 image verified on the ThinkPad)
OPENCORE_VERSION: 1.0.7 RELEASE
REQUIRED_SKILLS: rust-best-practices, security-threat-model, uml-and-software-architecture-visualization, legacy2uefi-domain, legacy2uefi-hardware, legacy2uefi-testing, legacy2uefi-ui

## Project mission

Legacy2UEFI is a **universal, self-contained USB key** that gives a BIOS-only x86 PC a software UEFI
environment, adapting to the hardware it finds — without modifying the firmware and without
Legacy2UEFI itself writing to internal disks.

- **No operating system required.** The internal disk may be empty.
- **Primary use case:** install a UEFI-only OS (reference: Bazzite) on a BIOS-only PC, from a second
  installer USB key, through the bridge. Afterwards, boot that installed OS through the bridge.
- **The key stays plugged in** after installation: the BIOS still cannot boot UEFI by itself (ADR-0007).
- **Modern pre-OS experience:** a graphical launcher shows the machine's specs and the boot choices,
  breaking with the legacy BIOS look (ADR-0008, proposed). Text picker remains the fallback.
- The OS installer writes to the internal disk, not Legacy2UEFI; the user is warned before that step.

## Core principle

```
BIOS Legacy  →  software UEFI bridge (OpenDuet)  →  Legacy2UEFI boot screen (OpenCore as fallback)  →  OS EFI loader
```

Legacy2UEFI builds a software UEFI environment **on top of** a BIOS Legacy boot.
It does **not** transform, flash or replace the firmware.

## Scope rule (owner, 2026-11-20)

All effort goes to the key itself: making a BIOS-only PC get a working UEFI environment, universally.
What the user does around it (erasing or installing disks, which OS, what data) is the user's
responsibility — agents do not ask about it or plan it. In scope: whether the key finds and boots
whatever UEFI loader is present.

## Invariant — UI

The graphical interface is optional.
The compatibility engine is mandatory.
A graphical failure must NEVER prevent Legacy2UEFI from diagnosing or booting a supported machine.

## Safety invariants

1. NO INTERNAL DISK WRITE BY DEFAULT. Internal disks are opened read-only. Any future write
   path needs an ADR, explicit user confirmation and device-identity re-verification.
2. The only writable target is the Legacy2UEFI USB key, identified by a stable identity
   (e.g. partition GUID / label written at build time), never by `/dev/sdX` name alone.
3. Never report UNKNOWN as compatible. Unknown is GREY, never GREEN.
4. No hardware is VERIFIED without a real physical test with stored evidence.
5. No binary is downloaded at runtime. All third-party binaries are pinned (version + SHA-256)
   in `third_party/`.

## Reference machines

- **Primary test machine (active): Alienware Aurora R1** — owner decision 2026-11-19. All P0/P1
  experiments target it first. Current evidence: E1 (bridge → UEFI Shell with USB keyboard).
- **Dell OptiPlex 980 — deferred.** Historical known-good machine from the mandate; the owner will test
  it later when space allows. Do not plan work around it, do not block any phase on it. Its known-good
  key, if imported later, goes to `profiles/golden/dell-optiplex-980/`.
- Other legacy machines may be added by the owner; each gets its own evidence folder.

## Current stack (proposed, see ADR-0001 / ADR-0004)

- Rust (engine, probe, profiles, OpenCore config generation, TUI, GUI)
- POSIX shell (initramfs hooks, build, image assembly, CI)
- Ratatui + Crossterm (mandatory fallback TUI)
- Slint, LinuxKMS backend, software renderer (optional GUI)
- OpenDuet (OpenCorePkg `Utilities/LegacyBoot`)
- OpenCore (version pinned in `third_party/opencore/version`)
- Python: dev tooling / tests only, never on the key at runtime

## Current boot flow (proposed, ADR-0002 — status PROPOSED)

```
BIOS → USB MBR (boot0) → FAT32 PBR (boot1f32) → /boot (OpenDuet) → \EFI\OC\OpenCore.efi = Legacy2UEFI screen
     (OpenDuet only ever starts that path; real OpenCore moved to \EFI\OpenCore\ as text fallback)
     → OS EFI loader on internal disk (read-only use) or on the key
Diagnostics path (separate boot entry, separate boot): BIOS → Linux minimal → probe → TUI/GUI
     → writes ONLY to the USB key's config area → reboot into the bridge path
```

## Skills available

| Skill | Kind | Status |
|---|---|---|
| rust-best-practices | external (apollographql/skills) | installed for the user's Claude Code, not vendored |
| security-threat-model | external (openai/skills) | installed for the user's Claude Code, not vendored |
| uml-and-software-architecture-visualization | external (openai/plugins) | installed for the user's Claude Code, not vendored |
| legacy2uefi-domain | project (`.skills/`) | DRAFT v0.1 |
| legacy2uefi-hardware | project (`.skills/`) | DRAFT v0.1 |
| legacy2uefi-testing | project (`.skills/`) | DRAFT v0.1 |
| legacy2uefi-ui | project (`.skills/`) | DRAFT v0.1 |

Details, sources and commits: `docs/SKILLS.md`.

## Verified hardware

- **Alienware Aurora R1** (BIOS A11 2010, X58, i7-920) — VERIFIED 2026-11-20, scope: Bazzite installed through the key and
  booted from the internal SATA disk through the Legacy2UEFI boot screen (v0.3.1, OpenDuet + OpenCore 1.0.7 files,
  USB keyboard). Physical test run and confirmed by the owner ("Tout est ok"). Evidence: E3, `tests/evidence/alienware-aurora-r1/`.
- **Lenovo ThinkPad 4414WBF** (BIOS 6AET58WW 2009, Core 2 Duo T6670, x86-64-v1) — VERIFIED 2026-11-20, scope: Omarchy
  installed and booted through the **0.1.0-alpha.2 image** (written with balenaEtcher), 1024×768, USB keyboard.
  Confirmed by the owner ("Tout semble ok"). Evidence: E4, `tests/evidence/lenovo-thinkpad-4414wbf/`.

## Experimental hardware

_None._

## Active architectural decisions

| ADR | Title | Status |
|---|---|---|
| 0001 | Rust core | Proposed |
| 0002 | Two-stage bootstrap (bridge path / diagnostics path) | Proposed |
| 0003 | OpenDuet strategy: upstream binaries, pinned | Proposed |
| 0004 | Slint LinuxKMS + software renderer, TUI mandatory | Proposed |
| 0005 | No internal disk write | Accepted |
| 0006 | USB keyboard REQUIRED (OpenUsbKbDxe); PS/2 not supported (owner, 2026-11-20) | Accepted |
| 0007 | Key stays plugged after OS install; no bridge on internal disk | Accepted |
| 0008 | Graphical boot menu = key's BOOTx64.efi (Rust, Slint, 1080p); OpenCore = text fallback | Proposed (revised 2026-11-20) |
| 0009 | Licence: GPL-3.0-or-later (Slint used under GPL-3.0-only) | Accepted |

## Forbidden actions

- BIOS / firmware flashing of any kind
- Modifying partitions, MBR, PBR or boot sectors of internal disks
- Automatic MBR→GPT conversion
- Running OpenCorePkg `BootInstall*.tool` (free-text device selection); boot sectors are written
  only by the `image-writer` crate, on an image file or an identified Legacy2UEFI key (ADR-0003)
- Unverified compatibility claims (VERIFIED without physical evidence)
- Silent binary downloads (build or runtime)
- Reusing a third-party config.plist without a reviewed, documented diff
- Brand-based decisions where a capability check is possible ("detect capabilities, not brands")

## Required tests before commit

P0 (now): `sh tools/check-agent-md.sh` must pass.
From P1: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `ocvalidate` on every
generated config.plist, reference-machine regression fixture (Aurora R1 profile).

## Known issues

- Original mandate (v1.0) is not in the repository yet → add it as `docs/MANDATE.md`.
- Ventoy keys did not boot through the bridge (details not yet collected); dd-flashed installer keys work.
- Development machine used for this P0 has no Rust toolchain, no QEMU, no WSL.

## Experiments in progress

- **E1** — OpenDuet + OpenCore 1.0.7 smoke test key (`L2U-E1`, serial <test-key serial>).
  Aurora R1 run 1: picker reached, USB keyboard dead (Ps2KeyboardDxe only).
  Aurora R1 run 2 (OpenUsbKbDxe, KeySupport=false): **USB keyboard works, UEFI Shell reached**, USB key on
  EHCI and internal SATA/GPT disk visible. Run-1/2 logs empty (DisplayLevel); INFO logging now on the key.
  See `docs/experiments/E1-openduet-smoke-test.md`.
- **E2** — **success 2026-11-20**: dd-flashed Bazzite key booted through the bridge to the Live desktop;
  Bazzite reports `UEFI: YES`. Nothing installed. Live report: 64-bit UEFI 2.7 with runtime services,
  efivars readable (no Boot entries), SMBIOS 2.5 passed through. See `docs/experiments/E2-bazzite-installer-boot.md`.

- **E3** — **success 2026-11-20**: Bazzite installed through the key, then booted from the internal disk through the
  Legacy2UEFI screen (Aurora R1 VERIFIED). See `docs/experiments/E3-bazzite-install-and-boot.md`.

## Next safe action

1. alpha.2 image VERIFIED on the ThinkPad (E4, Omarchy). Aurora R1 re-test with the image: skipped by the owner (2026-11-20).
2. Check the OpenCore text fallback started from `\EFI\OpenCore\` (finds its config.plist?).
3. Licence GPL-3.0-or-later (ADR-0009). Source published 2026-11-20: https://github.com/loganxwolverine/Legacy2UEFI
   (public, branch main). Next: attach the release image to a GitHub Release when the owner decides.
4. Add the original mandate as `docs/MANDATE.md` and review the four DRAFT skills against it.
5. Optional: OpenCore DEBUG build for real OpenCore logs (RELEASE logs nothing on an error-free boot).

## Notes and journal (Obsidian)

The project folder is also the owner's Obsidian vault (home note: `Legacy2UEFI.md`). `journal/YYYY-MM-DD.md`
holds dated history in French for the owner, with screenshots. The journal is history only: current state
lives here, decisions in `docs/adr/`, results in `docs/experiments/`. Add a journal entry after each work session.

## Last updated

2026-11-20 — 0.1.0-alpha.2 image: Omarchy installed and booted on the ThinkPad (2 machines VERIFIED).
