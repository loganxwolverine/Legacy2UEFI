// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! CPU identification through CPUID: brand string and x86-64 microarchitecture level.

use alloc::string::String;
use core::arch::x86_64::{__cpuid, __cpuid_count};

#[allow(unused_unsafe)]
fn cpuid(leaf: u32, sub: u32) -> (u32, u32, u32, u32) {
    // SAFETY: CPUID is available on every x86-64 CPU; reading it has no side effects.
    let r = unsafe { if sub == 0 { __cpuid(leaf) } else { __cpuid_count(leaf, sub) } };
    (r.eax, r.ebx, r.ecx, r.edx)
}

pub struct CpuInfo {
    pub brand: Option<String>,
    /// 1..=4 for x86-64-v1..v4.
    pub level: u8,
    /// Highest level's first missing feature, for display.
    pub missing_for_next: Option<&'static str>,
}

fn brand() -> Option<String> {
    let (max_ext, ..) = cpuid(0x8000_0000, 0);
    if max_ext < 0x8000_0004 {
        return None;
    }
    let mut bytes = [0u8; 48];
    for (i, leaf) in (0x8000_0002u32..=0x8000_0004).enumerate() {
        let (a, b, c, d) = cpuid(leaf, 0);
        for (j, reg) in [a, b, c, d].iter().enumerate() {
            bytes[i * 16 + j * 4..i * 16 + j * 4 + 4].copy_from_slice(&reg.to_le_bytes());
        }
    }
    let raw = core::str::from_utf8(&bytes).ok()?.trim_matches(char::from(0)).trim();
    // Collapse runs of spaces ("Core(TM) i7 CPU         920").
    let mut out = String::new();
    for word in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    (!out.is_empty()).then_some(out)
}

pub fn read() -> CpuInfo {
    let (max_std, ..) = cpuid(0, 0);
    let (_, _, ecx1, _) = cpuid(1, 0);
    let (max_ext, ..) = cpuid(0x8000_0000, 0);
    let ecx_ext = if max_ext >= 0x8000_0001 { cpuid(0x8000_0001, 0).2 } else { 0 };
    let ebx7 = if max_std >= 7 { cpuid(7, 0).1 } else { 0 };
    let bit = |reg: u32, n: u32| reg & (1 << n) != 0;

    let v2: [(&'static str, bool); 7] = [
        ("SSE3", bit(ecx1, 0)),
        ("SSSE3", bit(ecx1, 9)),
        ("CX16", bit(ecx1, 13)),
        ("SSE4.1", bit(ecx1, 19)),
        ("SSE4.2", bit(ecx1, 20)),
        ("POPCNT", bit(ecx1, 23)),
        ("LAHF", bit(ecx_ext, 0)),
    ];
    let v3: [(&'static str, bool); 9] = [
        ("AVX", bit(ecx1, 28)),
        ("AVX2", bit(ebx7, 5)),
        ("BMI1", bit(ebx7, 3)),
        ("BMI2", bit(ebx7, 8)),
        ("F16C", bit(ecx1, 29)),
        ("FMA", bit(ecx1, 12)),
        ("LZCNT", bit(ecx_ext, 5)),
        ("MOVBE", bit(ecx1, 22)),
        ("OSXSAVE", bit(ecx1, 27)),
    ];
    let v4: [(&'static str, bool); 5] = [
        ("AVX512F", bit(ebx7, 16)),
        ("AVX512DQ", bit(ebx7, 17)),
        ("AVX512CD", bit(ebx7, 28)),
        ("AVX512BW", bit(ebx7, 30)),
        ("AVX512VL", bit(ebx7, 31)),
    ];
    let first_missing = |set: &[(&'static str, bool)]| set.iter().find(|(_, ok)| !ok).map(|(n, _)| *n);

    let (level, missing_for_next) = match first_missing(&v2) {
        Some(m) => (1, Some(m)),
        None => match first_missing(&v3) {
            Some(m) => (2, Some(m)),
            None => match first_missing(&v4) {
                Some(m) => (3, Some(m)),
                None => (4, None),
            },
        },
    };
    CpuInfo { brand: brand(), level, missing_for_next }
}
