// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Cœur du gestionnaire de clé : lecture d'une image de release, comparaison de versions,
//! état de la clé, calcul du différentiel. Aucune dépendance à l'interface ni au privilège.
//!
//! Disposition d'une image Legacy2UEFI (voir `crates/image-builder`) : MBR (`boot0` OpenDuet +
//! une partition FAT32-LBA active à 1 Mio) → volume FAT32 dont le secteur 0 porte `boot1f32`.
//! L'identité de la clé est le fichier `LEGACY2UEFI-KEY.txt` (version + date de build).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

pub const SECTOR: u64 = 512;
pub const KEY_MARKER: &str = "LEGACY2UEFI-KEY.txt";
pub const MIN_IMAGE_BYTES: u64 = 48 * 1024 * 1024;
/// Secteur de démarrage du volume : `boot1f32` + BPB. Octets 496..510 = signature de volume.
pub const PBR_SIG_START: usize = 496;
pub const PBR_SIG_END: usize = 510;

/// Fichiers que l'outil ne réécrit jamais : journaux produits par la launcher sur la clé.
pub fn is_key_log(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "l2u-launcher-log.txt" || (n.starts_with("opencore-") && n.ends_with(".txt"))
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub raw: String,
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Identifiants de pré-version (`alpha.3` → `["alpha", "3"]`), vide pour une version finale.
    pub pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let raw = s.trim().trim_start_matches('v').trim();
        if raw.is_empty() {
            return None;
        }
        let core_and_pre = raw.split('+').next().unwrap_or(raw);
        let mut it = core_and_pre.splitn(2, '-');
        let core = it.next().unwrap_or("");
        let pre: Vec<String> = match it.next() {
            Some(p) => p.split('.').map(|x| x.to_string()).collect(),
            None => Vec::new(),
        };
        let nums: Vec<&str> = core.split('.').collect();
        if nums.len() != 3 {
            return None;
        }
        let mut n = [0u64; 3];
        for (i, part) in nums.iter().enumerate() {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            n[i] = part.parse().ok()?;
        }
        Some(Version {
            raw: raw.to_string(),
            major: n[0],
            minor: n[1],
            patch: n[2],
            pre,
        })
    }

    fn core(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

/// Compare deux chaînes de version au sens de SemVer (0.1.0-alpha.3 < 0.1.0-alpha.10 < 0.1.0).
/// Une chaîne non analysable est considérée inférieure à toute version analysable.
pub fn compare_version_str(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (Version::parse(a), Version::parse(b)) {
        (Some(x), Some(y)) => cmp_version(&x, &y),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => a.cmp(b),
    }
}

pub fn cmp_version(a: &Version, b: &Version) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    a.core()
        .cmp(&b.core())
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (x, y) in a.pre.iter().zip(b.pre.iter()) {
                    let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                        (Ok(nx), Ok(ny)) => nx.cmp(&ny),
                        // Un identifiant numérique est toujours inférieur à un identifiant texte.
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => x.cmp(y),
                    };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                a.pre.len().cmp(&b.pre.len())
            }
        })
}

// ---------------------------------------------------------------------------
// Marqueur de clé
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marker {
    pub version: Option<String>,
    pub built: Option<String>,
}

pub fn parse_marker(text: &str) -> Marker {
    let mut m = Marker::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim().to_string();
        if v.is_empty() {
            continue;
        }
        match k.trim().to_ascii_lowercase().as_str() {
            "version" if m.version.is_none() => m.version = Some(v),
            "built" if m.built.is_none() => m.built = Some(v),
            _ => {}
        }
    }
    m
}

#[allow(dead_code)]
pub fn render_marker(version: &str, built: &str) -> String {
    format!("LEGACY2UEFI key\nversion: {version}\nbuilt: {built}\n")
}

// ---------------------------------------------------------------------------
// Table de partitions
// ---------------------------------------------------------------------------

/// (début, fin) en octets de la première partition, d'après le MBR.
pub fn partition_from_mbr(mbr: &[u8]) -> Result<(u64, u64), String> {
    if mbr.len() < 512 {
        return Err("secteur 0 trop court".into());
    }
    if mbr[510..512] != [0x55, 0xAA] {
        return Err("secteur 0 sans signature 0x55AA : ce n'est pas un disque de démarrage".into());
    }
    let p = &mbr[446..462];
    let kind = p[4];
    if !matches!(kind, 0x0B | 0x0C | 0x06) {
        return Err(format!(
            "partition 1 de type 0x{kind:02X} — FAT32 (0x0C) attendu"
        ));
    }
    let start = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as u64;
    let count = u32::from_le_bytes([p[12], p[13], p[14], p[15]]) as u64;
    if start == 0 || count == 0 {
        return Err("partition 1 vide".into());
    }
    Ok((start * SECTOR, (start + count) * SECTOR))
}

