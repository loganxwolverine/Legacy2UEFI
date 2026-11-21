// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Interface graphique Qt6 (qtrs) — un binaire, aucune donnée privilégiée manipulée ici.
//!
//! L'interface ne touche jamais un disque : les opérations d'écriture sont des tâches lancées via
//! `pkexec` (`--apply`), suivies dans un fichier journal relu par la pompe Qt.
//!
//! Règles qtrs respectées : tous les layouts sont conservés jusqu'à la fin de `exec()`, aucun
//! `RefCell` n'est emprunté pendant un dialogue modal, et les widgets mutés sont pilotés par leur
//! pointeur C++ (jamais par un wrapper Rust qui serait détruit avant son parent).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::mpsc;

use qtrs::prelude::*;
use qtrs::{Application, FileDialog, MainWindow, MessageBox, Timer};

use crate::device::{self, Device};
use crate::engine::{self, KeyState, Payload, Verdict};
use crate::job::{self, Job};
use crate::key;

/// `QMessageBox::StandardButton` : Oui = 0x4000, Annuler = 0x00400000 (Ok vaut 0x400).
const BTN_YES: i32 = 0x0000_4000;
const BTN_CANCEL: i32 = 0x0040_0000;
const ICON_QUESTION: i32 = 4;
const ICON_WARNING: i32 = 2;

const APP_TITLE: &str = "Legacy2UEFI — clé USB";
const IMAGE_FILTER: &str = "Images Legacy2UEFI (*.img *.zip);;Tous les fichiers (*)";

// ---------------------------------------------------------------------------
// Handles de widgets (pointeurs C++, valides tant que la fenêtre vit)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct W {
    dev_combo: *mut qtrs::ffi::QComboBox,
    detail: *mut qtrs::ffi::QLabel,
    img_edit: *mut qtrs::ffi::QLineEdit,
    img_info: *mut qtrs::ffi::QLabel,
    key_info: *mut qtrs::ffi::QLabel,
    verdict: *mut qtrs::ffi::QLabel,
    status: *mut qtrs::ffi::QLabel,
    bar: *mut qtrs::ffi::QProgressBar,
    log: *mut qtrs::ffi::QPlainTextEdit,
    create_btn: *mut qtrs::ffi::QPushButton,
    update_btn: *mut qtrs::ffi::QPushButton,
    repo_btn: *mut qtrs::ffi::QPushButton,
    inspect_btn: *mut qtrs::ffi::QPushButton,
    confirm: *mut qtrs::ffi::QCheckBox,
    central: *mut qtrs::ffi::QWidget,
}

/// Résultat d'une analyse privilégiée de la clé (quand elle n'est pas montée).
#[derive(Clone, Debug, Default)]
struct Analysis {
    version: String,
    verdict: Option<Verdict>,
    diff: String,
}

#[derive(Default)]
struct State {
    w: RefCell<W>,
    devices: RefCell<Vec<Device>>,
    payload: RefCell<Option<Payload>>,
    key_state: RefCell<Option<KeyState>>,
    mounted: RefCell<Option<String>>,
    analysis: RefCell<Option<Analysis>>,
    busy: Cell<bool>,
    repo_busy: Cell<bool>,
    child: RefCell<Option<Child>>,
    log_file: RefCell<Option<PathBuf>>,
    log_pos: Cell<u64>,
    job_dir: RefCell<Option<PathBuf>>,
    done: RefCell<Option<Result<String, String>>>,
    /// Dernier libellé sélectionné (détection du changement par la pompe).
    sel: RefCell<String>,
    /// Miroir Rust du statut et du journal (Qt n'expose pas de getter) : sert à l'auto-test.
    last_status: RefCell<String>,
    last_log: RefCell<Vec<String>>,
}

impl State {
    fn w(&self) -> W {
        *self.w.borrow()
    }
    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        let w = self.w();
        unsafe {
            qtrs::ffi::QWidget_setEnabled(w.create_btn as *mut qtrs::ffi::QWidget, !busy);
            qtrs::ffi::QWidget_setEnabled(w.inspect_btn as *mut qtrs::ffi::QWidget, !busy);
            qtrs::ffi::QWidget_setEnabled(w.repo_btn as *mut qtrs::ffi::QWidget, !busy);
            if busy {
                qtrs::ffi::QWidget_setEnabled(w.update_btn as *mut qtrs::ffi::QWidget, false);
            }
        }
        if !busy {
            self.refresh_buttons();
        }
    }

    fn refresh_buttons(&self) {
        let w = self.w();
        let dev = selected_device(self).is_some();
        let payload = self.payload.borrow().is_some();
        let confirmed = unsafe { qtrs::ffi::QCheckBox_isChecked(w.confirm) };
        let busy = self.busy.get();
        let updatable = self
            .verdict()
            .map(|v| v.update_available())
            .unwrap_or(false)
            || self.verdict() == Some(Verdict::Downgrade);
        unsafe {
            qtrs::ffi::QWidget_setEnabled(
                w.create_btn as *mut qtrs::ffi::QWidget,
                !busy && dev && payload && confirmed,
            );
            qtrs::ffi::QWidget_setEnabled(
                w.update_btn as *mut qtrs::ffi::QWidget,
                !busy && dev && payload && updatable,
            );
        }
    }

    fn verdict(&self) -> Option<Verdict> {
        if let (Some(state), Some(payload)) = (
            self.key_state.borrow().as_ref(),
            self.payload.borrow().as_ref(),
        ) {
            return Some(engine::verdict(state, payload));
        }
        self.analysis.borrow().as_ref().and_then(|a| a.verdict)
    }
}

