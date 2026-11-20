// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Boot side of the launcher: load the key's drivers, find bootable loaders on every volume,
//! and start the chosen one. Everything here reads; nothing writes to any disk.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};
use uefi::boot::{self, LoadImageSource, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol, SearchType};
use uefi::proto::BootPolicy;
use uefi::proto::device_path::build::{self, DevicePathBuilder};
use uefi::proto::device_path::{DevicePath, DeviceSubType, DeviceType};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode, FileSystemVolumeLabel};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::{CString16, Handle};

/// Loader paths checked on every volume, in priority order (first match wins, one entry per volume).
/// Distro shims first; `\EFI\BOOT\BOOTX64.EFI` (removable-media default) last.
const CANDIDATES: &[(&str, &str)] = &[
    ("\\EFI\\fedora\\shimx64.efi", "Fedora / Bazzite"),
    ("\\EFI\\ubuntu\\shimx64.efi", "Ubuntu"),
    ("\\EFI\\debian\\shimx64.efi", "Debian"),
    ("\\EFI\\opensuse\\shim.efi", "openSUSE"),
    ("\\EFI\\rocky\\shimx64.efi", "Rocky Linux"),
    ("\\EFI\\almalinux\\shimx64.efi", "AlmaLinux"),
    ("\\EFI\\redhat\\shimx64.efi", "Red Hat"),
    ("\\EFI\\centos\\shimx64.efi", "CentOS"),
    ("\\EFI\\Microsoft\\Boot\\bootmgfw.efi", "Windows"),
    ("\\EFI\\systemd\\systemd-bootx64.efi", "systemd-boot"),
    ("\\EFI\\GRUB\\grubx64.efi", "GRUB"),
    ("\\EFI\\arch\\grubx64.efi", "Arch Linux"),
    ("\\EFI\\Manjaro\\grubx64.efi", "Manjaro"),
    ("\\EFI\\refind\\refind_x64.efi", "rEFInd"),
    ("\\EFI\\BOOT\\BOOTX64.EFI", ""),
];

// OpenDuet's BDS (Legacy/BootPlatform/BdsDxe/BdsEntry.c) only ever starts `\EFI\OC\OpenCore.efi`, so the
// launcher occupies that path on the key; the real OpenCore (text fallback) lives in `\EFI\OpenCore\`.
pub const SHELL_PATH: &str = "\\EFI\\OpenCore\\Tools\\OpenShell.efi";
pub const OPENCORE_PATH: &str = "\\EFI\\OpenCore\\OpenCore.efi";
const DRIVERS_DIR: &str = "\\EFI\\L2U\\drivers";

static OWN_DEVICE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(core::ptr::null_mut());

pub struct BootEntry {
    pub title: String,
    pub subtitle: String,
    pub removable: bool,
    pub device: Handle,
    pub path: &'static str,
}

fn params(handle: Handle) -> OpenProtocolParams {
    OpenProtocolParams { handle, agent: boot::image_handle(), controller: None }
}

/// The device the launcher itself was loaded from (the Legacy2UEFI key).
pub fn own_device() -> Option<Handle> {
    let p = OWN_DEVICE.load(Ordering::Relaxed);
    if !p.is_null() {
        // SAFETY: stored from a valid handle below.
        return unsafe { Handle::from_ptr(p) };
    }
    let li = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle()).ok()?;
    let dev = li.device()?;
    OWN_DEVICE.store(dev.as_ptr(), Ordering::Relaxed);
    Some(dev)
}

fn open_fs(h: Handle) -> Option<ScopedProtocol<SimpleFileSystem>> {
    // SAFETY: shared access; only reads are performed.
    unsafe { boot::open_protocol::<SimpleFileSystem>(params(h), OpenProtocolAttributes::GetProtocol).ok() }
}

fn device_path(h: Handle) -> Option<ScopedProtocol<DevicePath>> {
    // SAFETY: shared, read-only access.
    unsafe { boot::open_protocol::<DevicePath>(params(h), OpenProtocolAttributes::GetProtocol).ok() }
}

