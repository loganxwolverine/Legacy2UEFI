// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Énumération et contrôle de sûreté des périphériques de bloc.
//!
//! Règles (agent.md, ADR-0005) : seule une cible **amovible/USB** est acceptée, jamais le disque
//! qui porte le système en cours d'exécution, et jamais un périphérique trop petit pour FAT32.
//! La décision se fonde sur des capacités mesurées (`/sys/block`, `/proc/mounts`), pas sur un nom.

use std::fs;
use std::path::Path;

use crate::engine::MIN_IMAGE_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub model: String,
    pub removable: bool,
    pub usb: bool,
    pub mounts: Vec<String>,
    pub system: bool,
}

impl Device {
    /// Libellé compact pour l'interface (`sdb — 28,9 Gio — USB — /dev/sdb`).
    pub fn is_virtual(&self) -> bool {
        is_virtual_disk(&self.name)
    }

    pub fn label(&self) -> String {
        let mut flags = Vec::new();
        if self.usb {
            flags.push("USB");
        } else if self.removable {
            flags.push("amovible");
        } else {
            flags.push("non amovible");
        }
        if self.system {
            flags.push("SYSTÈME");
        }
        if self.is_virtual() {
            flags.push("VIRTUEL (test)");
        }
        format!(
            "{} — {} — {} — {}",
            self.name,
            human_size(self.size_bytes),
            flags.join(", "),
            self.path
        )
    }

    pub fn detail(&self) -> String {
        let model = if self.model.is_empty() {
            "modèle inconnu".to_string()
        } else {
            self.model.clone()
        };
        let mounts = if self.mounts.is_empty() {
            "non montée".to_string()
        } else {
            format!("montée sur {}", self.mounts.join(", "))
        };
        format!("{model} · {} · {mounts}", human_size(self.size_bytes))
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["o", "Kio", "Mio", "Gio", "Tio"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} o")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Périphériques virtuels ou sans rapport avec une clé USB.
pub fn is_virtual_disk(name: &str) -> bool {
    ["loop", "ram", "zram", "dm-", "md", "nbd", "sr", "fd"]
        .iter()
        .any(|p| name.starts_with(p))
}

/// Dérogation de test : `L2U_KEY_ALLOW_VIRTUAL=1` fait apparaître les périphériques virtuels
/// (loop, ram…) dans la liste. Elle n'est jamais active par défaut, et les périphériques
/// concernés restent marqués « VIRTUEL » dans l'interface.
pub fn allow_virtual() -> bool {
    std::env::var("L2U_KEY_ALLOW_VIRTUAL").as_deref() == Ok("1")
}

/// Vrai si le chemin réel du périphérique passe par un contrôleur USB.
pub fn looks_usb(real_path: &str) -> bool {
    real_path.contains("/usb") || real_path.contains("usb-storage")
}

pub fn parse_mounts(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let src = it.next()?;
            let dst = it.next()?;
            if !src.starts_with("/dev/") {
                return None;
            }
            Some((src.to_string(), dst.to_string()))
        })
        .collect()
}

/// Vrai si `source` est une partition du disque `disk` (`/dev/sdb1` → `sdb`).
pub fn is_partition_of(source: &str, disk: &str) -> bool {
    let Some(rest) = source.strip_prefix("/dev/") else {
        return false;
    };
    if rest.len() <= disk.len() || !rest.starts_with(disk) {
        return false;
    }
    rest[disk.len()..]
        .bytes()
        .all(|b| b.is_ascii_digit() || b == b'p')
        && rest[disk.len()..].bytes().any(|b| b.is_ascii_digit())
}

pub fn system_mounts() -> Vec<(String, String)> {
    let text = fs::read_to_string("/proc/mounts").unwrap_or_default();
    parse_mounts(&text)
        .into_iter()
        .filter(|(_, target)| {
            matches!(
                target.as_str(),
                "/" | "/boot" | "/boot/efi" | "/home" | "/usr" | "/var"
            ) || target.starts_with("/boot/")
        })
        .collect()
}

/// Toutes les montes de type bloc (`/dev/...`).
pub fn all_mounts() -> Vec<(String, String)> {
    let text = fs::read_to_string("/proc/mounts").unwrap_or_default();
    parse_mounts(&text)
}