// ---------------------------------------------------------------------------
// Arborescence FAT32
// ---------------------------------------------------------------------------

fn walk<IO: fatfs::ReadWriteSeek>(
    dir: &fatfs::Dir<IO>,
    prefix: &str,
    out: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let mut entries: Vec<fatfs::DirEntry<IO>> = dir
        .iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("lecture du dossier {prefix}: {e}"))?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        // `Dir::iter` renvoie aussi `.` et `..` : les ignorer, sinon récursion infinie.
        if name == "." || name == ".." {
            continue;
        }
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if e.is_dir() {
            let sub = dir
                .open_dir(&name)
                .map_err(|err| format!("{path}: {err}"))?;
            walk(&sub, &path, out)?;
        } else {
            let mut f = dir
                .open_file(&name)
                .map_err(|err| format!("{path}: {err}"))?;
            let mut data = Vec::new();
            f.read_to_end(&mut data)
                .map_err(|err| format!("{path}: {err}"))?;
            out.insert(path, data);
        }
    }
    Ok(())
}

/// Lit tous les fichiers du volume (hors journaux de la launcher et dossiers de travail).
pub fn read_tree<IO: fatfs::ReadWriteSeek>(
    fs: &fatfs::FileSystem<IO>,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut all = BTreeMap::new();
    walk(&fs.root_dir(), "", &mut all)?;
    all.retain(|path, _| {
        if path.contains('/') {
            !path.starts_with("EFI.NEW/") && !path.starts_with("EFI.OLD/")
        } else {
            !is_key_log(path)
        }
    });
    Ok(all)
}

fn write_one<IO: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<IO>,
    path: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut f = root.create_file(path).map_err(|e| format!("{path}: {e}"))?;
    f.truncate().map_err(|e| format!("{path}: {e}"))?;
    f.write_all(data).map_err(|e| format!("{path}: {e}"))?;
    f.flush().map_err(|e| format!("{path}: {e}"))?;
    Ok(())
}

/// Crée `path` et ses parents sous `root` (chemins relatifs `/`).
fn ensure_dir<IO: fatfs::ReadWriteSeek>(root: &fatfs::Dir<IO>, path: &str) -> Result<(), String> {
    let mut acc = String::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(part);
        if root.open_dir(&acc).is_err() {
            root.create_dir(&acc).map_err(|e| format!("{acc}: {e}"))?;
        }
    }
    Ok(())
}

/// Crée `dir_path` sous `root` (si besoin) et y écrit `files` (chemins relatifs `/`).
pub fn write_tree<IO: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<IO>,
    dir_path: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<usize, String> {
    ensure_dir(root, dir_path)?;
    let mut count = 0;
    for (rel, data) in files {
        let full = if dir_path.is_empty() {
            rel.clone()
        } else {
            format!("{dir_path}/{rel}")
        };
        if let Some((parent, _)) = full.rsplit_once('/') {
            ensure_dir(root, parent)?;
        }
        write_one(root, &full, data)?;
        count += 1;
    }
    Ok(count)
}

pub fn remove_tree<IO: fatfs::ReadWriteSeek>(
    dir: &fatfs::Dir<IO>,
    name: &str,
) -> Result<(), String> {
    match dir.open_dir(name) {
        Ok(sub) => {
            let names: Vec<String> = sub
                .iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{name}: {e}"))?
                .into_iter()
                .map(|e| e.file_name())
                .filter(|n| n != "." && n != "..")
                .collect();
            for child in names {
                let child_path = format!("{name}/{child}");
                let is_dir = sub.open_dir(&child).is_ok();
                if is_dir {
                    remove_tree(&sub, &child)?;
                } else {
                    sub.remove(&child)
                        .map_err(|e| format!("{child_path}: {e}"))?;
                }
            }
            dir.remove(name).map_err(|e| format!("{name}: {e}"))
        }
        Err(_) => match dir.remove(name) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{name}: {e}")),
        },
    }
}

