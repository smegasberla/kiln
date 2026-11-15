//! Turn on the Linux half of the `menu` feature automatically.
//!
//! The menu is built with `muda`, which needs a GTK container on Linux and has no
//! such dependency on Windows or macOS. Rather than make every app say
//! `features = ["menu", "menu-gtk"]` on Linux only, the crate enables it here for
//! the target actually being compiled.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(kiln_gtk_menu)");

    let linux = std::env::var("CARGO_CFG_TARGET_OS")
        .map(|os| !matches!(os.as_str(), "windows" | "macos" | "ios" | "android"))
        .unwrap_or(false);

    if linux {
        println!("cargo::rustc-cfg=kiln_gtk_menu");
    }
    println!("cargo::rerun-if-changed=build.rs");
}