fn file_exists(fs: &mut SimpleFileSystem, path: &str) -> bool {
    let Ok(mut root) = fs.open_volume() else { return false };
    let Ok(p) = CString16::try_from(path) else { return false };
    root.open(&p, FileMode::Read, FileAttribute::empty()).is_ok()
}

/// Bus of a volume from its device path: ("USB" | "SATA" | "NVMe" | "disque", removable).
fn bus_of(h: Handle) -> (&'static str, bool) {
    let Some(dp) = device_path(h) else { return ("disque", false) };
    let mut bus = ("disque", false);
    for node in dp.node_iter() {
        if node.device_type() == DeviceType::MESSAGING {
            match node.sub_type() {
                DeviceSubType::MESSAGING_USB | DeviceSubType::MESSAGING_USB_CLASS | DeviceSubType::MESSAGING_USB_WWID => {
                    return ("USB", true);
                }
                DeviceSubType::MESSAGING_SATA => bus = ("SATA", false),
                DeviceSubType::MESSAGING_NVME_NAMESPACE => bus = ("NVMe", false),
                DeviceSubType::MESSAGING_ATAPI | DeviceSubType::MESSAGING_SCSI => bus = ("disque", false),
                _ => {}
            }
        }
    }
    bus
}

fn volume_label(fs: &mut SimpleFileSystem) -> Option<String> {
    let mut root = fs.open_volume().ok()?;
    let info = root.get_boxed_info::<FileSystemVolumeLabel>().ok()?;
    let l = format!("{}", info.volume_label());
    let l = l.trim().to_string();
    (!l.is_empty() && l != "NO NAME" && l != "EFI").then_some(l)
}

/// Concatenated node bytes (without the end node): used to test "this volume is on that disk".
fn path_bytes(h: Handle) -> Vec<u8> {
    let mut v = Vec::new();
    if let Some(dp) = device_path(h) {
        for n in dp.node_iter() {
            // SAFETY: a node is `length()` contiguous bytes starting at its header.
            let raw = unsafe { core::slice::from_raw_parts(n.as_ffi_ptr().cast::<u8>(), n.length() as usize) };
            v.extend_from_slice(raw);
        }
    }
    v
}

/// Nodes of `h`'s device path up to (excluding) the first node matching `stop`, as a new device path.
fn truncated_path(h: Handle, stop: impl Fn(DeviceType, DeviceSubType) -> bool, keep_match: bool) -> Option<Vec<u8>> {
    let dp = device_path(h)?;
    let mut buf = Vec::new();
    let mut b = DevicePathBuilder::with_vec(&mut buf);
    let mut matched = false;
    for n in dp.node_iter() {
        let hit = stop(n.device_type(), n.sub_type());
        if hit && !keep_match {
            matched = true;
            break;
        }
        b = b.push(&n).ok()?;
        if hit {
            matched = true;
        }
    }
    b.finalize().ok()?;
    matched.then_some(buf)
}

fn find_on_path<P: uefi::proto::ProtocolPointer + ?Sized>(bytes: &[u8]) -> Option<Handle> {
    // SAFETY: `bytes` was produced by DevicePathBuilder::finalize, so it is a valid device path.
    let dp: &DevicePath = unsafe { &*(core::ptr::from_ref::<[u8]>(bytes) as *const DevicePath) };
    let mut p = dp;
    boot::locate_device_path::<P>(&mut p).ok()
}