enum Msg {
    Repo(Result<(String, Option<String>), String>),
    Downloaded(Result<(String, String), String>),
}

// ---------------------------------------------------------------------------
// Aides widgets
// ---------------------------------------------------------------------------

fn set_label(ptr: *mut qtrs::ffi::QLabel, text: &str) {
    if ptr.is_null() {
        return;
    }
    cxx::let_cxx_string!(c = text);
    unsafe { qtrs::ffi::QLabel_setText(ptr, &c) };
}

fn read_edit(ptr: *mut qtrs::ffi::QLineEdit) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { qtrs::ffi::QLineEdit_text(ptr) }
}

fn set_edit(ptr: *mut qtrs::ffi::QLineEdit, text: &str) {
    if ptr.is_null() {
        return;
    }
    cxx::let_cxx_string!(c = text);
    unsafe { qtrs::ffi::QLineEdit_setText(ptr, &c) };
}

fn log_line(s: &State, text: &str) {
    let w = s.w();
    s.last_log.borrow_mut().push(text.to_string());
    if w.log.is_null() {
        return;
    }
    cxx::let_cxx_string!(c = text);
    unsafe { qtrs::ffi::QPlainTextEdit_appendPlainText(w.log, &c) };
}

fn set_status(s: &State, text: &str) {
    *s.last_status.borrow_mut() = text.to_string();
    set_label(s.w().status, text);
}

fn set_progress(w: &W, pct: i32) {
    if w.bar.is_null() {
        return;
    }
    unsafe { qtrs::ffi::QProgressBar_setValue(w.bar, pct.clamp(0, 100)) };
}

fn set_progress_indeterminate(w: &W, on: bool) {
    if w.bar.is_null() {
        return;
    }
    unsafe {
        if on {
            qtrs::ffi::QProgressBar_setRange(w.bar, 0, 0)
        } else {
            qtrs::ffi::QProgressBar_setRange(w.bar, 0, 100)
        }
    }
}

fn set_fixed_height(ptr: *mut qtrs::ffi::QWidget, h: i32) {
    if !ptr.is_null() {
        unsafe { qtrs::ffi::QWidget_setFixedHeight(ptr, h) };
    }
}

fn confirm(title: &str, text: &str, icon: i32) -> bool {
    // Dérogation de test : `L2U_KEY_GUI_YES=1` répond « oui » aux confirmations. Sans elle,
    // une exécution sans utilisateur (CI, Xvfb) resterait bloquée sur la boîte modale.
    if std::env::var("L2U_KEY_GUI_YES").as_deref() == Ok("1") {
        eprintln!("CONFIRM {title} : {text}");
        return true;
    }
    // Sans parent : un parent forcerait à garder un emprunt du RefCell pendant la boucle modale.
    let mb = MessageBox::new()
        .icon(icon)
        .text(text)
        .window_title(title)
        .standard_buttons(BTN_YES | BTN_CANCEL)
        .default_button(BTN_YES)
        .build();
    mb.exec() == BTN_YES
}

fn info(title: &str, text: &str) {
    MessageBox::new()
        .icon(ICON_QUESTION)
        .text(text)
        .window_title(title)
        .standard_buttons(BTN_YES)
        .default_button(BTN_YES)
        .build()
        .exec();
}

fn selected_device(s: &State) -> Option<Device> {
    let w = s.w();
    let text = unsafe { qtrs::ffi::QComboBox_currentText(w.dev_combo) };
    s.devices
        .borrow()
        .iter()
        .find(|d| d.label() == text)
        .cloned()
}

// ---------------------------------------------------------------------------
// Rafraîchissements
// ---------------------------------------------------------------------------

fn refresh_devices(s: &State) {
    let w = s.w();
    let keep = selected_device(s).map(|d| d.name);
    let devs = device::list_devices();
    unsafe {
        qtrs::ffi::QComboBox_clear(w.dev_combo);
        let mut keep_index = -1;
        for (i, d) in devs.iter().enumerate() {
            cxx::let_cxx_string!(c = d.label());
            qtrs::ffi::QComboBox_addItem(w.dev_combo, &c);
            if Some(&d.name) == keep.as_ref() {
                keep_index = i as i32;
            }
        }
        if keep_index >= 0 {
            qtrs::ffi::QComboBox_setCurrentIndex(w.dev_combo, keep_index);
        }
    }
    *s.devices.borrow_mut() = devs;
    let cur = selected_device(s);
    *s.sel.borrow_mut() = cur.as_ref().map(|d| d.label()).unwrap_or_default();
    refresh_key(s);
}