/// Relit chaque fichier de `files` sous `dir_path` et compare son contenu octet par octet.
pub fn verify_tree<IO: fatfs::ReadWriteSeek>(
    root: &fatfs::Dir<IO>,
    dir_path: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<usize, String> {
    let mut checked = 0;
    for (rel, want) in files {
        let full = if dir_path.is_empty() {
            rel.clone()
        } else {
            format!("{dir_path}/{rel}")
        };
        let mut got = Vec::new();
        root.open_file(&full)
            .and_then(|mut f| f.read_to_end(&mut got))
            .map_err(|e| format!("{full}: relecture impossible ({e})"))?;
        if got.len() != want.len() {
            return Err(format!(
                "{full}: taille {} au lieu de {}",
                got.len(),
                want.len()
            ));
        }
        if got != *want {
            return Err(format!("{full}: contenu différent après écriture"));
        }
        checked += 1;
    }
    Ok(checked)
}

// ---------------------------------------------------------------------------
// Payload = image de release
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Payload {
    pub path: PathBuf,
    pub version: Option<String>,
    pub built: Option<String>,
    /// Octets bruts de l'image (utilisés tels quels pour la création de la clé).
    pub bytes: Vec<u8>,
    /// Fichiers de la partition, hors journaux (clés relatives, `/`).
    pub files: BTreeMap<String, Vec<u8>>,
    /// Message sur la vérification d'intégrité, s'il y en a une.
    pub integrity: Option<String>,
}

impl Payload {
    pub fn load(path: &Path) -> Result<Payload, String> {
        let mut bytes = read_image_bytes(path)?;
        if (bytes.len() as u64) < MIN_IMAGE_BYTES {
            return Err(format!(
                "{} : image de {} octets, trop petite pour une clé Legacy2UEFI ({} Mio minimum)",
                path.display(),
                bytes.len(),
                MIN_IMAGE_BYTES / 1024 / 1024
            ));
        }
        let (off, end) = partition_from_mbr(&bytes[..512])?;
        if off % SECTOR != 0 || end > bytes.len() as u64 || off >= end {
            return Err(format!(
                "{} : table de partitions incohérente",
                path.display()
            ));
        }
        let files = {
            let part = &mut bytes[off as usize..end as usize];
            let mut cur = Cursor::new(&mut part[..]);
            let fs = fatfs::FileSystem::new(&mut cur, fatfs::FsOptions::new())
                .map_err(|e| format!("{} : FAT32 illisible ({e})", path.display()))?;
            read_tree(&fs)?
        };
        let marker = files
            .get(KEY_MARKER)
            .map(|d| parse_marker(&String::from_utf8_lossy(d)))
            .unwrap_or_default();
        if marker.version.is_none() {
            return Err(format!(
                "{} : marqueur {KEY_MARKER} absent ou sans version — ce n'est pas une image Legacy2UEFI",
                path.display()
            ));
        }
        if !files.contains_key("EFI/OC/OpenCore.efi") {
            return Err(format!(
                "{} : EFI/OC/OpenCore.efi absent de l'image",
                path.display()
            ));
        }
        let integrity = verify_sums(path, &bytes);
        Ok(Payload {
            path: path.to_path_buf(),
            version: marker.version,
            built: marker.built,
            bytes,
            files,
            integrity,
        })
    }

    pub fn version_or_unknown(&self) -> String {
        self.version.clone().unwrap_or_else(|| "inconnue".into())
    }
}

fn read_image_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "img" | "raw" => fs::read(path).map_err(|e| format!("{} : {e}", path.display())),
        "zip" => {
            let f = fs::File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
            let mut z = zip::ZipArchive::new(f)
                .map_err(|e| format!("{} : ZIP illisible ({e})", path.display()))?;
            let mut best: Option<(u64, usize)> = None;
            for i in 0..z.len() {
                let name = z
                    .by_index(i)
                    .map(|e| e.name().to_string())
                    .unwrap_or_default();
                if name.to_ascii_lowercase().ends_with(".img") {
                    let size = z.by_index(i).map(|e| e.size()).unwrap_or(0);
                    if best.map(|(s, _)| size > s).unwrap_or(true) {
                        best = Some((size, i));
                    }
                }
            }
            let (_, idx) = best
                .ok_or_else(|| format!("{} : aucun fichier .img dans l'archive", path.display()))?;
            let mut entry = z.by_index(idx).map_err(|e| e.to_string())?;
            let mut out = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut out)
                .map_err(|e| format!("{} : {e}", path.display()))?;
            Ok(out)
        }
        _ => Err(format!(
            "{} : format non pris en charge (attendu : image .img ou archive .zip de release)",
            path.display()
        )),
    }
}

