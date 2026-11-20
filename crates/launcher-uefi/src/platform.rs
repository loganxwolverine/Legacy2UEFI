// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Slint platform for UEFI: software renderer → GOP Blt; keyboard read from every text-input device.
//! Adapted from Slint's uefi-demo (MIT), without mouse support (ADR-0008: keyboard first).

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, TargetPixel,
};
use slint::platform::WindowEvent;
use slint::SharedString;
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::text::Input;
use uefi::{Char16, Handle};

static QUIT: AtomicBool = AtomicBool::new(false);

pub fn request_quit() {
    QUIT.store(true, Ordering::Relaxed);
}

/// Preferred resolution: the UI is drawn for 1920×1080 with fixed font sizes (the software renderer
/// embeds glyphs at the sizes used in the .slint file, so text cannot be scaled at runtime).
pub const TARGET: (usize, usize) = (1920, 1080);

/// 0x00RRGGBB, i.e. the in-memory layout of a UEFI `BltPixel` (blue, green, red, reserved).
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
struct Px(u32);

impl TargetPixel for Px {
    fn blend(&mut self, c: PremultipliedRgbaColor) {
        let inv = 255 - c.alpha as u32;
        let ch = |shift: u32, v: u8| ((((self.0 >> shift) & 0xFF) * inv) / 255 + v as u32).min(255) << shift;
        self.0 = ch(16, c.red) | ch(8, c.green) | ch(0, c.blue);
    }
    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Px(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }
}

fn timer_tick() -> u64 {
    // SAFETY: RDTSC has no side effects.
    unsafe { core::arch::x86_64::_rdtsc() }
}

fn timer_freq() -> u64 {
    let start = timer_tick();
    boot::stall(Duration::from_millis(1));
    (timer_tick() - start).max(1) * 1000
}

fn params(handle: Handle) -> OpenProtocolParams {
    OpenProtocolParams { handle, agent: boot::image_handle(), controller: None }
}

fn open_gop() -> Option<ScopedProtocol<GraphicsOutput>> {
    let h = boot::get_handle_for_protocol::<GraphicsOutput>().ok()?;
    // SAFETY: exclusive open does not work on real firmware (the console owns GOP); shared access.
    unsafe { boot::open_protocol::<GraphicsOutput>(params(h), OpenProtocolAttributes::GetProtocol).ok() }
}

/// Current GOP resolution, if a graphics output exists.
pub fn resolution() -> Option<(usize, usize)> {
    Some(open_gop()?.current_mode_info().resolution())
}

/// Native resolution of the attached display from its EDID (first detailed timing descriptor), if the
/// firmware exposes EDID on the GOP handle.
fn edid_native() -> Option<(usize, usize)> {
    let h = boot::get_handle_for_protocol::<GraphicsOutput>().ok()?;
    // SAFETY: shared, read-only access.
    let ed = unsafe {
        boot::open_protocol::<uefi::proto::console::gop::EdidDiscovered>(params(h), OpenProtocolAttributes::GetProtocol).ok()?
    };
    let e = ed.edid()?;
    if e.len() < 128 || e[0..8] != [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00] {
        return None;
    }
    let d = &e[54..72];
    if d[0] == 0 && d[1] == 0 {
        return None; // not a timing descriptor
    }
    let w = d[2] as usize | ((d[4] as usize & 0xF0) << 4);
    let hh = d[5] as usize | ((d[7] as usize & 0xF0) << 4);
    (w >= 640 && hh >= 480).then_some((w, hh))
}

/// Universal mode choice (owner, 2026-11-20: one screen design must work on every machine):
/// 1. the display's native resolution (EDID) if offered and ≤ 1920×1080;
/// 2. else the largest offered mode ≤ 1920×1080 (ThinkPad 4414WBF: started in 800×600, offers up to
///    1024×768; Aurora R1 on a 4K monitor: offers 3840×2160 and 1920×1080).
/// The UI then scales to whatever resolution results (see `ui_scale`).
pub fn set_preferred_mode() -> Option<(usize, usize)> {
    let mut gop = open_gop()?;
    let modes: Vec<_> = gop.modes().collect();
    let list: Vec<alloc::string::String> = modes
        .iter()
        .map(|m| { let (w, h) = m.info().resolution(); alloc::format!("{w}x{h}") })
        .collect();
    let current = gop.current_mode_info().resolution();
    let native = edid_native();
    crate::slog!("GOP: mode actuel {:?}, EDID natif {:?}, modes proposés : {}", current, native, list.join(" "));
    let fits = |r: (usize, usize)| r.0 <= TARGET.0 && r.1 <= TARGET.1;
    let offered = |r: (usize, usize)| modes.iter().find(|m| m.info().resolution() == r);
    let target = match native {
        Some(n) if fits(n) && offered(n).is_some() => n,
        _ => modes
            .iter()
            .map(|m| m.info().resolution())
            .filter(|&r| fits(r))
            .max_by_key(|&(w, h)| w * h)
            .unwrap_or(current),
    };
    if target != current {
        if let Some(m) = offered(target) {
            let _ = gop.set_mode(m);
        }
    }
    let r = gop.current_mode_info().resolution();
    crate::slog!("GOP: mode retenu {:?}", r);
    Some(r)
}

/// Discrete UI scale steps; the build embeds glyphs for every base font size × step (build.rs).
pub const SCALE_STEPS: [f32; 8] = [1.0, 0.85, 0.75, 0.667, 0.6, 0.533, 0.45, 0.4];

