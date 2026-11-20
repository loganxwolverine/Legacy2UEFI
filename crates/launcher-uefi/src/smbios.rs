// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Minimal read-only SMBIOS parser (2.x and 3.x entry points) for the specs panel.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use uefi::table::cfg::ConfigTableEntry;

#[derive(Default)]
pub struct Dimm {
    /// None = size unknown.
    pub size_mb: Option<u32>,
    pub populated: bool,
    pub mem_type: u8,
    pub speed_mts: Option<u16>,
}

#[derive(Default)]
pub struct SmbiosInfo {
    #[allow(dead_code)] // kept for the diagnostics report
    pub version: String,
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
    pub sys_manufacturer: Option<String>,
    pub sys_product: Option<String>,
    pub board_manufacturer: Option<String>,
    pub board_product: Option<String>,
    pub dimms: Vec<Dimm>,
}

pub fn mem_type_name(t: u8) -> Option<&'static str> {
    Some(match t {
        0x0F => "SDRAM",
        0x12 => "DDR",
        0x13 => "DDR2",
        0x14 => "DDR2 FB-DIMM",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => return None, // 0x01 Other, 0x02 Unknown, …: not a usable answer
    })
}

/// Locates the SMBIOS structure table. Returns (table bytes, "major.minor").
fn locate() -> Option<(&'static [u8], String)> {
    uefi::system::with_config_table(|tables| {
        let find = |g| tables.iter().find(|t| t.guid == g).map(|t| t.address as *const u8);
        // SAFETY: entry points and tables are firmware-provided, identity-mapped during boot services.
        unsafe {
            if let Some(ep) = find(ConfigTableEntry::SMBIOS3_GUID) {
                let e = core::slice::from_raw_parts(ep, 0x18);
                if &e[0..5] == b"_SM3_" {
                    let len = u32::from_le_bytes([e[0x0C], e[0x0D], e[0x0E], e[0x0F]]) as usize;
                    let addr = u64::from_le_bytes(e[0x10..0x18].try_into().ok()?) as *const u8;
                    let ver = alloc::format!("{}.{}", e[7], e[8]);
                    return Some((core::slice::from_raw_parts(addr, len), ver));
                }
            }
            if let Some(ep) = find(ConfigTableEntry::SMBIOS_GUID) {
                let e = core::slice::from_raw_parts(ep, 0x1F);
                if &e[0..4] == b"_SM_" {
                    let len = u16::from_le_bytes([e[0x16], e[0x17]]) as usize;
                    let addr = u32::from_le_bytes([e[0x18], e[0x19], e[0x1A], e[0x1B]]) as usize as *const u8;
                    let ver = alloc::format!("{}.{}", e[6], e[7]);
                    return Some((core::slice::from_raw_parts(addr, len), ver));
                }
            }
        }
        None
    })
}

fn get_str(strings: &[&[u8]], idx: u8) -> Option<String> {
    if idx == 0 {
        return None;
    }
    let s = strings.get(idx as usize - 1)?;
    let s = String::from_utf8_lossy(s);
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

pub fn read() -> Option<SmbiosInfo> {
    let (table, version) = locate()?;
    let mut info = SmbiosInfo { version, ..Default::default() };
    let mut p = 0usize;
    while p + 4 <= table.len() {
        let t = table[p];
        let l = table[p + 1] as usize;
        if l < 4 || p + l > table.len() {
            break;
        }
        let f = &table[p..p + l];
        // String-set: NUL-separated, ends with a double NUL.
        let mut q = p + l;
        let mut strings: Vec<&[u8]> = Vec::new();
        let mut start = q;
        while q + 1 < table.len() {
            if table[q] == 0 {
                if q > start {
                    strings.push(&table[start..q]);
                }
                if table[q + 1] == 0 {
                    q += 2;
                    break;
                }
                start = q + 1;
            }
            q += 1;
        }
        let byte = |o: usize| f.get(o).copied();
        let word = |o: usize| Some(u16::from_le_bytes([byte(o)?, byte(o + 1)?]));
        match t {
            0 => {
                info.bios_vendor = byte(4).and_then(|i| get_str(&strings, i));
                info.bios_version = byte(5).and_then(|i| get_str(&strings, i));
                info.bios_date = byte(8).and_then(|i| get_str(&strings, i));
            }
            1 => {
                info.sys_manufacturer = byte(4).and_then(|i| get_str(&strings, i));
                info.sys_product = byte(5).and_then(|i| get_str(&strings, i));
            }
            2 => {
                info.board_manufacturer = byte(4).and_then(|i| get_str(&strings, i));
                info.board_product = byte(5).and_then(|i| get_str(&strings, i));
            }
            17 => {
                let raw = word(0x0C).unwrap_or(0xFFFF);
                let size_mb = match raw {
                    0 => Some(0),
                    0xFFFF => None,
                    0x7FFF => f.get(0x1C..0x20).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & 0x7FFF_FFFF),
                    s if s & 0x8000 != 0 => Some(((s & 0x7FFF) as u32) / 1024),
                    s => Some(s as u32),
                };
                info.dimms.push(Dimm {
                    populated: size_mb != Some(0),
                    size_mb,
                    mem_type: byte(0x12).unwrap_or(0x02),
                    speed_mts: word(0x15).filter(|&s| s != 0 && s != 0xFFFF),
                });
            }
            127 => break,
            _ => {}
        }
        p = q;
    }
    Some(info)
}
