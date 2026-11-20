// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
//! Step log kept in memory and appended to `\L2U-LAUNCHER-LOG.txt` on the Legacy2UEFI key only, so a
//! failed start on a test machine can be diagnosed afterwards. Never written anywhere else.
//! Append-only: v0.3 rewrote the file (delete + create) and the second rewrite silently failed on
//! OpenDuet's FAT driver.

use alloc::string::String;
use core::cell::{Cell, RefCell};
use core::fmt::Write;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::CString16;

struct Log {
    text: RefCell<String>,
    flushed: Cell<usize>,
}
// SAFETY: UEFI boot services are single-threaded; the launcher never runs code on other processors.
unsafe impl Sync for Log {}

static LOG: Log = Log { text: RefCell::new(String::new()), flushed: Cell::new(0) };
const LOG_PATH: &str = "\\L2U-LAUNCHER-LOG.txt";

pub fn line(args: core::fmt::Arguments) {
    if let Ok(mut s) = LOG.text.try_borrow_mut() {
        let _ = s.write_fmt(args);
        s.push_str("\r\n");
    }
}

#[macro_export]
macro_rules! slog {
    ($($t:tt)*) => { $crate::steplog::line(format_args!($($t)*)) };
}

/// Appends what was logged since the previous flush to the log file on the key.
pub fn flush() {
    let Ok(s) = LOG.text.try_borrow() else { return };
    let from = LOG.flushed.get();
    if from >= s.len() {
        return;
    }
    let Ok(mut fs) = uefi::boot::get_image_file_system(uefi::boot::image_handle()) else { return };
    let Ok(mut root) = fs.open_volume() else { return };
    let Ok(path) = CString16::try_from(LOG_PATH) else { return };
    let Ok(f) = root.open(&path, FileMode::CreateReadWrite, FileAttribute::empty()) else { return };
    let Some(mut f) = f.into_regular_file() else { return };
    // Position u64::MAX = end of file (UEFI File Protocol SetPosition).
    if f.set_position(u64::MAX).is_err() {
        return;
    }
    if f.write(&s.as_bytes()[from..]).is_ok() {
        LOG.flushed.set(s.len());
    }
    let _ = f.flush();
}