fn refresh_key(s: &State) {
    let w = s.w();
    *s.key_state.borrow_mut() = None;
    *s.mounted.borrow_mut() = None;
    let Some(dev) = selected_device(s) else {
        set_label(w.detail, "Aucune clé détectée.");
        set_label(w.key_info, "Clé : —");
        refresh_payload_info(s);
        return;
    };
    let mut detail = dev.detail();
    if let Err(e) = device::check_writable(&dev) {
        detail.push_str(&format!("\n⚠ {e}"));
    } else if dev.mounts.is_empty() {
        detail.push_str("\nNon montée : ⚠ le contenu sera relu par l'analyse privilégiée.");
    }
    set_label(w.detail, &detail);

    if let Some((state, mount)) = key::state_from_mounts(&dev) {
        let v = state.version_or_unknown();
        let ident = if state.is_legacy2uefi() {
            "Legacy2UEFI"
        } else {
            "identité inconnue"
        };
        set_label(
            w.key_info,
            &format!(
                "Clé : version {v} · {ident} · {} fichier(s) · montée sur {mount}",
                state.files.len()
            ),
        );
        *s.key_state.borrow_mut() = Some(state);
    } else {
        let a = s.analysis.borrow().clone();
        match a {
            Some(a) if !a.version.is_empty() => set_label(
                w.key_info,
                &format!("Clé : version {} · {} · (analyse privilégiée)", a.version, a.diff),
            ),
            _ => set_label(
                w.key_info,
                "Clé : non montée — « Analyser la clé » vérifie son contenu (mot de passe demandé).",
            ),
        }
    }
    refresh_payload_info(s);
}

fn refresh_payload_info(s: &State) {
    let w = s.w();
    let path = read_edit(w.img_edit);
    if path.trim().is_empty() {
        *s.payload.borrow_mut() = None;
        set_label(w.img_info, "Aucune image choisie.");
        refresh_verdict_label(s);
        return;
    }
    match Payload::load(std::path::Path::new(path.trim())) {
        Ok(p) => {
            let integrity = p
                .integrity
                .clone()
                .unwrap_or_else(|| "aucun SHA256SUMS.txt à côté de l'image".into());
            let info = format!(
                "Image : version {} · build {} · {} fichier(s) · {}",
                p.version_or_unknown(),
                p.built.clone().unwrap_or_else(|| "inconnu".into()),
                p.files.len(),
                integrity
            );
            if integrity.contains("DIFFÉRENT") {
                set_label(w.img_info, &info);
                *s.payload.borrow_mut() = None;
                set_label(
                    w.verdict,
                    "⚠ Image non conforme à SHA256SUMS.txt : écriture refusée.",
                );
                s.refresh_buttons();
                return;
            }
            set_label(w.img_info, &info);
            *s.payload.borrow_mut() = Some(p);
        }
        Err(e) => {
            *s.payload.borrow_mut() = None;
            set_label(w.img_info, &format!("⚠ {e}"));
        }
    }
    refresh_verdict_label(s);
}

fn refresh_verdict_label(s: &State) {
    let w = s.w();
    match s.verdict() {
        Some(Verdict::UpToDate) => set_label(w.verdict, "✔ La clé est à jour : rien à faire."),
        Some(Verdict::Update) => set_label(
            w.verdict,
            "▲ Mise à jour disponible : l'image est plus récente que la clé.",
        ),
        Some(Verdict::SameVersionDifferentContent) => set_label(
            w.verdict,
            "▲ Même version mais contenu différent : une mise à jour est proposée.",
        ),
        Some(Verdict::Downgrade) => set_label(
            w.verdict,
            "⚠ L'image est plus ancienne que la clé (rétrogradation) : confirmation supplémentaire.",
        ),
        Some(Verdict::Unknown) => {
            let detail = s.analysis.borrow().as_ref().map(|a| a.diff.clone()).unwrap_or_default();
            set_label(
                w.verdict,
                &format!("Version de la clé inconnue — comparaison du contenu : {detail}"),
            );
        }
        None => set_label(w.verdict, "Choisissez une image de release pour comparer."),
    }
    s.refresh_buttons();
}

