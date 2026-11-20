// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Builds a bootable Legacy2UEFI key image (raw `.img`) from a staged key tree.
//!
//! Layout: MBR (OpenDuet `boot0` code + one active FAT32 LBA partition at 1 MiB) → FAT32 volume whose
//! boot sector carries OpenDuet `boot1f32` (BPB preserved) → files copied from the stage directory
//! (`/boot` = OpenDuet, `\EFI\OC\OpenCore.efi` = Legacy2UEFI screen, …).
//! Same three writes as OpenCorePkg `BootInstallBase.sh`, but into a file only (ADR-0003, ADR-0005):
//! the user writes the image to a key with a standard flasher (e.g. balenaEtcher).
//!
//! Usage: l2u-image-builder --stage DIR --legacyboot DIR --out FILE.img [--size-mib 128] [--label LEGACY2UEFI]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SECTOR: u64 = 512;
const PART_START_LBA: u64 = 2048; // 1 MiB alignment
const DISK_SIGNATURE: u32 = 0x4555_324C; // "L2UE"

struct Args {
    stage: PathBuf,
    legacyboot: PathBuf,
    out: PathBuf,
    size_mib: u64,
    label: String,
}

fn parse_args() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let (mut stage, mut legacyboot, mut out) = (None, None, None);
    let (mut size_mib, mut label) = (128u64, String::from("LEGACY2UEFI"));
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or_else(|| format!("missing value for {k}"));
        match k.as_str() {
            "--stage" => stage = Some(PathBuf::from(v()?)),
            "--legacyboot" => legacyboot = Some(PathBuf::from(v()?)),
            "--out" => out = Some(PathBuf::from(v()?)),
            "--size-mib" => size_mib = v()?.parse().map_err(|_| "bad --size-mib")?,
            "--label" => label = v()?,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if label.len() > 11 || !label.is_ascii() {
        return Err("label must be ASCII, at most 11 characters".into());
    }
    if size_mib < 64 {
        return Err("--size-mib must be at least 64 (FAT32 minimum cluster count)".into());
    }
    Ok(Args {
        stage: stage.ok_or("--stage is required")?,
        legacyboot: legacyboot.ok_or("--legacyboot is required")?,
        out: out.ok_or("--out is required")?,
        size_mib,
        label,
    })
}

fn read_512(path: &Path) -> Result<[u8; 512], String> {
    let v = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    v.try_into().map_err(|_| format!("{} must be exactly 512 bytes", path.display()))
}

/// Copies `src` (a directory on the host) into `dst` (a FAT directory), recursively.
fn copy_tree<IO: fatfs::ReadWriteSeek>(src: &Path, dst: &fatfs::Dir<IO>, count: &mut u64) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(src)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().into_string().map_err(|_| io::Error::other("non-UTF-8 file name"))?;
        if e.file_type()?.is_dir() {
            let sub = dst.create_dir(&name)?;
            copy_tree(&e.path(), &sub, count)?;
        } else {
            let data = fs::read(e.path())?;
            let mut f = dst.create_file(&name)?;
            f.truncate()?;
            f.write_all(&data)?;
            *count += 1;
        }
    }
    Ok(())
}

/// Reads every file back from the image and compares it with the stage tree.
fn verify_tree<IO: fatfs::ReadWriteSeek>(src: &Path, dst: &fatfs::Dir<IO>, rel: &str, checked: &mut u64) -> Result<(), String> {
    for e in fs::read_dir(src).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().into_string().map_err(|_| "non-UTF-8 name")?;
        let path = format!("{rel}/{name}");
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            let sub = dst.open_dir(&name).map_err(|err| format!("{path}: {err}"))?;
            verify_tree(&e.path(), &sub, &path, checked)?;
        } else {
            let want = fs::read(e.path()).map_err(|e| e.to_string())?;
            let mut got = Vec::new();
            dst.open_file(&name)
                .and_then(|mut f| f.read_to_end(&mut got))
                .map_err(|err| format!("{path}: {err}"))?;
            if got != want {
                return Err(format!("{path}: content differs after writing"));
            }
            *checked += 1;
        }
    }
    Ok(())
}

/// 14 non-zero bytes, different for every image build (xorshift seeded from the clock).
fn volume_signature() -> [u8; 14] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x4C32_5545_4649_0001);
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    let mut out = [0u8; 14];
    for b in &mut out {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x >> 24) as u8 | 1; // never zero
    }
    out
}

