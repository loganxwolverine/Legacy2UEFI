// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Legacy2UEFI boot screen v0.2 (ADR-0008, revised 2026-11-20).
//! First program started by the key (`\EFI\BOOT\BOOTx64.efi`): loads the key's drivers, finds the
//! bootable systems and USB keys, shows them with the machine's measured specs, and starts the choice.
//! OpenCore stays on the key as the text-mode fallback. A graphics failure falls back to a text menu;
//! a panic falls back to OpenCore: the launcher must never prevent booting.
#![no_main]
#![no_std]

extern crate alloc;

#[macro_use]
mod steplog;
mod boot;
mod cpu;
mod platform;
mod smbios;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, Ordering};
use core::time::Duration;
use slint::{ModelRc, VecModel};
use uefi::prelude::*;
use uefi::runtime::ResetType;

slint::include_modules!();

const VERSION: &str = concat!("Legacy2UEFI ", env!("CARGO_PKG_VERSION"));
const UNKNOWN: &str = "inconnu";
const COUNTDOWN_S: i32 = 10;
const TOOLS: [&str; 4] = ["UEFI Shell", "Menu OpenCore (texte)", "Redémarrer", "Éteindre"];

static CHOICE: AtomicI32 = AtomicI32::new(-1);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    slog!("PANIC: {} ({:?})", info.message(), info.location());
    steplog::flush();
    uefi::println!("Legacy2UEFI : erreur interne ({}).", info.message());
    uefi::println!("Passage au menu OpenCore.");
    let _ = boot::launch_own(boot::OPENCORE_PATH);
    uefi::boot::stall(Duration::from_secs(10));
    uefi::runtime::reset(ResetType::COLD, Status::ABORTED, None)
}

struct Row {
    label: &'static str,
    value: Option<String>,
}

fn join(parts: &[&Option<String>]) -> Option<String> {
    let v: Vec<&str> = parts.iter().filter_map(|p| p.as_deref()).collect();
    (!v.is_empty()).then(|| v.join(" "))
}

fn collect_specs() -> (Vec<Row>, bool, String) {
    let sm = smbios::read();
    let cpu = cpu::read();
    let s = sm.as_ref();
    let mut rows = Vec::new();
    rows.push(Row { label: "Machine", value: s.and_then(|s| join(&[&s.sys_manufacturer, &s.sys_product])) });
    rows.push(Row {
        label: "BIOS",
        value: s.and_then(|s| {
            let base = join(&[&s.bios_vendor, &s.bios_version])?;
            Some(match &s.bios_date { Some(d) => format!("{base}, {d}"), None => base })
        }),
    });
    rows.push(Row { label: "Processeur", value: cpu.brand.clone() });
    rows.push(Row {
        label: "Niveau x86-64",
        value: Some(match cpu.missing_for_next {
            Some(m) => format!("v{} (pas de {} pour v{})", cpu.level, m, cpu.level + 1),
            None => format!("v{}", cpu.level),
        }),
    });
    let (mem, mem_type) = match s {
        Some(s) if !s.dimms.is_empty() => {
            let pop: Vec<&smbios::Dimm> = s.dimms.iter().filter(|d| d.populated).collect();
            let total: Option<u32> = pop.iter().map(|d| d.size_mb).sum();
            let mut txt = match total {
                Some(mb) if mb >= 1024 && mb % 1024 == 0 => format!("{} Go", mb / 1024),
                Some(mb) => format!("{mb} Mo"),
                None => UNKNOWN.to_string(),
            };
            if let Some(sp) = pop.iter().filter_map(|d| d.speed_mts).min() {
                txt.push_str(&format!(", {sp} MT/s"));
            }
            txt.push_str(&format!(", {} barrettes sur {}", pop.len(), s.dimms.len()));
            (Some(txt), pop.iter().find_map(|d| smbios::mem_type_name(d.mem_type)).map(|t| t.to_string()))
        }
        _ => (None, None),
    };
    rows.push(Row { label: "Mémoire", value: mem });
    rows.push(Row { label: "Type de mémoire", value: mem_type });
    rows.push(Row { label: "Affichage", value: platform::resolution().map(|(w, h)| format!("{w} x {h}")) });
    let vendor = String::from_utf16_lossy(uefi::system::firmware_vendor().to_u16_slice());
    rows.push(Row { label: "Environnement UEFI", value: Some(format!("{} (UEFI {})", vendor, uefi::system::uefi_revision())) });
    let ok = vendor.contains("Acidanthera");
    let status = if ok { "Passerelle UEFI active".to_string() } else { format!("Firmware UEFI : {vendor}") };
    (rows, ok, status)
}

enum Action {
    Boot(usize),
    Tool(usize),
}

fn decode(choice: i32, n_entries: usize) -> Option<Action> {
    let c = usize::try_from(choice).ok()?;
    if c < n_entries {
        Some(Action::Boot(c))
    } else if c - n_entries < TOOLS.len() {
        Some(Action::Tool(c - n_entries))
    } else {
        None
    }
}

/// Runs an action. Returns a message to show if control comes back to the launcher.
fn perform(action: Action, entries: &[boot::BootEntry]) -> String {
    let r = match action {
        Action::Boot(i) => boot::launch(entries[i].device, entries[i].path),
        Action::Tool(0) => boot::launch_own(boot::SHELL_PATH),
        Action::Tool(1) => boot::launch_own(boot::OPENCORE_PATH),
        Action::Tool(2) => uefi::runtime::reset(ResetType::COLD, Status::SUCCESS, None),
        Action::Tool(_) => uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None),
    };
    platform::set_preferred_mode();
    match r {
        Ok(()) => String::new(),
        Err(e) => format!("Échec : {e}"),
    }
}