// ---------------------------------------------------------------------------
// Tâche privilégiée
// ---------------------------------------------------------------------------

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn start_job(s: &State, op: &str) {
    if s.busy.get() {
        return;
    }
    let w = s.w();
    let Some(dev) = selected_device(s) else {
        set_status(s, "Aucune clé sélectionnée.");
        return;
    };
    let img = read_edit(w.img_edit).trim().to_string();
    if img.is_empty() || s.payload.borrow().is_none() {
        set_status(s, "Choisissez d'abord une image de release valide.");
        return;
    }
    if let Err(e) = device::check_writable(&dev) {
        set_status(s, &format!("Écriture refusée : {e}"));
        return;
    }
    let payload_version = s
        .payload
        .borrow()
        .as_ref()
        .map(|p| p.version_or_unknown())
        .unwrap_or_default();
    let (title, text) = if op == "inspect" {
        (
            "Analyser la clé",
            format!(
                "Analyser {} et la comparer à Legacy2UEFI {payload_version} ?\n\n\
                 La clé est lue seulement : aucun contenu n'est modifié.",
                dev.path
            ),
        )
    } else if op == "create" {
        (
            "Créer la clé",
            format!(
                "Écrire Legacy2UEFI {payload_version} sur {} ?\n\n\
                 Tout le contenu actuel de ce disque sera effacé.\n\
                 Le disque interne n'est jamais touché.",
                dev.path
            ),
        )
    } else {
        (
            "Mettre à jour la clé",
            format!(
                "Mettre à jour {} vers Legacy2UEFI {payload_version} ?\n\n\
                 Le dossier EFI, /boot, les licences et le mode d'emploi sont remplacés.\n\
                 Les journaux de la launcher sont conservés.\n\
                 Le MBR et le secteur de démarrage ne sont pas modifiés.",
                dev.path
            ),
        )
    };
    let downgrade = s.verdict() == Some(Verdict::Downgrade);
    if !confirm(title, &text, ICON_QUESTION) {
        set_status(s, "Opération annulée.");
        return;
    }
    if downgrade
        && !confirm(
            "Rétrogradation",
            &format!(
                "L'image ({payload_version}) est plus ancienne que la clé.\n\
                 Continuer quand même (rétrogradation) ?"
            ),
            ICON_WARNING,
        )
    {
        set_status(s, "Opération annulée.");
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "l2u-key-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        set_status(s, &format!("Dossier temporaire : {e}"));
        return;
    }
    let log = dir.join("job.log");
    let jobfile = dir.join("job.json");
    let task = Job {
        op: op.to_string(),
        device: dev.path.clone(),
        payload: img.clone(),
        force: downgrade,
        log: log.display().to_string(),
    };
    let json = match serde_json::to_string_pretty(&task) {
        Ok(j) => j,
        Err(e) => {
            set_status(s, &format!("Tâche : {e}"));
            return;
        }
    };
    if let Err(e) = std::fs::write(&jobfile, json) {
        set_status(s, &format!("Tâche : {e}"));
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            set_status(s, &format!("Chemin du binaire : {e}"));
            return;
        }
    };
    let mut cmd = if crate::is_root() {
        Command::new(&exe)
    } else {
        match find_in_path("pkexec") {
            Some(pk) => {
                let mut c = Command::new(pk);
                c.arg(&exe);
                c
            }
            None => {
                set_status(
                    s,
                    "pkexec (polkit) est introuvable : lancez l'application en root ou installez polkit.",
                );
                return;
            }
        }
    };
    cmd.arg("--apply").arg(&jobfile);
    match cmd.spawn() {
        Ok(child) => {
            *s.child.borrow_mut() = Some(child);
            *s.log_file.borrow_mut() = Some(log);
            *s.job_dir.borrow_mut() = Some(dir);
            s.log_pos.set(0);
            *s.done.borrow_mut() = None;
            if !w.log.is_null() {
                unsafe { qtrs::ffi::QPlainTextEdit_clear(w.log) };
            }
            set_progress_indeterminate(&w, true);
            set_status(
                s,
                if op == "create" {
                    "Création de la clé en cours — autorisation système demandée…"
                } else {
                    "Mise à jour de la clé en cours — autorisation système demandée…"
                },
            );
            log_line(s, &format!("── {} {} depuis {}", op, dev.path, img));
            s.set_busy(true);
        }
        Err(e) => set_status(s, &format!("Lancement de la tâche impossible : {e}")),
    }
}

fn handle_job_line(s: &State, line: &str) {
    let w = s.w();
    if let Some(rest) = line.strip_prefix("PROGRESS ") {
        let (pct, msg) = rest.split_once(' ').unwrap_or((rest, ""));
        if let Ok(p) = pct.parse::<i32>() {
            set_progress_indeterminate(&w, false);
            set_progress(&w, p);
            if !msg.is_empty() {
                set_status(s, msg);
                log_line(s, &format!("{p:3} % {msg}"));
            }
        }
        return;
    }
    if let Some(rest) = line.strip_prefix("DONE ") {
        let (status, msg) = rest.split_once(' ').unwrap_or((rest, ""));
        *s.done.borrow_mut() = Some(if status == "ok" {
            Ok(msg.to_string())
        } else {
            Err(msg.to_string())
        });
        return;
    }
    if let Some(rest) = line.strip_prefix("VERDICT ") {
        let v = match rest.trim() {
            "uptodate" => Verdict::UpToDate,
            "update" => Verdict::Update,
            "different" => Verdict::SameVersionDifferentContent,
            "downgrade" => Verdict::Downgrade,
            _ => Verdict::Unknown,
        };
        let mut a = s.analysis.borrow_mut();
        let entry = a.get_or_insert_with(Analysis::default);
        entry.verdict = Some(v);
        return;
    }
    if let Some(rest) = line.strip_prefix("KEYVERSION ") {
        let mut a = s.analysis.borrow_mut();
        let entry = a.get_or_insert_with(Analysis::default);
        entry.version = rest.trim().to_string();
        return;
    }
    if let Some(rest) = line.strip_prefix("DIFF ") {
        let mut a = s.analysis.borrow_mut();
        let entry = a.get_or_insert_with(Analysis::default);
        entry.diff = rest.trim().to_string();
        return;
    }
    if let Some(rest) = line.strip_prefix("KEYBUILD ") {
        log_line(s, &format!("build de la clé : {}", rest.trim()));
        return;
    }
    if line.starts_with("CHANGED ") || line.starts_with("ADDED ") || line.starts_with("REMOVED ") {
        log_line(s, &format!("  {line}"));
        return;
    }
    if let Some(rest) = line.strip_prefix("JOB ") {
        log_line(s, &format!("tâche : {rest}"));
        return;
    }
    if let Some(rest) = line.strip_prefix("TARGET ") {
        log_line(s, &format!("cible : {rest}"));
        return;
    }
    if let Some(rest) = line.strip_prefix("PAYLOAD ") {
        log_line(s, &format!("image : {rest}"));
        return;
    }
    if let Some(rest) = line.strip_prefix("INTEGRITY ") {
        log_line(s, &format!("intégrité : {rest}"));
        return;
    }
    if !line.is_empty() {
        log_line(s, line);
    }
}

