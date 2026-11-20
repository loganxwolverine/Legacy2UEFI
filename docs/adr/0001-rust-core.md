# 0001 — Rust for the engine

Status: Proposed | Date: 2026-11-19

## Context

The engine runs in a minimal Linux (initramfs) on old x86 machines, parses untrusted on-disk
structures, and must never corrupt data. The mandate recommends Rust.

## Options considered

1. Rust, static `x86_64-unknown-linux-musl` binaries.
2. C: smallest footprint, but memory-unsafe parsers of untrusted disks.
3. Go: single static binary, but GC, larger binaries, weaker fit for a future `no_std` UEFI path.
4. Shell + Python: fast to prototype, but Python on the key is forbidden and shell engines do not scale.

## Decision

Option 1. One Cargo workspace. Engine crates are libraries with a CLI front-end; UI crates depend on
them, never the reverse.

## Rationale

Memory safety for parsers; static binaries with no runtime; `core::arch` CPUID; mature crates for
GPT/MBR, plist, TUI and GUI; the `uefi` crate keeps a door open for a `no_std` probe (ADR-0002 option C).

## Consequences

- Pin the toolchain (`rust-toolchain.toml`), target musl, set a binary size budget per crate.
- i686 (32-bit-only CPUs) is out of scope for P1; revisit only with a measured need.
- Every C dependency needs a written justification (mandate §0A).
