// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Opérations disque : écriture brute d'une image de release, relecture de vérification,
//! et mise à jour en place du contenu de la clé (sans toucher au MBR ni au PBR — ADR-0003).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

use fscommon::StreamSlice;

use crate::device::{self, Device};
use crate::engine::{
    self, compare_version_str, diff_trees, partition_from_mbr, read_tree, remove_tree, verify_tree,
    write_tree, Diff, KeyState, Payload, KEY_MARKER, PBR_SIG_END, PBR_SIG_START,
};

pub type Progress<'a> = &'a mut dyn FnMut(u32, &str);

pub fn open_rw(path: &str) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("{path} : {e} (droits administrateur nécessaires)"))
}

/// Démonte toutes les partitions montées de ce disque (écrire sur un volume monté le corrompt).
pub fn unmount_all(dev: &Device, progress: Progress) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for (src, target) in device::all_mounts() {
        if !device::is_partition_of(&src, &dev.name) {
            continue;
        }
        progress(0, &format!("démontage de {target}"));
        let out = Command::new("umount")
            .arg(&target)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "démontage de {target} impossible : {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        done.push(target);
    }
    Ok(done)
}

pub fn read_key_state(file: &mut File) -> Result<KeyState, String> {
    let mut mbr = [0u8; 512];
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.read_exact(&mut mbr)
        .map_err(|e| format!("lecture du secteur 0 : {e}"))?;
    let (off, end) = partition_from_mbr(&mbr)?;
    let mut slice = StreamSlice::new(file, off, end).map_err(|e| e.to_string())?;
    let fs = fatfs::FileSystem::new(
        &mut slice,
        fatfs::FsOptions::new().update_accessed_date(false),
    )
    .map_err(|e| format!("FAT32 illisible : {e}"))?;
    Ok(KeyState::from_files(read_tree(&fs)?))
}

/// Écrit l'image de release telle quelle sur la cible (même disposition que `l2u-image-builder`).
pub fn write_image(file: &mut File, payload: &Payload, progress: Progress) -> Result<(), String> {
    progress(0, "écriture de l'image (cela efface le début de la clé)");
    engine::write_image_blocks(file, &payload.bytes, |pct| {
        progress(pct.min(90) * 90 / 100, "")
    })?;
    file.sync_all()
        .map_err(|e| format!("synchronisation : {e}"))?;
    Ok(())
}

fn read_exact_at(file: &mut File, off: u64, len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len];
    file.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    file.read_exact(&mut buf)
        .map_err(|e| format!("lecture à l'offset {off} : {e}"))?;
    Ok(buf)
}

/// Contrôle indépendant : MBR identique à l'image, PBR signé, arborescence relue et comparée.
pub fn verify_image(
    file: &mut File,
    payload: &Payload,
    progress: Progress,
) -> Result<usize, String> {
    progress(92, "vérification du secteur de démarrage");
    let mbr = read_exact_at(file, 0, 512)?;
    if mbr != payload.bytes[..512] {
        return Err("le MBR relu diffère de celui de l'image".into());
    }
    let (off, end) = partition_from_mbr(&mbr)?;
    let pbr = read_exact_at(file, off, 512)?;
    if pbr[510..512] != [0x55, 0xAA] {
        return Err("secteur de démarrage de la partition (boot1f32) non signé".into());
    }
    if pbr[PBR_SIG_START..PBR_SIG_END].iter().all(|&b| b == 0) {
        return Err("signature de volume nulle : la launcher refuserait le démarrage".into());
    }
    progress(96, "relecture des fichiers de la clé");
    let mut slice = StreamSlice::new(file, off, end).map_err(|e| e.to_string())?;
    let fs = fatfs::FileSystem::new(
        &mut slice,
        fatfs::FsOptions::new().update_accessed_date(false),
    )
    .map_err(|e| format!("FAT32 illisible après écriture : {e}"))?;
    let key_files = read_tree(&fs)?;
    let d = diff_trees(&key_files, &payload.files);
    if !d.is_empty() {
        return Err(format!("la clé relue diffère de l'image : {}", d.summary()));
    }
    progress(100, "clé vérifiée");
    Ok(d.same)
}

#[derive(Clone, Debug)]
pub struct UpdateOutcome {
    pub key_before: String,
    pub key_after: String,
    pub diff: Diff,
    pub verified: usize,
}