/// Lit les lignes de journal nouvellement écrites par la tâche.
fn drain_log(s: &State) {
    if let Some(path) = s.log_file.borrow().as_ref() {
        if let Ok(mut f) = std::fs::File::open(path) {
            use std::io::{Read, Seek, SeekFrom};
            let pos = s.log_pos.get();
            if let Ok(meta) = f.metadata() {
                if meta.len() > pos && f.seek(SeekFrom::Start(pos)).is_ok() {
                    let mut buf = Vec::new();
                    if f.read_to_end(&mut buf).is_ok() {
                        let text = String::from_utf8_lossy(&buf).to_string();
                        let mut consumed = 0usize;
                        for line in text.split_inclusive('\n') {
                            if !line.ends_with('\n') {
                                break;
                            }
                            consumed += line.len();
                            handle_job_line(s, line.trim_end_matches(['\r', '\n']));
                        }
                        s.log_pos.set(pos + consumed as u64);
                    }
                }
            }
        }
    }
}

fn pump_job(s: &State) {
    drain_log(s);
    // Fin du processus ?
    let exited = {
        let mut slot = s.child.borrow_mut();
        match slot.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Some(status.code()),
                Ok(None) => None,
                Err(_) => None,
            },
            None => None,
        }
    };
    let Some(code) = exited else { return };
    if !s.busy.get() {
        return;
    }
    // Dernière lecture : la tâche peut avoir écrit son DONE juste avant de sortir.
    drain_log(s);
    *s.child.borrow_mut() = None;
    *s.log_file.borrow_mut() = None;
    let w = s.w();
    set_progress_indeterminate(&w, false);
    let done = s.done.borrow_mut().take();
    let result = match done {
        Some(r) => r,
        None if code == Some(126) || code == Some(127) => {
            Err("autorisation refusée (polkit) : rien n'a été écrit".to_string())
        }
        None => Err(format!("tâche interrompue (code de sortie {code:?})")),
    };
    match result {
        Ok(msg) => {
            set_progress(&w, 100);
            set_status(s, &format!("✔ {msg}"));
            log_line(s, &format!("fin : {msg}"));
        }
        Err(e) => {
            set_progress(&w, 0);
            set_status(s, &format!("✘ Échec : {e}"));
            log_line(s, &format!("échec : {e}"));
        }
    }
    if let Some(dir) = s.job_dir.borrow_mut().take() {
        let _ = std::fs::remove_dir_all(dir);
    }
    *s.analysis.borrow_mut() = None;
    s.set_busy(false);
    refresh_devices(s);
}

fn start_inspect(s: &State) {
    if s.busy.get() {
        return;
    }
    let w = s.w();
    let Some(dev) = selected_device(s) else {
        set_status(s, "Aucune clé sélectionnée.");
        return;
    };
    if s.payload.borrow().is_none() {
        set_status(
            s,
            "Choisissez une image de release : la comparaison en a besoin.",
        );
        return;
    }
    let img = read_edit(w.img_edit).trim().to_string();
    let dir = std::env::temp_dir().join(format!(
        "l2u-key-inspect-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    if std::fs::create_dir_all(&dir).is_err() {
        set_status(s, "Dossier temporaire indisponible.");
        return;
    }
    let log = dir.join("job.log");
    let jobfile = dir.join("job.json");
    let task = Job {
        op: "inspect".into(),
        device: dev.path.clone(),
        payload: img,
        force: false,
        log: log.display().to_string(),
    };
    let Ok(json) = serde_json::to_string_pretty(&task) else {
        set_status(s, "Tâche invalide.");
        return;
    };
    if std::fs::write(&jobfile, json).is_err() {
        set_status(s, "Écriture de la tâche impossible.");
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        set_status(s, "Chemin du binaire introuvable.");
        return;
    };
    let mut cmd = if crate::is_root() {
        Command::new(&exe)
    } else {
        match find_in_path("pkexec") {
            Some(pk) => {
                let mut c = Command::new(pk);
                c.arg(&exe);
                c
            }
            None => {
                set_status(s, "pkexec (polkit) est introuvable.");
                return;
            }
        }
    };
    match cmd.arg("--apply").arg(&jobfile).spawn() {
        Ok(child) => {
            *s.child.borrow_mut() = Some(child);
            *s.log_file.borrow_mut() = Some(log);
            *s.job_dir.borrow_mut() = Some(dir);
            s.log_pos.set(0);
            *s.done.borrow_mut() = None;
            *s.analysis.borrow_mut() = None;
            set_progress_indeterminate(&w, true);
            set_status(s, "Analyse de la clé — autorisation système demandée…");
            s.set_busy(true);
        }
        Err(e) => set_status(s, &format!("Lancement de l'analyse impossible : {e}")),
    }
}

fn start_repo_check(s: &Rc<State>, tx: mpsc::Sender<Msg>) {
    if s.repo_busy.get() || s.busy.get() {
        return;
    }
    s.repo_busy.set(true);
    set_status(s, "Recherche de la dernière release publiée…");
    std::thread::spawn(move || match job::latest_release() {
        Ok((tag, Some(url))) => {
            let dl = job::download(&url, &job::cache_dir());
            let _ = match dl {
                Ok(path) => tx.send(Msg::Downloaded(Ok((tag, path)))),
                Err(e) => tx.send(Msg::Repo(Err(format!(
                    "{tag} : téléchargement impossible ({e})"
                )))),
            };
        }
        Ok((tag, None)) => {
            let _ = tx.send(Msg::Repo(Ok((tag, None))));
        }
        Err(e) => {
            let _ = tx.send(Msg::Repo(Err(e)));
        }
    });
}

// ---------------------------------------------------------------------------
// Construction de l'interface
// ---------------------------------------------------------------------------

struct Layouts(Vec<Box<dyn std::any::Any>>);

impl Layouts {
    fn new() -> Layouts {
        Layouts(Vec::new())
    }
    fn push<T: 'static>(&mut self, v: T) {
        self.0.push(Box::new(v));
    }
}