/// Compare l'image au `SHA256SUMS.txt` du même dossier, s'il existe.
fn verify_sums(path: &Path, bytes: &[u8]) -> Option<String> {
    let dir = path.parent()?;
    let sums = fs::read_to_string(dir.join("SHA256SUMS.txt")).ok()?;
    let file_name = path.file_name()?.to_string_lossy().to_string();
    let mut wanted = None;
    let mut img_hash = None;
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = name.trim_start_matches('*');
        if name == file_name {
            wanted = Some(hash.to_ascii_lowercase());
        } else if name.to_ascii_lowercase().ends_with(".img") {
            img_hash = Some(hash.to_ascii_lowercase());
        }
    }
    let wanted = wanted.or(img_hash)?;
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let got = format!("{:x}", hasher.finalize());
    Some(if got == wanted {
        format!("SHA-256 vérifié ({})", &got[..16])
    } else {
        format!("SHA-256 DIFFÉRENT de SHA256SUMS.txt ({})", &got[..16])
    })
}

// ---------------------------------------------------------------------------
// État de la clé et verdict
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct KeyState {
    pub version: Option<String>,
    pub built: Option<String>,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl KeyState {
    pub fn from_files(files: BTreeMap<String, Vec<u8>>) -> KeyState {
        let marker = files
            .get(KEY_MARKER)
            .map(|d| parse_marker(&String::from_utf8_lossy(d)))
            .unwrap_or_default();
        KeyState {
            version: marker.version,
            built: marker.built,
            files,
        }
    }

    /// Une clé est reconnue Legacy2UEFI si elle porte le marqueur et au moins un chargeur.
    pub fn is_legacy2uefi(&self) -> bool {
        self.files.contains_key(KEY_MARKER)
            && (self.files.contains_key("EFI/OC/OpenCore.efi") || self.files.contains_key("boot"))
    }

    pub fn version_or_unknown(&self) -> String {
        self.version.clone().unwrap_or_else(|| "inconnue".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// La clé contient déjà exactement la version et le contenu de l'image.
    UpToDate,
    /// L'image est plus récente : mise à jour recommandée.
    Update,
    /// Même numéro de version, contenu différent (build local, image retouchée).
    SameVersionDifferentContent,
    /// L'image est plus ancienne que la clé : rétrogradation.
    Downgrade,
    /// Version de la clé illisible : contenu comparé, prudence.
    Unknown,
}

impl Verdict {
    pub fn update_available(self) -> bool {
        matches!(
            self,
            Verdict::Update | Verdict::SameVersionDifferentContent | Verdict::Unknown
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::UpToDate => "à jour",
            Verdict::Update => "mise à jour disponible",
            Verdict::SameVersionDifferentContent => "même version, contenu différent",
            Verdict::Downgrade => "image plus ancienne que la clé",
            Verdict::Unknown => "version de la clé illisible",
        }
    }
}

/// Compare l'état de la clé au contenu de l'image (le marqueur, qui contient une date de build,
/// est comparé par sa seule version).
pub fn verdict(key: &KeyState, payload: &Payload) -> Verdict {
    let key_files = comparable(&key.files);
    let payload_files = comparable(&payload.files);
    let same_content = key_files == payload_files;
    match (key.version.as_deref(), payload.version.as_deref()) {
        (Some(k), Some(p)) => match compare_version_str(p, k) {
            std::cmp::Ordering::Greater => Verdict::Update,
            std::cmp::Ordering::Equal => {
                if same_content {
                    Verdict::UpToDate
                } else {
                    Verdict::SameVersionDifferentContent
                }
            }
            std::cmp::Ordering::Less => Verdict::Downgrade,
        },
        _ => Verdict::Unknown,
    }
}

fn comparable(files: &BTreeMap<String, Vec<u8>>) -> BTreeSet<(&str, &[u8])> {
    files
        .iter()
        .filter(|(p, _)| p.as_str() != KEY_MARKER)
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub same: usize,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} ajouté(s), {} modifié(s), {} supprimé(s), {} identique(s)",
            self.added.len(),
            self.changed.len(),
            self.removed.len(),
            self.same
        )
    }
}

