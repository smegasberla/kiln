//! Detect whether this target is a GTK platform.
//!
//! Linux and the BSDs share tao's GTK backend and wry's `webkitgtk` module, so the
//! menu is a GTK container and the webview is a GTK widget there. Windows, macOS,
//! Android and iOS have neither. Rather than ask every app to enable a
//! platform-specific feature, the crate sets a cfg here for the target actually
//! being compiled.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(voussoir_gtk_menu)");
    println!("cargo::rustc-check-cfg=cfg(voussoir_gtk_webview)");

    let gtk = std::env::var("CARGO_CFG_TARGET_OS")
        .map(|os| {
            matches!(
                os.as_str(),
                "linux" | "freebsd" | "netbsd" | "openbsd" | "dragonfly"
            )
        })
        .unwrap_or(false);

    if gtk {
        println!("cargo::rustc-cfg=voussoir_gtk_menu");
        println!("cargo::rustc-cfg=voussoir_gtk_webview");
    }
    println!("cargo::rerun-if-changed=build.rs");
}
