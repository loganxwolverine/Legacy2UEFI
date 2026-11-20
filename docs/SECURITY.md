# Legacy2UEFI — threat model (initial, DRAFT)

Method: `security-threat-model` skill, adapted to a pre-code repository. There is no code yet, so
every component below is **planned**, not observed; the model must be re-run on real code at the
end of P1 and after every scope extension (mandate §35.1 B).

## Assumptions (to confirm — they drive the ranking)

1. Used by one person on hardware they own; physical access is assumed for the user.
2. The key is built on a trusted workstation from pinned artefacts; there is no network at runtime.
3. No Secure Boot exists on target machines (BIOS-only); the bridge cannot provide a hardware root of trust.
4. Internal disks may hold valuable personal data and an existing OS that must keep booting without the key.
5. Support bundles may be shared publicly (forums, GitHub issues).

## Assets

| Asset | Why it matters |
|---|---|
| Internal disk data and boot sectors | Loss = user's data or existing OS destroyed — highest impact |
| Integrity of the key's boot chain (boot0, boot1f32, /boot, OpenCore, drivers, config.plist) | Code running before the OS with full hardware access |
| Build pipeline and pinned third-party binaries | Supply chain |
| Personal data in reports and logs (serials, UUIDs, MACs, user names, hostnames, partition labels) | Privacy when bundles are shared |
| Compatibility status (VERIFIED etc.) | Wrong claims make users try unsafe setups |

## Trust boundaries

1. Upstream OpenCorePkg release → our `third_party/` (download, hash pinning).
2. Build workstation → image file → **physical device being written** (device selection).
3. Engine → internal disks (read-only parsing of untrusted structures).
4. Engine → key ESP (the only write path: config, logs, reports).
5. OpenCore → OS loaders found on internal disks (executes code it did not build).
6. Key → outside world (support bundle export).

## Threats (abuse paths and failure paths)

| # | Threat | Boundary | Likelihood | Impact | Priority |
|---|---|---|---|---|---|
| T1 | Writer targets the wrong device (typo, `/dev/sdX` renumbering, key unplugged/replugged) → internal disk MBR/PBR overwritten. Upstream `BootInstallBase.sh` asks for a free-text device name and `dd`s to it. | 2 | Medium | High | **Critical** |
| T2 | A bug opens an internal disk read-write (mount without `ro`, ext4 journal replay, fsck invoked by the init system, auto-mount). | 3 | Medium | High | **Critical** |
| T3 | Interrupted write (power loss, key pulled) leaves the key with no valid `config.plist` → unbootable bridge and no recovery. | 4 | Medium | Medium | High |
| T4 | Tampered or substituted OpenDuet/OpenCore binary (compromised download, mirror, maintainer mistake). | 1 | Low | High | High |
| T5 | Malicious EFI loader on an internal disk or a second USB device is launched by OpenCore with no verification (no Secure Boot). | 5 | Low | High | Medium (inherent; disclose it) |
| T6 | Crafted partition table / filesystem crashes or exploits the probe parsers. | 3 | Low | Medium | Medium |
| T7 | Support bundle leaks serials, MACs, UUIDs, user names, hostnames. | 6 | High | Low–Medium | High |
| T8 | Booting an OS through OpenCore changes its view of hardware (ACPI/SMBIOS patches) and corrupts the OS install or hibernation image. | 5 | Low | Medium | Medium |
| T9 | Unverified compatibility claim (agent or contributor marks VERIFIED without proof) → users with untested hardware trust a GREEN screen. | process | Medium | Medium | High |
| T10 | Third-party config.plist copied from a forum enables risky quirks or unknown drivers. | 1/process | Medium | Medium | Medium |

Non-capabilities assumed: no remote attacker (no network service on the key); no attacker with
firmware write access (out of scope: they already own the machine).

## Mitigations (all planned — none exist yet)

| Threat | Mitigation | Where |
|---|---|---|
| T1 | Device identity marker set at image build (partition GUID + label + marker file); writer refuses anything else; re-check just before writing; show model/size/label; never call `BootInstall*.tool`; image-file-first workflow (write an image, flash with a tool that lists removable devices only). | `image-writer` crate, ADR-0003 |
| T2 | `O_RDONLY` everywhere outside the writer module; `ro,noload` mounts; no fsck/auto-mount in initramfs; QEMU before/after hashes of internal disk images in CI. | ADR-0005, testing skill |
| T3 | Write new config to temp file, fsync, rename; keep `config.plist.bak` and a minimal safe profile; recovery from the diagnostics path. | `recovery` crate |
| T4 | SHA-256 of every artefact in `third_party/opencore/`; build fails on mismatch; record upstream release URL; later, reproducible build from source. | build, CI |
| T5 | Document clearly: no Secure Boot under the bridge. Show the path and hash of the loader to be launched; option to restrict OpenCore scan policy to known ESPs. | docs, profile defaults |
| T6 | Rust parsers, bounds-checked, fuzzed (cargo-fuzz) on MBR/GPT/FAT/ext4 headers. | `hardware-probe`, `efi-discovery` |
| T7 | Privacy filter by default (hash or drop serials, UUIDs, MACs, user names, hostnames); preview before export; test in regression suite. | `logging` crate |
| T8 | Generic profiles avoid ACPI/SMBIOS patching unless a capability requires it; macOS-style spoofing out of scope. | profile spec |
| T9 | Evidence directory required for VERIFIED; CI check; only a human signs. | testing skill, CI |
| T10 | Configs only generated from reviewed profiles; diff + `ocvalidate` on every change. | `opencore-config` |

## Open questions for the owner

1. Will support bundles be posted publicly, or only sent privately? (changes T7 priority)
2. Must the key also be usable to *install* an OS (writes to internal disks by the OS installer, outside our control)? This should be stated as out of our write invariant but in the user warning.
3. Is a Linux-built key (on the user's Linux machine) the only supported build host, or also Windows?