fn run() -> Result<(), String> {
    let a = parse_args()?;
    let boot0 = read_512(&a.legacyboot.join("boot0"))?;
    let boot1 = read_512(&a.legacyboot.join("boot1f32"))?;
    if boot0[510..512] != [0x55, 0xAA] && boot1[510..512] != [0x55, 0xAA] {
        return Err("boot0/boot1f32 do not look like boot sectors".into());
    }
    if !a.stage.join("boot").is_file() || !a.stage.join("EFI").join("OC").join("OpenCore.efi").is_file() {
        return Err("stage must contain /boot (OpenDuet) and \\EFI\\OC\\OpenCore.efi".into());
    }

    let total_sectors = a.size_mib * 1024 * 1024 / SECTOR;
    let part_sectors = total_sectors - PART_START_LBA;
    let part_start = PART_START_LBA * SECTOR;
    let part_end = total_sectors * SECTOR;

    let mut img = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&a.out)
        .map_err(|e| format!("{}: {e}", a.out.display()))?;
    img.set_len(part_end).map_err(|e| e.to_string())?;

    // 1. MBR: OpenDuet boot0 code (446 bytes, as upstream BootInstall) + one active FAT32-LBA partition.
    let mut mbr = [0u8; 512];
    mbr[..446].copy_from_slice(&boot0[..446]);
    if mbr[440..444] == [0, 0, 0, 0] {
        mbr[440..444].copy_from_slice(&DISK_SIGNATURE.to_le_bytes());
    }
    let p = &mut mbr[446..462];
    p[0] = 0x80; // active
    p[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS start: use LBA
    p[4] = 0x0C; // FAT32 LBA
    p[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS end: use LBA
    p[8..12].copy_from_slice(&(PART_START_LBA as u32).to_le_bytes());
    p[12..16].copy_from_slice(&(part_sectors as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    img.seek(SeekFrom::Start(0)).and_then(|_| img.write_all(&mbr)).map_err(|e| e.to_string())?;

    // 2. FAT32 volume in the partition, then files.
    let mut label = [b' '; 11];
    label[..a.label.len()].copy_from_slice(a.label.as_bytes());
    let mut count = 0;
    {
        let mut slice = fscommon::StreamSlice::new(&mut img, part_start, part_end).map_err(|e| e.to_string())?;
        fatfs::format_volume(
            &mut slice,
            fatfs::FormatVolumeOptions::new()
                .fat_type(fatfs::FatType::Fat32)
                .bytes_per_cluster(1024)
                .volume_label(label),
        )
        .map_err(|e| format!("format: {e}"))?;
        let fsys = fatfs::FileSystem::new(&mut slice, fatfs::FsOptions::new()).map_err(|e| format!("mount: {e}"))?;
        copy_tree(&a.stage, &fsys.root_dir(), &mut count).map_err(|e| format!("copy: {e}"))?;
        fsys.unmount().map_err(|e| format!("unmount: {e}"))?;
    }

    // 3. Volume boot sector: OpenDuet boot1f32 with the formatted volume's OEM name + BPB (bytes 3..90).
    let mut pbr = [0u8; 512];
    img.seek(SeekFrom::Start(part_start)).and_then(|_| img.read_exact(&mut pbr)).map_err(|e| e.to_string())?;
    let mut new_pbr = boot1;
    new_pbr[3..90].copy_from_slice(&pbr[3..90]);
    // Bytes 496..509: volume signature. OpenDuet's BDS (BdsCheckSignature) compares them with the copy of
    // boot1 in memory and rejects an all-zero signature ("BOOT MISMATCH!" + 3 s + slower second pass).
    // Upstream BootInstall writes random bytes; here: 14 bytes derived from the build time, none zero.
    let sig = volume_signature();
    new_pbr[496..510].copy_from_slice(&sig);
    img.seek(SeekFrom::Start(part_start)).and_then(|_| img.write_all(&new_pbr)).map_err(|e| e.to_string())?;
    img.flush().map_err(|e| e.to_string())?;
    drop(img);

    // 4. Verification by reading the image back.
    let mut img = File::open(&a.out).map_err(|e| e.to_string())?;
    let mut s0 = [0u8; 512];
    img.read_exact(&mut s0).map_err(|e| e.to_string())?;
    if s0[..446] != mbr[..446] || s0[446] != 0x80 || s0[450] != 0x0C {
        return Err("MBR check failed".into());
    }
    let mut checked = 0;
    {
        let mut img_rw = OpenOptions::new().read(true).write(true).open(&a.out).map_err(|e| e.to_string())?;
        let mut slice = fscommon::StreamSlice::new(&mut img_rw, part_start, part_end).map_err(|e| e.to_string())?;
        let fsys = fatfs::FileSystem::new(&mut slice, fatfs::FsOptions::new().update_accessed_date(false))
            .map_err(|e| format!("re-mount: {e}"))?;
        if fsys.fat_type() != fatfs::FatType::Fat32 {
            return Err("volume is not FAT32".into());
        }
        verify_tree(&a.stage, &fsys.root_dir(), "", &mut checked)?;
    }
    let mut chk = [0u8; 512];
    img.seek(SeekFrom::Start(part_start)).and_then(|_| img.read_exact(&mut chk)).map_err(|e| e.to_string())?;
    if chk[90..496] != boot1[90..496] || chk[510..512] != [0x55, 0xAA] || chk[496..510].iter().all(|&b| b == 0) {
        return Err("volume boot sector check failed".into());
    }

    println!("image: {} ({} MiB)", a.out.display(), a.size_mib);
    println!("partition: LBA {PART_START_LBA}, {part_sectors} sectors, FAT32, label {}", a.label);
    println!("files written: {count}, verified by read-back: {checked}");
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