/// Met à jour le contenu de la clé en place : remplace `EFI/`, `boot`, le marqueur, les licences
/// et le mode d'emploi ; ne touche ni au MBR, ni au PBR, ni aux journaux de la launcher.
///
/// Sûreté : la nouvelle arborescence est écrite sous `EFI.NEW`, relue et comparée **avant** la
/// permutation `EFI → EFI.OLD`, `EFI.NEW → EFI`. Une interruption laisse l'ancienne clé intacte
/// (ou récupérable via `EFI.OLD`, que la fonction restaure automatiquement au prochain passage).
pub fn update_key(
    file: &mut File,
    payload: &Payload,
    force: bool,
    progress: Progress,
) -> Result<UpdateOutcome, String> {
    let mbr = read_exact_at(file, 0, 512)?;
    let (off, end) = partition_from_mbr(&mbr)?;

    let mut efi: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut root_files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (path, data) in &payload.files {
        if let Some(rest) = path.strip_prefix("EFI/") {
            efi.insert(rest.to_string(), data.clone());
        } else {
            root_files.insert(path.clone(), data.clone());
        }
    }
    if !efi.contains_key("OC/OpenCore.efi") {
        return Err(
            "image sans EFI/OC/OpenCore.efi : clé non démarrable, mise à jour refusée".into(),
        );
    }

    let mut verified_total;
    let key_before;
    let diff;
    {
        let mut slice = StreamSlice::new(&mut *file, off, end).map_err(|e| e.to_string())?;
        let fs = fatfs::FileSystem::new(
            &mut slice,
            fatfs::FsOptions::new().update_accessed_date(false),
        )
        .map_err(|e| format!("FAT32 illisible : {e}"))?;
        let root = fs.root_dir();

        // --- identité de la clé -------------------------------------------------
        progress(2, "lecture de la clé");
        let state = KeyState::from_files(read_tree(&fs)?);
        if !state.files.contains_key(KEY_MARKER) {
            return Err(format!(
                "{} n'est pas une clé Legacy2UEFI ({KEY_MARKER} absent) : mise à jour refusée",
                payload.path.display()
            ));
        }
        if !state.is_legacy2uefi() {
            return Err(
                "clé sans chargeur EFI/OC/OpenCore.efi ni /boot : mise à jour refusée".into(),
            );
        }
        key_before = state.version_or_unknown();

        // --- cohérence de version ----------------------------------------------
        if let (Some(k), Some(p)) = (state.version.as_deref(), payload.version.as_deref()) {
            match compare_version_str(p, k) {
                std::cmp::Ordering::Less if !force => {
                    return Err(format!(
                        "l'image est plus ancienne ({p}) que la clé ({k}) : rétrogradation refusée"
                    ))
                }
                std::cmp::Ordering::Equal => {
                    let d = diff_trees(&state.files, &payload.files);
                    if d.is_empty() && !force {
                        return Err(format!("la clé est déjà à jour ({k}) — rien à faire"));
                    }
                }
                _ => {}
            }
        }
        diff = diff_trees(&state.files, &payload.files);

        // --- reprise après interruption ---------------------------------------
        if root.open_dir("EFI").is_err() && root.open_dir("EFI.OLD").is_ok() {
            progress(3, "reprise : restauration de EFI.OLD");
            root.rename("EFI.OLD", &root, "EFI")
                .map_err(|e| format!("reprise EFI.OLD : {e}"))?;
        }

        // --- nouvelle arborescence EFI ---------------------------------------
        progress(5, "écriture de la nouvelle arborescence EFI");
        remove_tree(&root, "EFI.NEW")?;
        let written = write_tree(&root, "EFI.NEW", &efi)?;
        progress(50, &format!("{written} fichier(s) EFI écrits, relecture"));
        verify_tree(&root, "EFI.NEW", &efi)?;

        // --- fichiers de la racine (boot, marqueur, licences, mode d'emploi) ---
        progress(60, "écriture des fichiers de la racine");
        for (name, data) in &root_files {
            let tmp = format!("{name}.new");
            if root.open_file(&tmp).is_ok() {
                root.remove(&tmp).map_err(|e| format!("{tmp}: {e}"))?;
            }
            let mut f = root.create_file(&tmp).map_err(|e| format!("{tmp}: {e}"))?;
            f.truncate().map_err(|e| e.to_string())?;
            f.write_all(data).map_err(|e| format!("{tmp}: {e}"))?;
            f.flush().map_err(|e| e.to_string())?;
        }
        for (name, data) in &root_files {
            let tmp = format!("{name}.new");
            let mut got = Vec::new();
            root.open_file(&tmp)
                .and_then(|mut f| f.read_to_end(&mut got))
                .map_err(|e| format!("{tmp}: relecture impossible ({e})"))?;
            if got != *data {
                return Err(format!("{tmp}: contenu différent après écriture"));
            }
        }

        // --- permutation -------------------------------------------------------
        progress(75, "permutation EFI.NEW → EFI");
        remove_tree(&root, "EFI.OLD")?;
        if root.open_dir("EFI").is_ok() {
            root.rename("EFI", &root, "EFI.OLD")
                .map_err(|e| format!("EFI → EFI.OLD : {e}"))?;
        }
        root.rename("EFI.NEW", &root, "EFI")
            .map_err(|e| format!("EFI.NEW → EFI : {e}"))?;
        for name in root_files.keys() {
            if root.open_file(name).is_ok() {
                root.remove(name).map_err(|e| format!("{name}: {e}"))?;
            }
            root.rename(&format!("{name}.new"), &root, name)
                .map_err(|e| format!("{name}.new → {name} : {e}"))?;
        }
        progress(85, "nettoyage");
        remove_tree(&root, "EFI.OLD")?;

        // --- vérification complète --------------------------------------------
        progress(90, "vérification de la clé mise à jour");
        verified_total = verify_tree(&root, "EFI", &efi)?;
        for (name, data) in &root_files {
            let mut got = Vec::new();
            root.open_file(name)
                .and_then(|mut f| f.read_to_end(&mut got))
                .map_err(|e| format!("{name}: relecture impossible ({e})"))?;
            if got != *data {
                return Err(format!("{name}: contenu final différent"));
            }
            verified_total += 1;
        }
        drop(root);
        fs.unmount().map_err(|e| format!("démontage FAT32 : {e}"))?;
    }

    file.sync_all()
        .map_err(|e| format!("synchronisation : {e}"))?;

    // Le MBR et le PBR n'ont pas été touchés : on le prouve.
    if read_exact_at(file, 0, 512)? != mbr {
        return Err("le MBR a changé pendant la mise à jour (écriture inattendue)".into());
    }
    let pbr = read_exact_at(file, off, 512)?;
    if pbr[510..512] != [0x55, 0xAA] {
        return Err("le secteur de démarrage de la partition n'est plus signé".into());
    }

    let mut after = read_key_state(file)?;
    let key_after = after.version_or_unknown();
    if let Some(p) = payload.version.as_deref() {
        if after.version.as_deref() != Some(p) {
            return Err(format!(
                "la clé annonce {} après mise à jour, l'image {p} : incohérence",
                key_after
            ));
        }
    }
    after.files.clear();

    progress(100, "clé à jour");
    Ok(UpdateOutcome {
        key_before,
        key_after,
        diff,
        verified: verified_total,
    })
}

