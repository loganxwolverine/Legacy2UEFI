// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! l2u-key-gui — crée et met à jour la clé USB Legacy2UEFI (BIOS Legacy → UEFI logiciel).
//!
//! Interface Qt6 par défaut ; les modes texte servent aux tests, aux scripts et au diagnostic :
//!
//! ```text
//! l2u-key-gui                       interface graphique
//! l2u-key-gui --list-devices        périphériques de bloc et verdict de sûreté
//! l2u-key-gui --inspect <cible>     état d'une clé (version, contenu)
//! l2u-key-gui --create <image> <cible> [--force]
//! l2u-key-gui --update <image> <cible> [--force]
//! l2u-key-gui --check-updates       dernière release publiée du dépôt
//! l2u-key-gui --apply <tâche.json>  exécution privilégiée d'une tâche (interne)
//! ```
//!
//! Les modes `--create`/`--update` doivent tourner en root (directement ou via `pkexec`) ; la cible
//! est revalidée dans tous les cas : fichier ordinaire, ou disque amovible/USB hors système.

mod device;
mod engine;
mod gui;
mod job;
mod key;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
l2u-key-gui — clé USB Legacy2UEFI

  l2u-key-gui                                  interface graphique
  l2u-key-gui --list-devices                   périphériques et verdict de sûreté
  l2u-key-gui --inspect <cible>                version et contenu d'une clé
  l2u-key-gui --create <image> <cible>         écrit l'image de release sur la cible
  l2u-key-gui --update <image> <cible>         met à jour la clé en place (sans repartitionner)
  l2u-key-gui --check-updates                  dernière release publiée du dépôt
  l2u-key-gui --apply <tâche.json>             exécution privilégiée (interne)

  <image> : legacy2uefi-<version>.img ou .zip · <cible> : /dev/sdX ou fichier image
  --force, -f : autorise une rétrogradation ou une réécriture à l'identique
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("--version" | "-V") => {
            println!("l2u-key-gui {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("--apply") => match args.get(1) {
            Some(jobfile) => code(job::run(jobfile)),
            None => usage_error("--apply attend le chemin d'un fichier de tâche"),
        },
        Some("--list-devices") => {
            let devs = device::list_devices();
            if devs.is_empty() {
                println!("aucun périphérique de bloc");
            }
            for d in devs {
                let verdict = match device::check_writable(&d) {
                    Ok(()) => "utilisable".to_string(),
                    Err(e) => format!("refusé — {e}"),
                };
                println!("{}\n    {}\n    {}", d.name, d.detail(), verdict);
            }
            ExitCode::SUCCESS
        }
        Some("--inspect") => match args.get(1) {
            Some(target) => inspect(target),
            None => usage_error("--inspect attend une cible"),
        },
        Some("--create") => create_or_update(&args, true),
        Some("--update") => create_or_update(&args, false),
        Some("--check-updates") => check_updates(),
        Some(other) if other.starts_with('-') => usage_error(&format!("option inconnue : {other}")),
        _ => code(gui::run()),
    }
}

fn code(c: i32) -> ExitCode {
    ExitCode::from(c.clamp(0, 255) as u8)
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("l2u-key-gui : {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn inspect(target: &str) -> ExitCode {
    let dev = match key::check_target(target) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    // Sans privilège : lire la partition si le bureau l'a montée.
    if let Some(d) = &dev {
        if let Some((state, mount)) = key::state_from_mounts(d) {
            println!("cible     : {} ({mount})", d.path);
            println!("version   : {}", state.version_or_unknown());
            println!(
                "build     : {}",
                state.built.as_deref().unwrap_or("inconnu")
            );
            println!(
                "identité  : {}",
                if state.is_legacy2uefi() {
                    "Legacy2UEFI"
                } else {
                    "inconnue"
                }
            );
            println!("fichiers  : {}", state.files.len());
            return ExitCode::SUCCESS;
        }
    }
    let mut f = match key::open_rw(target) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    match key::read_key_state(&mut f) {
        Ok(state) => {
            println!("cible     : {target}");
            println!("version   : {}", state.version_or_unknown());
            println!(
                "build     : {}",
                state.built.as_deref().unwrap_or("inconnu")
            );
            println!(
                "identité  : {}",
                if state.is_legacy2uefi() {
                    "Legacy2UEFI"
                } else {
                    "inconnue"
                }
            );
            println!("fichiers  : {}", state.files.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{target} : {e}");
            ExitCode::from(1)
        }
    }
}

fn create_or_update(args: &[String], create: bool) -> ExitCode {
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let (Some(payload), Some(target)) = (positional.first(), positional.get(1)) else {
        return usage_error("attendu : <image> <cible>");
    };
    // Une cible fichier ne demande aucun privilège ; un disque, si.
    let target_is_file = Path::new(target.as_str()).is_file();
    if !target_is_file && !is_root() {
        eprintln!(
            "l2u-key-gui : cette opération écrit sur un disque et doit tourner en root.\n\
             Utilisez l'interface graphique (elle demande l'autorisation via polkit) ou :\n  \
             sudo l2u-key-gui --{} {payload} {target}",
            if create { "create" } else { "update" }
        );
        return ExitCode::from(3);
    }
    let dev = match key::check_target(target) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let payload = match engine::Payload::load(Path::new(payload.as_str())) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    println!(
        "image  : {} — version {}",
        payload.path.display(),
        payload.version_or_unknown()
    );
    if let Some(i) = &payload.integrity {
        println!("intégrité : {i}");
        if i.contains("DIFFÉRENT") {
            eprintln!("l'image ne correspond pas à SHA256SUMS.txt — abandon");
            return ExitCode::from(1);
        }
    }
    let mut progress = |pct: u32, msg: &str| {
        if !msg.is_empty() {
            println!("{pct:3}% {msg}");
        } else if pct % 10 == 0 {
            println!("{pct:3}% …");
        }
    };
    if let Some(d) = &dev {
        match key::unmount_all(d, &mut progress) {
            Ok(v) if !v.is_empty() => println!("démonté : {}", v.join(", ")),
            Ok(_) => {}
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(1);
            }
        }
    }
    let mut file = match key::open_rw(target) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let result = if create {
        key::write_image(&mut file, &payload, &mut progress)
            .and_then(|()| key::verify_image(&mut file, &payload, &mut progress).map(|_| ()))
            .map(|()| format!("clé créée — version {}", payload.version_or_unknown()))
    } else {
        key::update_key(&mut file, &payload, force, &mut progress).map(|o| {
            format!(
                "clé mise à jour : {} → {} ({} fichier(s) vérifiés, {})",
                o.key_before,
                o.key_after,
                o.verified,
                o.diff.summary()
            )
        })
    };
    match result {
        Ok(msg) => {
            println!("OK — {msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ÉCHEC — {e}");
            ExitCode::from(1)
        }
    }
}

fn check_updates() -> ExitCode {
    match job::latest_release() {
        Ok((tag, asset)) => {
            println!("dernière release : {tag}");
            match asset {
                Some(url) => println!("image : {url}"),
                None => println!("aucune image .img/.zip attachée à cette release"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(1)
        }
    }
}

pub fn is_root() -> bool {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}
