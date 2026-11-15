//! System tray icon with a context menu.
//!
//! Minimal by design: an icon, a tooltip, and menu items that route through the
//! same command/event path as everything else. Clicking an item emits a `tray`
//! event with `{"id": ...}` and optionally runs the command it names, so a tray
//! menu needs no separate plumbing in the app.
//!
//! On Linux this needs `libappindicator` (or ayatana-appindicator) at build time;
//! the icon lives in the desktop's status area and its menu is owned by the
//! environment.

use serde_json::json;

use crate::{AppHandle, Error};

/// One tray menu item.
#[derive(Debug, Clone)]
pub struct TrayItem {
    /// Identifier sent to the page.
    pub id: String,
    pub label: String,
    /// Optional command run when the item is clicked.
    pub command: Option<String>,
}

impl TrayItem {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            command: None,
        }
    }

    pub fn command(mut self, command: &str) -> Self {
        self.command = Some(command.to_owned());
        self
    }
}

/// Everything needed to create a tray icon.
#[derive(Debug, Clone)]
pub struct TrayConfig {
    pub tooltip: String,
    /// RGBA pixels and their dimensions. A 32x32 or 22x22 icon is typical.
    pub icon: Option<(Vec<u8>, u32, u32)>,
    pub items: Vec<TrayItem>,
    /// Quit the app when the last window closes. Defaults to true; a tray app
    /// often wants false so it can keep running in the background.
    pub quit_on_last_window: bool,
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            tooltip: "Kiln".to_owned(),
            icon: None,
            items: Vec::new(),
            quit_on_last_window: true,
        }
    }
}

impl TrayConfig {
    pub fn new(tooltip: &str) -> Self {
        Self {
            tooltip: tooltip.to_owned(),
            ..Self::default()
        }
    }

    pub fn icon(mut self, rgba: Vec<u8>, width: u32, height: u32) -> Self {
        self.icon = Some((rgba, width, height));
        self
    }

    pub fn item(mut self, item: TrayItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn keep_running(mut self) -> Self {
        self.quit_on_last_window = false;
        self
    }
}

/// A live tray icon. Dropping it removes the icon from the status area.
pub struct TrayHandle {
    _icon: tray_icon::TrayIcon,
}

/// Build the tray icon and wire its menu to the app's event loop.
pub fn build(config: &TrayConfig, handle: &AppHandle) -> Result<TrayHandle, Error> {
    use tray_icon::menu::{Menu, MenuItem as TrayMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    let menu = Menu::new();
    for item in &config.items {
        let menu_item = TrayMenuItem::with_id(&item.id, &item.label, true, None);
        menu.append(&menu_item).map_err(|e| Error::Runtime(e.to_string()))?;
    }

    let mut builder = TrayIconBuilder::new()
        .with_tooltip(&config.tooltip)
        .with_menu(Box::new(menu));

    if let Some((rgba, width, height)) = &config.icon {
        let icon = Icon::from_rgba(rgba.clone(), *width, *height)
            .map_err(|e| Error::Runtime(format!("invalid tray icon: {e}")))?;
        builder = builder.with_icon(icon);
    }

    let icon = builder
        .build()
        .map_err(|e| Error::Runtime(format!("tray icon failed: {e}")))?;

    let proxy = handle.clone();
    tray_icon::menu::MenuEvent::set_event_handler(Some(move |event: tray_icon::menu::MenuEvent| {
        proxy.emit_signal(&format!("tray:{}", event.id.0));
    }));

    Ok(TrayHandle { _icon: icon })
}

/// The event payload the page receives for a tray click.
pub fn event_payload(id: &str) -> serde_json::Value {
    json!({ "id": id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_builder_is_chainable_and_keeps_order() {
        let config = TrayConfig::new("Notes")
            .icon(vec![0; 4 * 16 * 16], 16, 16)
            .item(TrayItem::new("open", "Open").command("show"))
            .item(TrayItem::new("quit", "Quit"))
            .keep_running();

        assert_eq!(config.tooltip, "Notes");
        assert!(!config.quit_on_last_window);
        assert_eq!(config.items.len(), 2);
        assert_eq!(config.items[0].id, "open");
        assert_eq!(config.items[0].command.as_deref(), Some("show"));
        assert!(config.items[1].command.is_none());
        let (rgba, w, h) = config.icon.unwrap();
        assert_eq!((rgba.len(), w, h), (4 * 16 * 16, 16, 16));
    }

    #[test]
    fn defaults_quit_with_the_last_window() {
        let config = TrayConfig::default();
        assert!(config.quit_on_last_window);
        assert!(config.items.is_empty());
        assert!(config.icon.is_none());
    }

    #[test]
    fn the_event_payload_names_the_item() {
        assert_eq!(event_payload("open"), json!({ "id": "open" }));
    }
}
