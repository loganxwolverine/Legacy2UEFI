// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Tâche privilégiée : `l2u-key-gui --apply <fichier.json>`.
//!
//! L'interface ne touche jamais un disque : elle écrit un fichier de tâche, le lance sous `pkexec`,
//! et suit son avancement dans un journal (une ligne `PROGRESS`/`DONE` par étape). Le processus
//! privilégié ne fait que ce que la tâche décrit, sur la cible indiquée — et revalide lui-même que
//! la cible est amovible (aucune confiance dans les arguments).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::{self, Payload, Verdict};
use crate::key;

/// Code stable d'un verdict, utilisé dans le journal de tâche (lu par l'interface).
pub fn verdict_code(v: Verdict) -> &'static str {
    match v {
        Verdict::UpToDate => "uptodate",
        Verdict::Update => "update",
        Verdict::SameVersionDifferentContent => "different",
        Verdict::Downgrade => "downgrade",
        Verdict::Unknown => "unknown",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    /// `create` | `update` | `inspect`.
    pub op: String,
    pub device: String,
    pub payload: String,
    /// Autorise une rétrogradation (image plus ancienne) ou une réécriture à l'identique.
    #[serde(default)]
    pub force: bool,
    /// Fichier de journal d'exécution (lu par l'interface).
    pub log: String,
}

pub struct Journal {
    file: File,
}

impl Journal {
    pub fn create(path: &str) -> Result<Journal, String> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|e| format!("{path} : {e}"))?;
        Ok(Journal { file })
    }

    pub fn line(&mut self, text: &str) {
        let _ = writeln!(self.file, "{text}");
        let _ = self.file.flush();
    }

    pub fn progress(&mut self, pct: u32, msg: &str) {
        self.line(&format!("PROGRESS {} {}", pct.min(100), msg));
    }

    pub fn done(&mut self, result: Result<String, String>) -> i32 {
        match result {
            Ok(msg) => {
                self.line(&format!("DONE ok {msg}"));
                0
            }
            Err(e) => {
                self.line(&format!("DONE err {e}"));
                1
            }
        }
    }
}

pub fn run(job_path: &str) -> i32 {
    let text = match std::fs::read_to_string(job_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("tâche illisible ({job_path}) : {e}");
            return 2;
        }
    };
    let job: Job = match serde_json::from_str(&text) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("tâche invalide : {e}");
            return 2;
        }
    };
    let mut log = match Journal::create(&job.log) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    log.line(&format!("JOB {} {} {}", job.op, job.device, job.payload));
    let res = execute(&job, &mut log);
    log.done(res)
}