/// Différentiel clé → image, hors marqueur (dont la date de build change à chaque build).
pub fn diff_trees(key: &BTreeMap<String, Vec<u8>>, new: &BTreeMap<String, Vec<u8>>) -> Diff {
    let mut d = Diff::default();
    for (path, data) in new {
        if path == KEY_MARKER {
            continue;
        }
        match key.get(path) {
            None => d.added.push(path.clone()),
            Some(old) if old == data => d.same += 1,
            Some(_) => d.changed.push(path.clone()),
        }
    }
    for path in key.keys() {
        if path != KEY_MARKER && !new.contains_key(path) {
            d.removed.push(path.clone());
        }
    }
    d
}

// ---------------------------------------------------------------------------
// Aides disque (image en mémoire)
// ---------------------------------------------------------------------------

/// Lit l'état d'une clé à partir d'une image complète en mémoire (tests, inspection d'image).
#[allow(dead_code)]
pub fn state_from_image_bytes(bytes: &mut [u8]) -> Result<KeyState, String> {
    let (off, end) = partition_from_mbr(&bytes[..512])?;
    if end > bytes.len() as u64 {
        return Err("image tronquée".into());
    }
    let part = &mut bytes[off as usize..end as usize];
    let mut cur = Cursor::new(&mut part[..]);
    let fs = fatfs::FileSystem::new(&mut cur, fatfs::FsOptions::new())
        .map_err(|e| format!("FAT32 illisible : {e}"))?;
    Ok(KeyState::from_files(read_tree(&fs)?))
}