fn text_menu(entries: &[boot::BootEntry], rows: &[Row]) -> ! {
    slog!("mode texte de secours");
    steplog::flush();
    loop {
        uefi::println!("\nLegacy2UEFI - menu de démarrage (mode texte)");
        for r in rows {
            uefi::println!("  {:<20} {}", r.label, r.value.as_deref().unwrap_or(UNKNOWN));
        }
        for (i, e) in entries.iter().enumerate() {
            uefi::println!("  [{}] {} - {}", i + 1, e.title, e.subtitle);
        }
        uefi::println!("  [S] UEFI Shell   [O] Menu OpenCore   [R] Redémarrer");
        let key = uefi::system::with_stdin(|stdin| loop {
            if let Ok(ev) = stdin.wait_for_key_event() {
                let _ = uefi::boot::wait_for_event(&mut [ev]);
            }
            if let Ok(Some(uefi::proto::console::text::Key::Printable(c))) = stdin.read_key() {
                break char::from(c).to_ascii_uppercase();
            }
        });
        let action = match key {
            'S' => Some(Action::Tool(0)),
            'O' => Some(Action::Tool(1)),
            'R' => Some(Action::Tool(2)),
            d if d.is_ascii_digit() => (d as usize).checked_sub('1' as usize).filter(|&i| i < entries.len()).map(Action::Boot),
            _ => None,
        };
        if let Some(a) = action {
            let msg = perform(a, entries);
            if !msg.is_empty() {
                uefi::println!("{msg}");
            }
        }
    }
}

#[entry]
fn main() -> Status {
    let _ = uefi::boot::set_watchdog_timer(0, 0x10000, None);
    slog!("===== {VERSION} =====");
    slog!("clé Legacy2UEFI trouvée : {}", boot::own_device().is_some());
    // Drivers first (keyboard, storage), then connect everything, then look for systems.
    let n = boot::load_drivers();
    slog!("pilotes chargés depuis \\EFI\\L2U\\drivers : {n}");
    let entries = boot::scan();
    slog!("entrées : {}", entries.len());
    platform::set_preferred_mode();
    let (rows, bridge_ok, status) = collect_specs();
    steplog::flush();

    let ui = match platform::Platform::new() {
        None => {
            slog!("GUI: pas de sortie graphique (GOP)");
            text_menu(&entries, &rows)
        }
        Some(p) => match slint::platform::set_platform(Box::new(p)) {
            Err(e) => {
                slog!("GUI: set_platform a échoué : {e:?}");
                text_menu(&entries, &rows)
            }
            Ok(()) => match Launcher::new() {
                Err(e) => {
                    slog!("GUI: création de l'écran impossible : {e:?}");
                    text_menu(&entries, &rows)
                }
                Ok(ui) => ui,
            },
        },
    };
    slog!("GUI: écran créé");

    let specs: Vec<Spec> = rows
        .iter()
        .map(|r| Spec {
            label: r.label.into(),
            known: r.value.is_some(),
            value: match (&r.value, r.label) {
                (Some(v), _) => v.as_str().into(),
                (None, "Type de mémoire") => "non déclaré par le BIOS".into(),
                (None, _) => UNKNOWN.into(),
            },
        })
        .collect();
    let items: Vec<Item> = entries
        .iter()
        .map(|e| Item {
            title: e.title.as_str().into(),
            subtitle: e.subtitle.as_str().into(),
            kind: if e.path.is_empty() { 3 } else { e.removable as i32 },
        })
        .collect();
    let tools: Vec<Item> = TOOLS.iter().map(|t| Item { title: (*t).into(), subtitle: "".into(), kind: 2 }).collect();
    ui.set_specs(ModelRc::new(VecModel::from(specs)));
    ui.set_items(ModelRc::new(VecModel::from(items)));
    ui.set_tools(ModelRc::new(VecModel::from(tools)));
    ui.set_bridge_ok(bridge_ok);
    ui.set_bridge_status(status.as_str().into());
    ui.set_version(VERSION.into());
    if let Some(res) = platform::resolution() {
        let s = platform::ui_scale(res);
        slog!("UI: résolution {:?}, échelle {}", res, s);
        ui.set_s(s);
    }

    // Owner decision (2026-11-20): this screen is where the user chooses. No automatic boot, no countdown:
    // the screen waits. The cursor starts on the first bootable entry.
    let first = entries.iter().position(|e| !e.path.is_empty()).unwrap_or(0);
    ui.set_default_index(-1);
    ui.set_selected(first as i32);
    ui.set_countdown(COUNTDOWN_S);
    ui.set_counting(false);

    ui.on_activate(|i| {
        CHOICE.store(i, Ordering::Relaxed);
        platform::request_quit();
    });

    steplog::flush();
    loop {
        CHOICE.store(-1, Ordering::Relaxed);
        if let Err(e) = ui.run() {
            slog!("GUI: boucle d'affichage arrêtée : {e:?}");
            text_menu(&entries, &rows);
        }
        if let Some(action) = decode(CHOICE.load(Ordering::Relaxed), entries.len()) {
            match &action {
                Action::Boot(i) => slog!("choix : {} ({})", entries[*i].title, entries[*i].subtitle),
                Action::Tool(t) => slog!("choix : {}", TOOLS[*t]),
            }
            steplog::flush();
            let msg = perform(action, &entries);
            slog!("retour au menu : {}", if msg.is_empty() { "sans erreur" } else { msg.as_str() });
            steplog::flush();
            ui.set_message(msg.as_str().into());
            ui.set_counting(false);
        }
    }
}