fn execute(job: &Job, log: &mut Journal) -> Result<String, String> {
    // Revalidation de la cible : un fichier ordinaire (test, image) ou un disque amovible identifié.
    let dev = key::check_target(&job.device)?;
    if let Some(d) = &dev {
        log.line(&format!("TARGET {} {}", d.name, d.label()));
    } else {
        log.line("TARGET fichier (mode test)");
    }

    let payload = Payload::load(Path::new(&job.payload))?;
    log.line(&format!(
        "PAYLOAD {} version {}",
        payload.path.display(),
        payload.version_or_unknown()
    ));
    if let Some(i) = &payload.integrity {
        log.line(&format!("INTEGRITY {i}"));
        if i.contains("DIFFÉRENT") {
            return Err("l'image ne correspond pas à SHA256SUMS.txt — écriture refusée".into());
        }
    }

    match job.op.as_str() {
        "inspect" => {
            let mut f = key::open_rw(&job.device)?;
            let state = key::read_key_state(&mut f)?;
            let v = engine::verdict(&state, &payload);
            let d = engine::diff_trees(&state.files, &payload.files);
            log.line(&format!("KEYVERSION {}", state.version_or_unknown()));
            log.line(&format!(
                "KEYBUILD {}",
                state.built.clone().unwrap_or_else(|| "inconnu".into())
            ));
            log.line(&format!("KEYFILES {}", state.files.len()));
            log.line(&format!(
                "IDENTITY {}",
                if state.is_legacy2uefi() {
                    "legacy2uefi"
                } else {
                    "unknown"
                }
            ));
            log.line(&format!("VERDICT {}", verdict_code(v)));
            log.line(&format!("DIFF {}", d.summary()));
            for p in &d.changed {
                log.line(&format!("CHANGED {p}"));
            }
            for p in &d.added {
                log.line(&format!("ADDED {p}"));
            }
            for p in &d.removed {
                log.line(&format!("REMOVED {p}"));
            }
            Ok(format!(
                "clé : {} ({})",
                state.version_or_unknown(),
                v.label()
            ))
        }
        "create" => {
            if let Some(d) = &dev {
                key::unmount_all(d, &mut |p, m| log.progress(p, m))?;
            }
            let mut f = key::open_rw(&job.device)?;
            key::write_image(&mut f, &payload, &mut |p, m| log.progress(p, m))?;
            let checked = key::verify_image(&mut f, &payload, &mut |p, m| log.progress(p, m))?;
            log.progress(100, "terminé");
            Ok(format!(
                "clé créée : version {}, {checked} fichier(s) vérifiés",
                payload.version_or_unknown()
            ))
        }
        "update" => {
            if let Some(d) = &dev {
                key::unmount_all(d, &mut |p, m| log.progress(p, m))?;
            }
            let mut f = key::open_rw(&job.device)?;
            let out = key::update_key(&mut f, &payload, job.force, &mut |p, m| log.progress(p, m))?;
            Ok(format!(
                "clé mise à jour : {} → {}, {} fichier(s) vérifiés ({})",
                out.key_before,
                out.key_after,
                out.verified,
                out.diff.summary()
            ))
        }
        other => Err(format!("opération inconnue : {other}")),
    }
}

/// Version publiée la plus récente du dépôt (GitHub Releases), pour proposer une image à jour.
pub fn latest_release() -> Result<(String, Option<String>), String> {
    const API: &str = "https://api.github.com/repos/loganxwolverine/Legacy2UEFI/releases/latest";
    let resp = ureq::get(API)
        .set("User-Agent", "l2u-key-gui")
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "aucune release publiée pour l'instant".to_string(),
            ureq::Error::Status(code, _) => format!("GitHub a répondu {code}"),
            other => format!("réseau : {other}"),
        })?;
    let body = resp
        .into_string()
        .map_err(|e| format!("lecture de la réponse : {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('v').to_string())
        .ok_or("réponse sans tag_name")?;
    let asset = json.get("assets").and_then(|a| a.as_array()).and_then(|a| {
        a.iter()
            .filter_map(|x| x.get("browser_download_url").and_then(|v| v.as_str()))
            .find(|u| u.ends_with(".img") || u.ends_with(".zip"))
            .map(|s| s.to_string())
    });
    Ok((tag, asset))
}

/// Télécharge une image de release dans le dossier indiqué et retourne son chemin.
pub fn download(url: &str, dest_dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let name = url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .ok_or("URL sans nom de fichier")?;
    let dest = dest_dir.join(name);
    let resp = ureq::get(url)
        .set("User-Agent", "l2u-key-gui")
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| format!("téléchargement : {e}"))?;
    let mut reader = resp.into_reader();
    let mut file = File::create(&dest).map_err(|e| format!("{} : {e}", dest.display()))?;
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("{} : {e}", dest.display()))?;
    file.flush().map_err(|e| e.to_string())?;
    // Vérification de taille : une réponse d'erreur HTML ne doit pas passer pour une image.
    let mut head = [0u8; 512];
    let mut f = File::open(&dest).map_err(|e| e.to_string())?;
    let n = f.read(&mut head).map_err(|e| e.to_string())?;
    if n < 512 {
        return Err("fichier téléchargé trop court".into());
    }
    Ok(dest.to_string_lossy().to_string())
}

/// Répertoire de cache des images téléchargées.
pub fn cache_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                .join(".cache")
        });
    base.join("l2u-key-gui")
}