fn build_ui(
    window: &Rc<RefCell<MainWindow>>,
    s: &Rc<State>,
    tx: mpsc::Sender<Msg>,
) -> (Layouts, W) {
    let w = window.borrow();
    let wp: &dyn AsWidget = &*w;
    let file_menu = Menu::new("Fichier")
        .action("Quitter", || std::process::exit(0))
        .parent(wp)
        .build();
    let help_menu = Menu::new("Aide")
        .action("À propos", || {
            MessageBox::about(
                None,
                APP_TITLE,
                &format!(
                    "Gestionnaire de clé USB Legacy2UEFI\nversion {}\n\n\
                     Crée une clé qui apporte un environnement UEFI à un PC BIOS Legacy\n\
                     (OpenDuet + lanceur graphique), et la met à jour en place.\n\n\
                     Ne modifie jamais le firmware ni les disques internes.\n\
                     Licence GPL-3.0-or-later.",
                    env!("CARGO_PKG_VERSION")
                ),
            )
        })
        .parent(wp)
        .build();
    let menubar = MenuBar::new()
        .add_menu(file_menu)
        .add_menu(help_menu)
        .parent(wp)
        .build();
    w.set_menu_bar(&menubar);
    drop(w);

    let mut central = Widget::new().build();
    central.set_has_parent();
    let mut layouts = Layouts::new();
    let mut root = VBoxLayout::with_parent(&central);
    root.set_contents_margins(12, 12, 12, 12);
    root.set_spacing(10);

    // ---- 1. Clé USB --------------------------------------------------------
    // Un GroupBox ne supporte qu'un seul layout : une grille unique par section (les
    // sous-layouts posés sur le même GroupBox déclenchent « already has a layout »).
    let g1 = GroupBox::new("1. Clé USB").parent(&central).build();
    let mut g1l = GridLayout::with_parent(&g1);
    let dev_combo = ComboBox::new().parent(&g1).build();
    dev_combo.set_fixed_width(520);
    let dev_ptr = dev_combo.widget_ptr() as *mut qtrs::ffi::QComboBox;
    g1l.add(dev_combo, 0, 0, 1, 2);

    let refresh_btn = PushButton::new("Rafraîchir la liste")
        .on_clicked({
            let s = Rc::clone(s);
            move || {
                refresh_devices(&s);
                set_status(s.as_ref(), "Liste des périphériques actualisée.");
            }
        })
        .parent(&g1)
        .build();
    g1l.add(refresh_btn, 1, 0, 1, 1);
    let inspect_btn = PushButton::new("Analyser la clé")
        .on_clicked({
            let s = Rc::clone(s);
            move || start_inspect(&s)
        })
        .parent(&g1)
        .build();
    let inspect_ptr = inspect_btn.widget_ptr() as *mut qtrs::ffi::QPushButton;
    g1l.add(inspect_btn, 1, 1, 1, 1);

    let detail = Label::new("Aucune clé détectée.").parent(&g1).build();
    let detail_ptr = detail.widget_ptr() as *mut qtrs::ffi::QLabel;
    g1l.add(detail, 2, 0, 1, 2);
    layouts.push(g1l);
    root.add(g1);

    // ---- 2. Image de release ----------------------------------------------
    let g2 = GroupBox::new("2. Image de release Legacy2UEFI")
        .parent(&central)
        .build();
    let mut g2l = GridLayout::with_parent(&g2);
    let img_title = Label::new("Image du projet (.img ou .zip : legacy2uefi-<version>.img)")
        .parent(&g2)
        .build();
    g2l.add(img_title, 0, 0, 1, 3);

    let img_edit = LineEdit::new("").parent(&g2).build();
    let img_ptr = img_edit.widget_ptr() as *mut qtrs::ffi::QLineEdit;
    g2l.add(img_edit, 1, 0, 1, 1);
    let browse_btn = PushButton::new("Parcourir…")
        .on_clicked({
            let s = Rc::clone(s);
            move || {
                let start = read_edit(img_ptr);
                let dir = std::path::Path::new(start.trim())
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                if let Some(path) =
                    FileDialog::open_file(None, "Choisir l'image de release", &dir, IMAGE_FILTER)
                {
                    set_edit(img_ptr, &path);
                    set_status(s.as_ref(), "Image chargée, comparaison avec la clé…");
                    refresh_payload_info(&s);
                    refresh_verdict_label(&s);
                }
            }
        })
        .parent(&g2)
        .build();
    g2l.add(browse_btn, 1, 1, 1, 1);
    let repo_btn = PushButton::new("Vérifier le dépôt")
        .on_clicked({
            let s = Rc::clone(s);
            let tx = tx.clone();
            move || start_repo_check(&s, tx.clone())
        })
        .parent(&g2)
        .build();
    let repo_ptr = repo_btn.widget_ptr() as *mut qtrs::ffi::QPushButton;
    g2l.add(repo_btn, 1, 2, 1, 1);

    let img_info = Label::new("Aucune image choisie.").parent(&g2).build();
    let img_info_ptr = img_info.widget_ptr() as *mut qtrs::ffi::QLabel;
    g2l.add(img_info, 2, 0, 1, 3);
    layouts.push(g2l);
    root.add(g2);

    // ---- 3. Action ---------------------------------------------------------
    let g3 = GroupBox::new("3. Action").parent(&central).build();
    let mut g3l = GridLayout::with_parent(&g3);
    let key_info = Label::new("Clé : —").parent(&g3).build();
    let key_info_ptr = key_info.widget_ptr() as *mut qtrs::ffi::QLabel;
    g3l.add(key_info, 0, 0, 1, 2);
    let verdict = Label::new("Choisissez une image de release pour comparer.")
        .parent(&g3)
        .build();
    let verdict_ptr = verdict.widget_ptr() as *mut qtrs::ffi::QLabel;
    g3l.add(verdict, 1, 0, 1, 2);

    let create_btn = PushButton::new("Créer la clé")
        .on_clicked({
            let s = Rc::clone(s);
            move || start_job(&s, "create")
        })
        .parent(&g3)
        .build();
    let create_ptr = create_btn.widget_ptr() as *mut qtrs::ffi::QPushButton;
    g3l.add(create_btn, 2, 0, 1, 1);
    let update_btn = PushButton::new("Mettre à jour la clé")
        .on_clicked({
            let s = Rc::clone(s);
            move || {
                let v = s.verdict();
                if v == Some(Verdict::UpToDate) {
                    info(
                        "Déjà à jour",
                        "La clé contient déjà exactement le contenu de cette image.\nRien n'a été écrit.",
                    );
                    return;
                }
                start_job(&s, "update")
            }
        })
        .parent(&g3)
        .build();
    let update_ptr = update_btn.widget_ptr() as *mut qtrs::ffi::QPushButton;
    g3l.add(update_btn, 2, 1, 1, 1);

    let confirm_chk = CheckBox::new("J'ai compris que « Créer la clé » efface la cible")
        .on_toggled({
            let s = Rc::clone(s);
            move |_| s.refresh_buttons()
        })
        .parent(&g3)
        .build();
    let confirm_ptr = confirm_chk.widget_ptr() as *mut qtrs::ffi::QCheckBox;
    g3l.add(confirm_chk, 3, 0, 1, 2);
    layouts.push(g3l);
    root.add(g3);

    // ---- Progression, statut, journal --------------------------------------
    let status = Label::new("Prêt.").parent(&central).build();
    let status_ptr = status.widget_ptr() as *mut qtrs::ffi::QLabel;
    root.add(status);

    let bar = ProgressBar::new()
        .range(0, 100)
        .value(0)
        .format("%p %")
        .parent(&central)
        .build();
    let bar_ptr = bar.widget_ptr() as *mut qtrs::ffi::QProgressBar;
    root.add(bar);

    let journal_title = Label::new("Journal").parent(&central).build();
    root.add(journal_title);
    let logw = PlainTextEdit::new().parent(&central).build();
    logw.set_read_only(true);
    logw.set_line_wrap_mode(2); // 2 = WidgetWidth
    let log_ptr = logw.widget_ptr() as *mut qtrs::ffi::QPlainTextEdit;
    set_fixed_height(log_ptr as *mut qtrs::ffi::QWidget, 132);
    root.add(logw);

    layouts.push(root);

    let central_ptr = central.widget_ptr();
    window.borrow_mut().set_central_widget(&central);
    let w_out = W {
        dev_combo: dev_ptr,
        detail: detail_ptr,
        img_edit: img_ptr,
        img_info: img_info_ptr,
        key_info: key_info_ptr,
        verdict: verdict_ptr,
        status: status_ptr,
        bar: bar_ptr,
        log: log_ptr,
        create_btn: create_ptr,
        update_btn: update_ptr,
        repo_btn: repo_ptr,
        inspect_btn: inspect_ptr,
        confirm: confirm_ptr,
        central: central_ptr,
    };
    *s.w.borrow_mut() = w_out;
    (layouts, w_out)
}

