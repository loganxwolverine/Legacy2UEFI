# Third-party notices

Legacy2UEFI's own code is licensed under **GPL-3.0-or-later** (see [LICENSE](LICENSE)).
The key image also contains, or is built from, the third-party components below, each under its own licence.

## Components shipped on the key

| Component | Where on the key | Licence | Licence text |
|---|---|---|---|
| OpenDuet, OpenCore 1.0.7, OpenRuntime, OpenUsbKbDxe, OpenShell (acidanthera/OpenCorePkg; parts derived from EDK II) | `/boot`, `\EFI\OpenCore\`, `\EFI\L2U\drivers\` | BSD-3-Clause (OpenCorePkg); EDK II parts BSD-2-Clause-Patent | `third_party/opencore/LICENSE.txt` |
| OpenDuet boot sectors `boot0`, `boot1f32` | MBR and partition boot sector | BSD-3-Clause (OpenCorePkg) | same |
| Slint 1.19.0 (UI toolkit, compiled into the Legacy2UEFI screen) | `\EFI\OC\OpenCore.efi`, `\EFI\BOOT\BOOTx64.efi` | **GPL-3.0-only** option of Slint's triple licence | [LICENSE](LICENSE) |
| Inter font (embedded in the screen) | same | SIL Open Font License 1.1 | `crates/launcher-uefi/ui/fonts/Inter-LICENSE.txt` |
| Rust crates compiled into the screen (uefi, uefi-raw, log, libm, euclid, …) | same | MIT, Apache-2.0, BSD-2/3-Clause, Zlib, Unicode-3.0, MPL-2.0 (`ucs2`) — all GPL-3.0-compatible | `docs/licenses/rust-crates-launcher.txt` (name, version, SPDX licence) |

Because Slint is used under GPL-3.0-only, the Legacy2UEFI screen binary as a whole is distributed under GPL-3.0.

## Build-time tools (not shipped on the key)

| Tool | Licence |
|---|---|
| `l2u-image-builder` dependencies (fatfs, fscommon, …) | MIT / Apache-2.0 — see `docs/licenses/rust-crates-image-builder.txt` |
| Rust toolchain, WinLibs MinGW-w64 (build workstation only) | their own licences; nothing from them is shipped |

## Source code

The complete corresponding source of Legacy2UEFI is this repository (`crates/launcher-uefi`, `crates/image-builder`,
`tools/release`). OpenCorePkg sources: https://github.com/acidanthera/OpenCorePkg (release 1.0.7).
Slint sources: https://github.com/slint-ui/slint (1.19.0).
