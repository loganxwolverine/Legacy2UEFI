# 0009 — Licence: GPL-3.0-or-later

Status: Accepted (owner decision, 2026-11-20) | Date: 2026-11-20

## Context

Before any public release the project needed a licence. The boot screen links Slint 1.19, offered under
GPL-3.0-only, the Slint Royalty-free License 2.0 (excludes embedded systems; a pre-OS firmware application is an
ambiguous case) or a commercial licence. Other shipped components: OpenCorePkg/OpenDuet (BSD-3-Clause, EDK II parts
BSD-2-Clause-Patent), Inter font (OFL-1.1), Rust crates (MIT, Apache-2.0, BSD, Zlib, Unicode-3.0, MPL-2.0).

## Options considered

1. GPL-3.0-or-later — compatible with Slint's GPL option and with every other component; copyleft.
2. MIT / Apache-2.0 — permissive, but needs Slint's royalty-free licence to apply (uncertain) or a commercial one.

## Decision

Option 1: Legacy2UEFI's own code is **GPL-3.0-or-later**. The screen binary, which links Slint under GPL-3.0-only,
is distributed under GPL-3.0.

## Consequences

- `LICENSE` (official GPL-3.0 text from gnu.org, SHA-256 3972dc97…6986), `THIRD-PARTY-NOTICES.md`,
  `docs/licenses/rust-crates-*.txt`, SPDX headers in sources, `license = "GPL-3.0-or-later"` in both crates.
- Every key image carries `LICENSES\` (GPL text, notices, OpenCorePkg BSD text, Inter OFL text, crate list) —
  from 0.1.0-alpha.3.
- Distributing an image implies offering the corresponding source: this repository (to be published).
- Not legal advice; a lawyer should review before a commercial or large-scale distribution.
