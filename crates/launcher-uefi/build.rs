// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Legacy2UEFI contributors
// Base font sizes used in ui/launcher.slint (all written as `<base>px * root.s`) and the UI scale steps
// of src/platform.rs (SCALE_STEPS). The software renderer embeds glyphs only at known pixel sizes, so every
// base × step size is listed in SLINT_FONT_SIZES for the Slint compiler. Keep the three lists in sync.
const BASE_SIZES: [f32; 9] = [18.0, 20.0, 22.0, 23.0, 26.0, 30.0, 32.0, 44.0, 15.0];
const SCALE_STEPS: [f32; 8] = [1.0, 0.85, 0.75, 0.667, 0.6, 0.533, 0.45, 0.4];

fn main() {
    let mut sizes: Vec<i32> = BASE_SIZES
        .iter()
        .flat_map(|b| SCALE_STEPS.iter().map(move |s| (b * s).round() as i32))
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    let list = sizes.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
    // SAFETY: build scripts are single-threaded at this point; the variable is read by slint-build below.
    unsafe { std::env::set_var("SLINT_FONT_SIZES", &list) };
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ui/launcher.slint");

    slint_build::compile_with_config(
        "ui/launcher.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent-dark".to_owned())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer),
    )
    .expect("Slint UI compilation failed");
}