/// "SanDisk Ultra Fit" for a USB volume, from the device's own string descriptors.
fn usb_product(h: Handle) -> Option<String> {
    // Keep the path up to and including the last USB node.
    let dp = device_path(h)?;
    let last_usb = dp
        .node_iter()
        .enumerate()
        .filter(|(_, n)| n.device_type() == DeviceType::MESSAGING && n.sub_type() == DeviceSubType::MESSAGING_USB)
        .map(|(i, _)| i)
        .last()?;
    let mut buf = Vec::new();
    let mut b = DevicePathBuilder::with_vec(&mut buf);
    for n in dp.node_iter().take(last_usb + 1) {
        b = b.push(&n).ok()?;
    }
    b.finalize().ok()?;
    drop(dp);
    let uh = find_on_path::<uefi::proto::usb::io::UsbIo>(&buf)?;
    // SAFETY: shared access, descriptors are only read.
    let mut io = unsafe { boot::open_protocol::<uefi::proto::usb::io::UsbIo>(params(uh), OpenProtocolAttributes::GetProtocol).ok()? };
    let d = io.device_descriptor().ok()?;
    let lang = *io.supported_languages().ok()?.first()?;
    let mut get = |idx: u8| -> Option<String> {
        if idx == 0 {
            return None;
        }
        let s = io.string_descriptor(lang, idx).ok()?;
        let s = format!("{}", &*s).trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    // Generic manufacturer strings ("USB", "Generic") add nothing next to the product name.
    let m = get(d.str_manufacturer).filter(|m| !matches!(m.to_ascii_lowercase().as_str(), "usb" | "generic" | "usb device"));
    let p = get(d.str_product);
    match (m, p) {
        (Some(m), Some(p)) if p.starts_with(&m) => Some(p),
        (Some(m), Some(p)) => Some(format!("{m} {p}")),
        (m, p) => m.or(p),
    }
}

/// Size of the whole disk that holds volume `h`, in bytes.
fn disk_size(h: Handle) -> Option<u64> {
    let buf = truncated_path(h, |t, _| t == DeviceType::MEDIA, false)?;
    let dh = find_on_path::<uefi::proto::media::block::BlockIO>(&buf)?;
    block_size_of(dh)
}

fn block_size_of(h: Handle) -> Option<u64> {
    // SAFETY: shared access, media info is only read.
    let bio = unsafe {
        boot::open_protocol::<uefi::proto::media::block::BlockIO>(params(h), OpenProtocolAttributes::GetProtocol).ok()?
    };
    let m = bio.media();
    m.is_media_present().then(|| (m.last_block() + 1) * m.block_size() as u64)
}

fn human_size(bytes: u64) -> String {
    let gb = bytes / 1_000_000_000;
    if gb >= 1 { format!("{gb} Go") } else { format!("{} Mo", bytes / 1_000_000) }
}

/// Reads a small text file from the volume (first 16 KiB).
fn read_text(fs: &mut SimpleFileSystem, path: &str) -> Option<String> {
    let mut root = fs.open_volume().ok()?;
    let p = CString16::try_from(path).ok()?;
    let mut f = root.open(&p, FileMode::Read, FileAttribute::empty()).ok()?.into_regular_file()?;
    let mut buf = alloc::vec![0u8; 16 * 1024];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Name of the first entry in a Limine configuration ("/+CachyOS" → "CachyOS").
fn limine_system(fs: &mut SimpleFileSystem) -> Option<String> {
    const PATHS: &[&str] = &[
        "\\limine.conf", "\\boot\\limine\\limine.conf", "\\EFI\\limine\\limine.conf",
        "\\limine\\limine.conf", "\\EFI\\BOOT\\limine.conf",
    ];
    let text = PATHS.iter().find_map(|p| read_text(fs, p))?;
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with('/') && !l.starts_with("//"))
        .map(|l| l.trim_start_matches(['/', '+', '-']).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Name of the installed system from the directories under `\EFI` (e.g. `cachyos`, `limine`).
fn system_from_efi_dirs(fs: &mut SimpleFileSystem) -> Option<String> {
    let mut root = fs.open_volume().ok()?;
    let p = CString16::try_from("\\EFI").ok()?;
    let mut dir = root.open(&p, FileMode::Read, FileAttribute::empty()).ok()?.into_directory()?;
    const KNOWN: &[(&str, &str)] = &[
        ("cachyos", "CachyOS"), ("fedora", "Fedora / Bazzite"), ("bazzite", "Bazzite"), ("nobara", "Nobara"),
        ("ubuntu", "Ubuntu"), ("pop", "Pop!_OS"), ("debian", "Debian"), ("opensuse", "openSUSE"),
        ("arch", "Arch Linux"), ("endeavouros", "EndeavourOS"), ("manjaro", "Manjaro"), ("garuda", "Garuda"),
        ("linuxmint", "Linux Mint"), ("microsoft", "Windows"), ("limine", "Linux (Limine)"),
        ("systemd", "Linux (systemd-boot)"), ("grub", "Linux (GRUB)"),
    ];
    let mut best: Option<(usize, &str)> = None;
    while let Ok(Some(info)) = dir.read_entry_boxed() {
        let info: &FileInfo = &info;
        if !info.attribute().contains(FileAttribute::DIRECTORY) {
            continue;
        }
        let name = format!("{}", info.file_name()).to_ascii_lowercase();
        if let Some(rank) = KNOWN.iter().position(|(k, _)| *k == name) {
            if best.map_or(true, |(r, _)| rank < r) {
                best = Some((rank, KNOWN[rank].1));
            }
        }
    }
    best.map(|(_, n)| n.to_string())
}

/// Every bootable loader on volumes other than the Legacy2UEFI key, internal disks first, followed by
/// disks where nothing bootable was found (`path` empty), so the user sees every storage device.
pub fn scan() -> Vec<BootEntry> {
    let own = own_device();
    let own_bytes = own.map(path_bytes).unwrap_or_default();
    let mut found: Vec<BootEntry> = Vec::new();
    let mut bootable_paths: Vec<Vec<u8>> = Vec::new();
    let handles = boot::find_handles::<SimpleFileSystem>().unwrap_or_default();
    crate::slog!("volumes (SimpleFileSystem): {}", handles.len());
    for h in handles {
        let (bus, removable) = bus_of(h);
        if Some(h) == own {
            crate::slog!("  volume [{bus}] = clé Legacy2UEFI, ignorée");
            continue;
        }
        let Some(mut fs) = open_fs(h) else {
            crate::slog!("  volume [{bus}] : ouverture impossible");
            continue;
        };
        let Some(&(path, name)) = CANDIDATES.iter().find(|(p, _)| file_exists(&mut fs, p)) else {
            crate::slog!("  volume [{bus}] : aucun chargeur connu");
            continue;
        };
        let system = if name.is_empty() {
            limine_system(&mut fs).or_else(|| system_from_efi_dirs(&mut fs))
        } else {
            Some(name.to_string())
        };
        let label = volume_label(&mut fs);
        drop(fs);
        let size = disk_size(h).map(human_size);
        let device = if removable {
            let product = usb_product(h);
            match (product, &size) {
                (Some(p), Some(s)) => format!("Clé USB {p} · {s}"),
                (Some(p), None) => format!("Clé USB {p}"),
                (None, Some(s)) => format!("Clé USB · {s}"),
                (None, None) => "Clé USB".to_string(),
            }
        } else {
            match &size {
                Some(s) => format!("Disque interne {bus} · {s}"),
                None => format!("Disque interne {bus}"),
            }
        };
        let title = system
            .or(label)
            .unwrap_or_else(|| if removable { "Clé USB démarrable".to_string() } else { "Système UEFI".to_string() });
        crate::slog!("  volume [{bus}] : {title} | {device} | {path}");
        bootable_paths.push(path_bytes(h));
        found.push(BootEntry { title, subtitle: device, removable, device: h, path });
    }
    found.sort_by_key(|e| e.removable);

    // Whole disks with nothing bootable (and not the key itself): listed so every device is visible.
    if let Ok(bios) = boot::find_handles::<uefi::proto::media::block::BlockIO>() {
        for h in bios {
            // SAFETY: shared access, media info is only read.
            let Ok(bio) = (unsafe {
                boot::open_protocol::<uefi::proto::media::block::BlockIO>(params(h), OpenProtocolAttributes::GetProtocol)
            }) else {
                continue;
            };
            let m = bio.media();
            if m.is_logical_partition() || !m.is_media_present() {
                continue;
            }
            drop(bio);
            let disk = path_bytes(h);
            if disk.is_empty() || own_bytes.starts_with(&disk) || bootable_paths.iter().any(|v| v.starts_with(&disk)) {
                continue;
            }
            let (bus, removable) = bus_of(h);
            let size = block_size_of(h).map(human_size).unwrap_or_default();
            let what = if removable { format!("Clé USB · {size}") } else { format!("Disque interne {bus} · {size}") };
            crate::slog!("  disque sans système démarrable : {what}");
            found.push(BootEntry {
                title: "Aucun système démarrable".to_string(),
                subtitle: what,
                removable,
                device: h,
                path: "",
            });
        }
    }
    found
}

/// Starts `path` on `device`. Returns when the image exits (or fails to load/start).
pub fn launch(device: Handle, path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("rien à démarrer sur ce périphérique".to_string());
    }
    crate::slog!("démarrage de {path}");
    crate::steplog::flush();
    let dp = device_path(device).ok_or_else(|| "chemin du périphérique introuvable".to_string())?;
    let file = CString16::try_from(path).map_err(|_| "chemin invalide".to_string())?;
    let mut buf = Vec::new();
    let mut b = DevicePathBuilder::with_vec(&mut buf);
    for node in dp.node_iter() {
        b = b.push(&node).map_err(|_| "chemin trop long".to_string())?;
    }
    let full = b
        .push(&build::media::FilePath { path_name: &file })
        .and_then(|b| b.finalize())
        .map_err(|_| "chemin trop long".to_string())?;
    let img = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromDevicePath { device_path: full, boot_policy: BootPolicy::ExactMatch },
    )
    .map_err(|e| format!("chargement de {path} impossible ({:?})", e.status()))?;
    boot::start_image(img).map_err(|e| format!("{path} s'est arrêté avec une erreur ({:?})", e.status()))
}

pub fn launch_own(path: &str) -> Result<(), String> {
    launch(own_device().ok_or_else(|| "clé Legacy2UEFI introuvable".to_string())?, path)
}

/// Loads every `*.efi` in `\EFI\L2U\drivers` from the key (e.g. the USB keyboard driver chosen by the
/// profile, ADR-0006), then connects all controllers so the new drivers bind to their devices.
pub fn load_drivers() -> usize {
    let Some(own) = own_device() else { return 0 };
    let Some(mut fs) = open_fs(own) else { return 0 };
    let mut names: Vec<String> = Vec::new();
    if let Ok(mut root) = fs.open_volume() {
        if let Ok(dir_path) = CString16::try_from(DRIVERS_DIR) {
            if let Ok(dir) = root.open(&dir_path, FileMode::Read, FileAttribute::empty()) {
                if let Some(mut dir) = dir.into_directory() {
                    while let Ok(Some(info)) = dir.read_entry_boxed() {
                        let info: &FileInfo = &info;
                        let name = format!("{}", info.file_name());
                        if !info.attribute().contains(FileAttribute::DIRECTORY) && name.to_ascii_lowercase().ends_with(".efi") {
                            names.push(name);
                        }
                    }
                }
            }
        }
    }
    drop(fs);
    let mut loaded = 0;
    for n in &names {
        if launch(own, &format!("{DRIVERS_DIR}\\{n}")).is_ok() {
            loaded += 1;
        }
    }
    if let Ok(all) = boot::locate_handle_buffer(SearchType::AllHandles) {
        for h in all.iter() {
            let _ = boot::connect_controller(*h, &[], None, true);
        }
    }
    loaded
}
