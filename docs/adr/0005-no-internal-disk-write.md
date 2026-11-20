# 0005 — No internal disk write

Status: Accepted (from the mandate) | Date: 2026-11-19

## Decision

Internal disks are read-only. Block devices are opened `O_RDONLY`; filesystems are mounted `ro`
(ext4 with `noload`); no tool on the key is invoked with a write target other than the identified
Legacy2UEFI key.

## Enforcement

- A single Rust crate (`image-writer`) owns every block-device write; it refuses any device without the Legacy2UEFI identity marker
  and re-reads the marker immediately before writing.
- QEMU tests hash internal disk images before and after each run.
- Review checklist item: "does this open anything for writing?"

## Consequences

No automatic repair, no MBR→GPT, no ESP creation on internal disks. Machines that would need one of
those are UNSUPPORTED, with an explanation shown to the user.