/// Largest step not exceeding min(w/1920, h/1080): 1080p → 1.0, 1600×900 → 0.75 (0.83 rounds down),
/// 1366×768 / 1280×800 → 0.667, 1024×768 → 0.533, 800×600 → 0.4.
pub fn ui_scale((w, h): (usize, usize)) -> f32 {
    let s = (w as f32 / TARGET.0 as f32).min(h as f32 / TARGET.1 as f32);
    SCALE_STEPS.iter().copied().find(|&st| st <= s + 0.001).unwrap_or(0.4)
}

struct Keyboards(Vec<ScopedProtocol<Input>>);

impl Keyboards {
    /// Every Simple Text Input instance (console splitter and each physical keyboard). Reading them
    /// directly does not depend on the firmware having added a newly loaded keyboard to ConIn.
    fn open() -> Self {
        let mut v = Vec::new();
        if let Ok(hs) = boot::find_handles::<Input>() {
            for h in hs {
                // SAFETY: shared access; keys are read, nothing is reconfigured.
                if let Ok(p) = unsafe { boot::open_protocol::<Input>(params(h), OpenProtocolAttributes::GetProtocol) } {
                    v.push(p);
                }
            }
        }
        crate::slog!("claviers (Simple Text Input) : {}", v.len());
        Keyboards(v)
    }

    fn read(&mut self) -> Option<char> {
        use slint::platform::Key::*;
        use uefi::proto::console::text::{Key as UefiKey, ScanCode as Scan};
        let cr = Char16::try_from('\r').ok()?;
        for kb in self.0.iter_mut() {
            let k = match kb.read_key() {
                Ok(Some(k)) => k,
                _ => continue,
            };
            return match k {
                UefiKey::Printable(c) if c == cr => Some('\n'),
                UefiKey::Printable(c) => Some(char::from(c)),
                UefiKey::Special(s) => Some(
                    match s {
                        Scan::UP => UpArrow,
                        Scan::DOWN => DownArrow,
                        Scan::RIGHT => RightArrow,
                        Scan::LEFT => LeftArrow,
                        Scan::HOME => Home,
                        Scan::END => End,
                        Scan::PAGE_UP => PageUp,
                        Scan::PAGE_DOWN => PageDown,
                        Scan::ESCAPE => Escape,
                        _ => continue,
                    }
                    .into(),
                ),
            };
        }
        None
    }

    fn wait(&self, max: Option<Duration>) {
        use uefi::boot::{EventType, TimerTrigger, Tpl};
        let timeout = Duration::from_millis(500).min(max.unwrap_or(Duration::from_millis(500)));
        // SAFETY: the event is closed before returning.
        let Ok(timer) = (unsafe { boot::create_event(EventType::TIMER, Tpl::APPLICATION, None, None) }) else {
            boot::stall(Duration::from_millis(20));
            return;
        };
        let _ = boot::set_timer(&timer, TimerTrigger::Relative(timeout));
        let mut events = Vec::new();
        for kb in &self.0 {
            if let Ok(e) = kb.wait_for_key_event() {
                events.push(e);
            }
        }
        // SAFETY: the clone only lives for this wait.
        events.push(unsafe { timer.unsafe_clone() });
        let _ = boot::wait_for_event(&mut events);
        drop(events);
        let _ = boot::close_event(timer);
    }
}

pub struct Platform {
    window: Rc<MinimalSoftwareWindow>,
    freq: f64,
    start: f64,
}

impl Platform {
    pub fn new() -> Option<Self> {
        open_gop()?;
        Some(Self {
            window: MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer),
            freq: timer_freq() as f64,
            start: timer_tick() as f64,
        })
    }
}

impl slint::platform::Platform for Platform {
    fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        Duration::from_secs_f64((timer_tick() as f64 - self.start) / self.freq)
    }

    fn run_event_loop(&self) -> Result<(), slint::PlatformError> {
        QUIT.store(false, Ordering::Relaxed);
        let mut gop = open_gop().ok_or_else(|| slint::PlatformError::Other("GOP unavailable".into()))?;
        let (w, h) = gop.current_mode_info().resolution();
        let mut fb = alloc::vec![Px::default(); w * h];
        self.window.set_size(slint::PhysicalSize::new(w as u32, h as u32));
        self.window.request_redraw();
        let mut kbd = Keyboards::open();

        loop {
            slint::platform::update_timers_and_animations();
            while let Some(key) = kbd.read() {
                let text = SharedString::from(key);
                self.window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
                self.window.dispatch_event(WindowEvent::KeyReleased { text });
            }
            if QUIT.load(Ordering::Relaxed) {
                return Ok(());
            }
            self.window.draw_if_needed(|renderer| {
                renderer.render(&mut fb, w);
                // SAFETY: Px and BltPixel have the same size and byte layout (B, G, R, reserved).
                let blt = unsafe { core::slice::from_raw_parts(fb.as_ptr() as *const BltPixel, fb.len()) };
                let _ = gop.blt(BltOp::BufferToVideo { buffer: blt, src: BltRegion::Full, dest: (0, 0), dims: (w, h) });
            });
            if QUIT.load(Ordering::Relaxed) {
                return Ok(());
            }
            if !self.window.has_active_animations() {
                kbd.wait(slint::platform::duration_until_next_timer_update());
            }
        }
    }
}