// ---------------------------------------------------------------------------
// Pompe et point d'entrée
// ---------------------------------------------------------------------------

fn select_device(s: &State, name: &str) {
    if let Some(i) = s.devices.borrow().iter().position(|d| d.name == name) {
        unsafe { qtrs::ffi::QComboBox_setCurrentIndex(s.w().dev_combo, i as i32) };
        refresh_key(s);
    }
}

/// La sélection de la liste est détectée par la pompe (`textChanged` n'existe pas sur un
/// `QComboBox` de qtrs, et le signal de texte se déclenche pendant le remplissage).
fn pump_selection(s: &State) {
    if s.busy.get() {
        return;
    }
    let w = s.w();
    let text = unsafe { qtrs::ffi::QComboBox_currentText(w.dev_combo) };
    let mut last = s.sel.borrow_mut();
    if *last != text {
        *last = text;
        drop(last);
        refresh_key(s);
    }
}

fn pump_rx(s: &State, rx: &mpsc::Receiver<Msg>) {
    while let Ok(msg) = rx.try_recv() {
        let w = s.w();
        match msg {
            Msg::Repo(Ok((tag, None))) => {
                s.repo_busy.set(false);
                let keyv = s
                    .key_state
                    .borrow()
                    .as_ref()
                    .map(|k| k.version_or_unknown())
                    .or_else(|| s.analysis.borrow().as_ref().map(|a| a.version.clone()))
                    .unwrap_or_default();
                log_line(
                    s,
                    &format!(
                        "dépôt : dernière release {tag} · aucune image .img/.zip attachée · clé : {}",
                        if keyv.is_empty() { "inconnue" } else { &keyv }
                    ),
                );
                set_status(
                    s,
                    &format!(
                        "Dernière release publiée : {tag} — aucune image attachée, choisissez une image locale."
                    ),
                );
            }
            Msg::Repo(Ok((tag, Some(url)))) => {
                s.repo_busy.set(false);
                log_line(s, &format!("dépôt : release {tag}, image {url}"));
            }
            Msg::Repo(Err(e)) => {
                s.repo_busy.set(false);
                set_status(s, &format!("Vérification du dépôt : {e}"));
                log_line(s, &format!("dépôt : {e}"));
            }
            Msg::Downloaded(Ok((tag, path))) => {
                s.repo_busy.set(false);
                set_edit(w.img_edit, &path);
                log_line(s, &format!("dépôt : release {tag} téléchargée dans {path}"));
                set_status(s, "Image téléchargée, comparaison avec la clé…");
                refresh_payload_info(s);
                refresh_verdict_label(s);
            }
            Msg::Downloaded(Err(e)) => {
                s.repo_busy.set(false);
                set_status(s, &format!("Téléchargement impossible : {e}"));
                log_line(s, &format!("téléchargement : {e}"));
            }
        }
    }
}

