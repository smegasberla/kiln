//! Scratch on the desktop.
//!
//! The app itself is in `app.rs`, shared verbatim with the Android shell. What is
//! here is the desktop half of the difference: a menu bar with real accelerators,
//! a tray, and the fact that the app runs from `main` rather than from an activity.

mod app;
mod assets;

use kiln::menu::{MenuGroup, MenuItem};
use kiln::serde_json::json;
use kiln::tray::{TrayConfig, TrayItem};
use kiln::Error;

fn main() -> Result<(), Error> {
    app::build("Scratch")
        // Desktop gets a menu bar, and the accelerators are real: cmd/ctrl+N and
        // cmd/ctrl+S arrive as the same events a click does, so the page has one
        // handler for both and no key handling of its own.
        .menu(
            MenuGroup::new("Note")
                .item(MenuItem::new("new", "New Note").accelerator("CmdOrCtrl+N"))
                .item(MenuItem::new("save", "Save").accelerator("CmdOrCtrl+S"))
                .item(MenuItem::new("delete", "Delete Note").accelerator("CmdOrCtrl+Backspace")),
        )
        // Android shows the same groups as a PopupMenu the page raises; the page
        // hides the overflow button on desktop, where the menu bar is already there.
        .tray(
            TrayConfig::new("Scratch")
                .item(TrayItem::new("new", "New note"))
                .item(TrayItem::new("show", "Show Scratch")),
        )
        .on_ready(|app, _window| {
            // The directory is what a user actually needs from a notebook app, so
            // it is reported once the page can display it.
            if let Some(dir) = kiln::data_dir("Scratch") {
                app.emit("location", json!(dir.display().to_string()));
            }
        })
        .run()
}