/// Construit la description d'un disque à partir de son nom et de son entrée sysfs.
pub fn device_from_sysfs(name: &str, sys: &Path, mounts: &[(String, String)]) -> Option<Device> {
    let sectors: u64 = read_trim(&sys.join("size")).and_then(|s| s.parse().ok())?;
    let removable = read_trim(&sys.join("removable")).as_deref() == Some("1");
    let real = fs::canonicalize(sys).unwrap_or_else(|_| sys.to_path_buf());
    let usb = looks_usb(&real.to_string_lossy());
    let model = read_trim(&sys.join("device/model"))
        .filter(|m| !m.is_empty())
        .or_else(|| {
            let v = read_trim(&sys.join("device/vendor")).unwrap_or_default();
            let m = read_trim(&sys.join("device/name")).unwrap_or_default();
            Some(format!("{v} {m}").trim().to_string()).filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    let dev_mounts: Vec<String> = mounts
        .iter()
        .filter(|(src, _)| is_partition_of(src, name))
        .map(|(_, dst)| dst.clone())
        .collect();
    let system_targets = ["/", "/boot", "/boot/efi", "/home", "/usr", "/var"];
    let system = mounts.iter().any(|(src, dst)| {
        is_partition_of(src, name)
            && (system_targets.contains(&dst.as_str()) || dst.starts_with("/boot/"))
    });
    Some(Device {
        name: name.to_string(),
        path: format!("/dev/{name}"),
        size_bytes: sectors.saturating_mul(512),
        model,
        removable,
        usb,
        mounts: dev_mounts,
        system,
    })
}

pub fn list_devices() -> Vec<Device> {
    let mounts = system_mounts();
    read_devices_in(Path::new("/sys/block"), &mounts)
}

pub fn read_devices_in(sysfs: &Path, mounts: &[(String, String)]) -> Vec<Device> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(sysfs) else {
        return out;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_virtual_disk(&name) && !allow_virtual() {
            continue;
        }
        if let Some(d) = device_from_sysfs(&name, &entry.path(), mounts) {
            out.push(d);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Contrôles de sûreté avant toute écriture.
pub fn check_writable(dev: &Device) -> Result<(), String> {
    if dev.system {
        return Err(format!(
            "{} porte le système en cours d'exécution : écriture refusée",
            dev.name
        ));
    }
    if !(dev.usb || dev.removable) && !(allow_virtual() && dev.is_virtual()) {
        return Err(format!(
            "{} n'est pas un périphérique amovible : écriture refusée (garantie « aucun disque interne touché »)",
            dev.name
        ));
    }
    if dev.size_bytes < MIN_IMAGE_BYTES {
        return Err(format!(
            "{} ne fait que {} : trop petit pour la clé Legacy2UEFI",
            dev.name,
            human_size(dev.size_bytes)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 o");
        assert_eq!(human_size(1024), "1.0 Kio");
        assert_eq!(human_size(128 * 1024 * 1024), "128.0 Mio");
        assert_eq!(human_size(32 * 1024 * 1024 * 1024), "32.0 Gio");
    }

    #[test]
    fn virtual_disks_skipped() {
        assert!(is_virtual_disk("loop0"));
        assert!(is_virtual_disk("zram0"));
        assert!(is_virtual_disk("sr0"));
        assert!(is_virtual_disk("dm-1"));
        assert!(!is_virtual_disk("sdb"));
        assert!(!is_virtual_disk("nvme0n1"));
    }

    #[test]
    fn usb_detection() {
        assert!(looks_usb("/sys/devices/pci0000:00/0000:00:14.0/usb1/1-1/1-1:1.0/host4/target4:0:0/4:0:0:0/block/sdb"));
        assert!(!looks_usb(
            "/sys/devices/pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0/block/sda"
        ));
    }

    #[test]
    fn partition_belongs_to_disk() {
        assert!(is_partition_of("/dev/sdb1", "sdb"));
        assert!(is_partition_of("/dev/nvme0n1p2", "nvme0n1"));
        assert!(!is_partition_of("/dev/sdb", "sdb"));
        assert!(!is_partition_of("/dev/sdb1", "sdc"));
        assert!(!is_partition_of("/dev/sdbx1", "sdb"));
        assert!(!is_partition_of("tmpfs", "sdb"));
    }

    #[test]
    fn mounts_parsed() {
        let text = "/dev/vda2 / ext4 rw 0 0\nproc /proc proc rw 0 0\n/dev/sdb1 /run/media/julien/LEGACY2UEFI vfat rw 0 0\n";
        let m = parse_mounts(text);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].1, "/run/media/julien/LEGACY2UEFI");
    }

    fn sysfs_fixture(dir: &Path, name: &str, size: u64, removable: &str, model: &str) {
        let d = dir.join(name);
        fs::create_dir_all(d.join("device")).unwrap();
        fs::write(d.join("size"), format!("{size}\n")).unwrap();
        fs::write(d.join("removable"), format!("{removable}\n")).unwrap();
        fs::write(d.join("device/model"), format!("{model}\n")).unwrap();
    }

    #[test]
    fn device_listing_and_safety() {
        let tmp = std::env::temp_dir().join(format!("l2u-dev-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        sysfs_fixture(&tmp, "sdb", 60_000_000, "1", "USB DISK 3.0");
        sysfs_fixture(&tmp, "vda", 400_000_000, "0", "QEMU HARDDISK");
        sysfs_fixture(&tmp, "loop0", 100_000, "0", "");
        let mounts = vec![
            ("/dev/vda2".to_string(), "/".to_string()),
            ("/dev/sdb1".to_string(), "/run/media/julien/KEY".to_string()),
        ];
        let devs = read_devices_in(&tmp, &mounts);
        assert_eq!(devs.len(), 2, "loop0 doit être ignoré");
        let key = devs.iter().find(|d| d.name == "sdb").unwrap();
        assert!(key.removable && !key.system);
        assert_eq!(key.mounts, vec!["/run/media/julien/KEY"]);
        assert!(check_writable(key).is_ok());
        assert!(key.label().contains("amovible"));
        let sys = devs.iter().find(|d| d.name == "vda").unwrap();
        assert!(sys.system);
        assert!(check_writable(sys).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }
}