pub fn run() -> i32 {
    let app = Application::new();
    app.set_desktop_file_name("l2u-key-gui");
    let window = Rc::new(RefCell::new(
        MainWindow::new()
            .window_title(APP_TITLE)
            .size(980, 800)
            .build(),
    ));
    let s: Rc<State> = Rc::new(State::default());
    let (tx, rx) = mpsc::channel::<Msg>();
    let (layouts, w) = build_ui(&window, &s, tx.clone());
    window.borrow().show();

    // Hooks de test : image préchargée et périphérique présélectionné.
    if let Ok(p) = std::env::var("L2U_KEY_GUI_IMG") {
        set_edit(w.img_edit, &p);
    }
    refresh_devices(&s);
    if let Ok(name) = std::env::var("L2U_KEY_GUI_DEV") {
        select_device(&s, &name);
    }
    refresh_payload_info(&s);
    refresh_verdict_label(&s);

    // La pompe doit vivre jusqu'à la fin de `exec()` : déclarée au niveau de `run`.
    let _timer = Timer::new(150)
        .on_timeout({
            let s = Rc::clone(&s);
            move || {
                pump_job(&s);
                pump_rx(&s, &rx);
                pump_selection(&s);
            }
        })
        .build();

    // Auto-test : déclenche une analyse ou une mise à jour de clé, sans clic souris.
    if let Ok(what) = std::env::var("L2U_KEY_GUI_SELFTEST") {
        eprintln!("SELFTEST armed");
        let s2 = Rc::clone(&s);
        Timer::single_shot(600, move || {
            eprintln!("SELFTEST tick");
            unsafe { qtrs::ffi::QCheckBox_setChecked(s2.w().confirm, true) };
            start_job(
                &s2,
                if what == "update" {
                    "update"
                } else {
                    "inspect"
                },
            );
            eprintln!("SELFTEST job started");
            let s3 = Rc::clone(&s2);
            Timer::single_shot(3000, move || {
                println!("SELFTEST status={}", s3.last_status.borrow());
                for l in s3.last_log.borrow().iter() {
                    println!("SELFTEST log={l}");
                }
                let ok =
                    s3.last_log.borrow().iter().any(|l| {
                        l.contains("OK —") || l.contains("(à jour)") || l.contains("clé :")
                    });
                println!("SELFTEST ok={ok}");
                std::process::exit(if ok { 0 } else { 1 });
            });
        });
    }

    if let Ok(ms) = std::env::var("L2U_KEY_GUI_SMOKE_MS") {
        let ms: i32 = ms.parse().unwrap_or(1500);
        Timer::single_shot(ms, || std::process::exit(0));
    }
    if std::env::var("L2U_KEY_GUI_GEOM").is_ok() {
        Timer::single_shot(900, move || unsafe {
            let cw = qtrs::ffi::QWidget_width(w.central);
            let ch = qtrs::ffi::QWidget_height(w.central);
            let lh = qtrs::ffi::QWidget_height(w.log as *mut qtrs::ffi::QWidget);
            let bw = qtrs::ffi::QWidget_width(w.bar as *mut qtrs::ffi::QWidget);
            println!("GEOM central={cw}x{ch} log_h={lh} bar_w={bw}");
            if cw < 900 || ch < 700 || lh < 80 || bw < 300 {
                println!("GEOM_FAIL");
                std::process::exit(1);
            }
            std::process::exit(0);
        });
    }

    let code = app.exec();
    drop(layouts);
    code
}
