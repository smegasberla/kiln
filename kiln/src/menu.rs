//! Optional native menu bar.
//!
//! Builds the menu a desktop app is expected to have — Cut/Copy/Paste/Select All
//! and Quit from the platform, plus app-defined items — and turns clicks into
//! events the page receives as `kiln.listen("menu", ({id}) => ...)`.
//!
//! Predefined items are used wherever possible because they carry the platform's
//! own labels and shortcuts, which is also what makes Cmd-C and Ctrl-C work in the
//! webview without any key handling of ours.

use crate::WindowOptions;

/// One app-defined menu item.
#[derive(Debug, Clone)]
pub struct MenuItem {
    /// Identifier sent to the page when the item is clicked.
    pub id: String,
    pub label: String,
    /// Optional accelerator, e.g. `"CmdOrCtrl+S"`.
    pub accelerator: Option<String>,
    /// Optional command run when the item is clicked, so a menu item can be
    /// wired straight to a registered command.
    pub command: Option<String>,
}

impl MenuItem {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            accelerator: None,
            command: None,
        }
    }

    pub fn accelerator(mut self, accel: &str) -> Self {
        self.accelerator = Some(accel.to_owned());
        self
    }

    pub fn command(mut self, command: &str) -> Self {
        self.command = Some(command.to_owned());
        self
    }
}

/// A named top-level menu holding app items.
#[derive(Debug, Clone)]
pub struct MenuGroup {
    pub title: String,
    pub items: Vec<MenuItem>,
}

impl MenuGroup {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            items: Vec::new(),
        }
    }

    pub fn item(mut self, item: MenuItem) -> Self {
        self.items.push(item);
        self
    }
}

/// A built menu, kept alive for as long as the app runs.
pub struct MenuHandle {
    pub(crate) menu: muda::Menu,
}

impl MenuHandle {
    /// Attach the menu to a window.
    ///
    /// On Linux it goes into the window's GTK container; on Windows it becomes
    /// the window's menu bar; on macOS it becomes the application menu.
    pub fn attach(&self, window: &tao::window::Window) -> Result<(), String> {
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            use tao::platform::unix::WindowExtUnix;

            self.menu
                .init_for_gtk_window(window.gtk_window(), None::<&gtk::Box>)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "windows")]
        {
            use tao::platform::windows::WindowExtWindows;
            // SAFETY: the handle is the window's own and outlives this call.
            unsafe {
                self.menu
                    .init_for_hwnd(window.hwnd() as isize)
                    .map_err(|e| e.to_string())?;
            }
        }
        #[cfg(target_os = "macos")]
        {
            self.menu.init_for_nsapp();
        }
        let _ = window;
        Ok(())
    }
}

/// Create a menu with the platform-standard edit items plus the given groups.
pub fn build(groups: &[MenuGroup]) -> Result<MenuHandle, String> {
    use muda::{Menu, MenuItem as MudaItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new();

    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::new("Kiln", true);
        app_menu
            .append(&PredefinedMenuItem::quit(Some("Quit")))
            .map_err(|e| e.to_string())?;
        menu.append(&app_menu).map_err(|e| e.to_string())?;
    }

    let edit = Submenu::new("Edit", true);
    for item in [
        PredefinedMenuItem::cut(Some("Cut")),
        PredefinedMenuItem::copy(Some("Copy")),
        PredefinedMenuItem::paste(Some("Paste")),
        PredefinedMenuItem::select_all(Some("Select All")),
    ] {
        edit.append(&item).map_err(|e| e.to_string())?;
    }
    menu.append(&edit).map_err(|e| e.to_string())?;

    for group in groups {
        let submenu = Submenu::new(&group.title, true);
        for item in &group.items {
            let accel = item
                .accelerator
                .as_deref()
                .and_then(parse_accelerator);
            let menu_item = MudaItem::with_id(&item.id, &item.label, true, accel);
            submenu.append(&menu_item).map_err(|e| e.to_string())?;
        }
        #[cfg(not(target_os = "macos"))]
        submenu
            .append(&PredefinedMenuItem::separator())
            .map_err(|e| e.to_string())?;
        #[cfg(not(target_os = "macos"))]
        submenu
            .append(&PredefinedMenuItem::quit(Some("Quit")))
            .map_err(|e| e.to_string())?;
        menu.append(&submenu).map_err(|e| e.to_string())?;
    }

    Ok(MenuHandle { menu })
}

/// Parse an accelerator string, tolerating the two spellings muda accepts.
fn parse_accelerator(text: &str) -> Option<muda::accelerator::Accelerator> {
    use std::str::FromStr;
    muda::accelerator::Accelerator::from_str(text).ok()
}

/// The payload the page receives when a menu item is clicked.
pub fn event_payload(id: &str) -> serde_json::Value {
    serde_json::json!({ "id": id })
}

/// Options for a window opened from a menu item.
pub fn window_options(title: &str) -> WindowOptions {
    WindowOptions {
        title: Some(title.to_owned()),
        ..WindowOptions::default()
    }
}

/// Accelerators declared by a set of groups, for the unit tests and for docs.
pub fn accelerators(groups: &[MenuGroup]) -> Vec<(String, String)> {
    groups
        .iter()
        .flat_map(|g| g.items.iter())
        .filter_map(|item| {
            item.accelerator
                .as_ref()
                .map(|accel| (item.id.clone(), accel.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> MenuGroup {
        MenuGroup::new("File")
            .item(MenuItem::new("save", "Save").accelerator("CmdOrCtrl+S").command("save"))
            .item(MenuItem::new("open", "Open...").accelerator("CmdOrCtrl+O"))
    }

    #[test]
    fn groups_and_items_chain_in_order() {
        let g = group();
        assert_eq!(g.title, "File");
        assert_eq!(g.items.len(), 2);
        assert_eq!(g.items[0].id, "save");
        assert_eq!(g.items[0].command.as_deref(), Some("save"));
        assert_eq!(g.items[1].accelerator.as_deref(), Some("CmdOrCtrl+O"));
        assert!(g.items[1].command.is_none());
    }

    #[test]
    fn accelerators_are_collected() {
        let accels = accelerators(&[group()]);
        assert_eq!(accels.len(), 2);
        assert_eq!(accels[0], ("save".to_owned(), "CmdOrCtrl+S".to_owned()));
    }

    #[test]
    fn shortcut_strings_parse() {
        assert!(parse_accelerator("CmdOrCtrl+S").is_some());
        assert!(parse_accelerator("Command+Shift+Z").is_some());
        assert!(parse_accelerator("Ctrl+Alt+Delete").is_some());
        // Garbage must not panic, just not produce an accelerator.
        assert!(parse_accelerator("nonsense+++").is_none());
    }

    #[test]
    fn the_event_payload_names_the_item() {
        assert_eq!(event_payload("save"), serde_json::json!({ "id": "save" }));
    }

    #[test]
    fn menu_windows_carry_the_given_title() {
        let options = window_options("Settings");
        assert_eq!(options.title.as_deref(), Some("Settings"));
        assert!(options.entry.is_none());
    }
}