/// Lit l'état d'une clé **sans privilège** quand une de ses partitions est montée
/// (le bureau monte les clés USB automatiquement).
pub fn state_from_mounts(dev: &Device) -> Option<(KeyState, String)> {
    for target in &dev.mounts {
        let marker = Path::new(target).join(KEY_MARKER);
        if !marker.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&marker).ok()?;
        let mut state = KeyState {
            version: engine::parse_marker(&text).version,
            built: engine::parse_marker(&text).built,
            files: BTreeMap::new(),
        };
        if let Ok(files) = read_dir_tree(Path::new(target)) {
            state.files = files;
        }
        return Some((state, target.clone()));
    }
    None
}

fn read_dir_tree(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let rel = rel.to_string_lossy().replace('\\', "/");
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            if meta.is_dir() {
                walk(root, &path, out)?;
            } else if meta.is_file() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !rel.contains('/') && engine::is_key_log(&name) {
                    continue;
                }
                out.insert(rel, std::fs::read(&path).map_err(|e| e.to_string())?);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

/// Vérifie que la cible est un fichier ordinaire (mode test) ou un disque amovible autorisé.
pub fn check_target(path: &str) -> Result<Option<Device>, String> {
    if Path::new(path).is_file() {
        return Ok(None);
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    if let Some(dev) = device::list_devices().into_iter().find(|d| d.name == name) {
        device::check_writable(&dev)?;
        return Ok(Some(dev));
    }
    // Dérogation explicite, réservée aux tests et à l'intégration continue : un périphérique
    // virtuel (loop) peut être utilisé si l'appelant l'autorise nommément. Elle ne s'applique
    // jamais à un disque réel non amovible, et les contrôles de taille restent appliqués.
    if std::env::var("L2U_KEY_ALLOW_VIRTUAL").as_deref() == Ok("1") {
        let sys = Path::new("/sys/block").join(&name);
        if sys.exists() {
            let mounts = device::all_mounts();
            if let Some(dev) = device::device_from_sysfs(&name, &sys, &mounts) {
                if dev.system {
                    return Err(format!("{name} porte le système : écriture refusée"));
                }
                if dev.size_bytes < engine::MIN_IMAGE_BYTES {
                    return Err(format!(
                        "{name} ne fait que {} : trop petit pour la clé Legacy2UEFI",
                        device::human_size(dev.size_bytes)
                    ));
                }
                return Ok(Some(dev));
            }
        }
    }
    Err(format!("{path} : périphérique inconnu"))
}

#[allow(dead_code)]
pub fn file_size(file: &mut File) -> u64 {
    file.seek(SeekFrom::End(0)).unwrap_or(0)
}

#[allow(dead_code)]
pub fn is_zero_sector(file: &mut File) -> bool {
    let Ok(buf) = read_exact_at(file, 0, 512) else {
        return true;
    };
    buf.iter().all(|&b| b == 0)
}

/// Taille du volume de la partition 1 d'un disque (pour l'affichage).
#[allow(dead_code)]
pub fn partition_end(file: &mut File) -> Option<u64> {
    let mbr = read_exact_at(file, 0, 512).ok()?;
    partition_from_mbr(&mbr).ok().map(|(_, end)| end)
}