/// Écrit `bytes` au début de `w` par blocs, en signalant l'avancement (0..=100).
pub fn write_image_blocks<W: Write + Seek>(
    w: &mut W,
    bytes: &[u8],
    mut progress: impl FnMut(u32),
) -> Result<(), String> {
    const BLOCK: usize = 1 << 20;
    w.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    let total = bytes.len().max(1);
    let mut done = 0usize;
    for chunk in bytes.chunks(BLOCK) {
        w.write_all(chunk).map_err(|e| format!("écriture : {e}"))?;
        done += chunk.len();
        progress(((done * 100) / total) as u32);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_ordering() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_version_str("0.1.0-alpha.3", "0.1.0-alpha.10"), Less);
        assert_eq!(compare_version_str("0.1.0-alpha.3", "0.1.0"), Less);
        assert_eq!(compare_version_str("0.1.0", "0.1.0-alpha.3"), Greater);
        assert_eq!(compare_version_str("0.2.0", "0.1.9"), Greater);
        assert_eq!(compare_version_str("0.1.0-alpha.2", "0.1.0-alpha.2"), Equal);
        assert_eq!(compare_version_str("v1.2.3", "1.2.3"), Equal);
        assert_eq!(compare_version_str("1.0.0+build7", "1.0.0"), Equal);
        assert_eq!(compare_version_str("1.0.0-alpha", "1.0.0-alpha.1"), Less);
        assert_eq!(compare_version_str("1.0.0-alpha.1", "1.0.0-beta"), Less);
    }

    #[test]
    fn version_rejects_garbage() {
        assert!(Version::parse("").is_none());
        assert!(Version::parse("0.1").is_none());
        assert!(Version::parse("0.1.x").is_none());
        assert!(Version::parse("abc").is_none());
        assert_eq!(
            Version::parse("0.1.0-alpha.3").unwrap().pre,
            vec!["alpha", "3"]
        );
    }

    #[test]
    fn marker_parsing() {
        let m =
            parse_marker("LEGACY2UEFI key\nversion: 0.1.0-alpha.3\nbuilt: 2026-11-20T10:00:00\n");
        assert_eq!(m.version.as_deref(), Some("0.1.0-alpha.3"));
        assert_eq!(m.built.as_deref(), Some("2026-11-20T10:00:00"));
        assert!(parse_marker("").version.is_none());
        assert!(parse_marker("version:\n").version.is_none());
    }

    #[test]
    fn mbr_partition_parse() {
        let mut mbr = [0u8; 512];
        mbr[446] = 0x80;
        mbr[450] = 0x0C;
        mbr[454..458].copy_from_slice(&2048u32.to_le_bytes());
        mbr[458..462].copy_from_slice(&1000u32.to_le_bytes());
        mbr[510] = 0x55;
        mbr[511] = 0xAA;
        let (off, end) = partition_from_mbr(&mbr).unwrap();
        assert_eq!(off, 2048 * 512);
        assert_eq!(end, (2048 + 1000) * 512);

        let mut bad = mbr;
        bad[450] = 0x83;
        assert!(partition_from_mbr(&bad).is_err());
        let mut nosig = mbr;
        nosig[510] = 0;
        assert!(partition_from_mbr(&nosig).is_err());
        let mut empty = mbr;
        empty[458..462].copy_from_slice(&0u32.to_le_bytes());
        assert!(partition_from_mbr(&empty).is_err());
    }

    fn payload_with(version: &str, files: Vec<(&str, &str)>) -> Payload {
        let mut map = BTreeMap::new();
        map.insert(
            KEY_MARKER.to_string(),
            render_marker(version, "2026-11-20T00:00:00").into_bytes(),
        );
        for (p, d) in files {
            map.insert(p.to_string(), d.as_bytes().to_vec());
        }
        Payload {
            path: PathBuf::from("/tmp/i.img"),
            version: Some(version.to_string()),
            built: Some("2026-11-20T00:00:00".into()),
            bytes: Vec::new(),
            files: map,
            integrity: None,
        }
    }

    #[test]
    fn verdict_cases() {
        let key = KeyState::from_files(BTreeMap::from([
            (
                KEY_MARKER.to_string(),
                render_marker("0.1.0-alpha.2", "old").into_bytes(),
            ),
            ("EFI/OC/OpenCore.efi".to_string(), b"v2".to_vec()),
        ]));
        let newer = payload_with("0.1.0-alpha.3", vec![("EFI/OC/OpenCore.efi", "v3")]);
        assert_eq!(verdict(&key, &newer), Verdict::Update);
        assert!(verdict(&key, &newer).update_available());

        let older = payload_with("0.1.0-alpha.1", vec![("EFI/OC/OpenCore.efi", "v1")]);
        assert_eq!(verdict(&key, &older), Verdict::Downgrade);
        assert!(!verdict(&key, &older).update_available());

        let same = payload_with("0.1.0-alpha.2", vec![("EFI/OC/OpenCore.efi", "v2")]);
        assert_eq!(verdict(&key, &same), Verdict::UpToDate);

        let same_version_other = payload_with("0.1.0-alpha.2", vec![("EFI/OC/OpenCore.efi", "vX")]);
        assert_eq!(
            verdict(&key, &same_version_other),
            Verdict::SameVersionDifferentContent
        );
        assert!(verdict(&key, &same_version_other).update_available());

        let unknown = KeyState::from_files(BTreeMap::from([(
            "EFI/OC/OpenCore.efi".to_string(),
            b"x".to_vec(),
        )]));
        assert_eq!(verdict(&unknown, &newer), Verdict::Unknown);
        assert!(!unknown.is_legacy2uefi());
    }

    #[test]
    fn diff_reports_changes() {
        let key = BTreeMap::from([
            (
                KEY_MARKER.to_string(),
                render_marker("0.1.0-alpha.2", "a").into_bytes(),
            ),
            ("boot".to_string(), b"boot-old".to_vec()),
            ("EFI/BOOT/BOOTx64.efi".to_string(), b"same".to_vec()),
            ("EFI/GONE.efi".to_string(), b"gone".to_vec()),
        ]);
        let new = BTreeMap::from([
            (
                KEY_MARKER.to_string(),
                render_marker("0.1.0-alpha.3", "b").into_bytes(),
            ),
            ("boot".to_string(), b"boot-new".to_vec()),
            ("EFI/BOOT/BOOTx64.efi".to_string(), b"same".to_vec()),
            ("EFI/NEW.efi".to_string(), b"new".to_vec()),
        ]);
        let d = diff_trees(&key, &new);
        assert_eq!(d.changed, vec!["boot"]);
        assert_eq!(d.added, vec!["EFI/NEW.efi"]);
        assert_eq!(d.removed, vec!["EFI/GONE.efi"]);
        assert_eq!(d.same, 1);
        assert!(!d.is_empty());
    }

    #[test]
    fn log_files_are_ignored() {
        assert!(is_key_log("L2U-LAUNCHER-LOG.txt"));
        assert!(is_key_log("opencore-2026-11-20-101500.txt"));
        assert!(!is_key_log("LEGACY2UEFI-KEY.txt"));
        assert!(!is_key_log("boot"));
    }

    #[test]
    fn render_marker_roundtrip() {
        let text = render_marker("0.1.0-alpha.4", "2026-11-21T09:00:00");
        let m = parse_marker(&text);
        assert_eq!(m.version.as_deref(), Some("0.1.0-alpha.4"));
        assert_eq!(m.built.as_deref(), Some("2026-11-21T09:00:00"));
    }
}
