//! Tray and menu state that can change after the app starts.
//!
//! A tray icon and a menu bar are built once, on the UI thread, and the native
//! objects behind them are neither `Send` nor `Sync`. An app still needs to
//! retitle a tray, check a menu item, or raise a menu from a command handler,
//! which runs on any thread.
//!
//! So the decision and the action are separated. [`Declared`] records what the
//! app declared, which is enough to answer "does this tray item exist", "is it a
//! check item", "does this platform even have a tray" — all of it off the UI
//! thread, synchronously, so a caller gets a real error instead of a log line.
//! The resulting [`Change`] is then queued to the event loop, which owns the
//! native objects and applies it.
//!
//! Both halves are testable without a display: `Declared::request` is pure, and
//! `apply` is the only function that touches the platform.

// Without tray or menu compiled in, an app has no chrome to update, so the
// request/reply types below exist but are never reachable. The declarations
// themselves stay, because a detached handle is built with them.
#![cfg_attr(not(any(feature = "tray", feature = "menu")), allow(dead_code))]

use crate::error::Error;

/// Whether a tray icon can be changed after it is built.
///
/// Desktop trays are real objects with a tooltip and check items. Android turns
/// the same declaration into a notification that is posted once and cannot be
/// edited; iOS has no tray at all. Reporting that instead of accepting a call
/// that silently does nothing is the difference between a documented limit and a
/// bug report.
pub const TRAY_IS_LIVE: bool = cfg!(not(any(target_os = "android", target_os = "ios")));

/// One change for the UI thread to apply to a native object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    #[cfg(feature = "tray")]
    TrayTooltip(String),
    #[cfg(feature = "tray")]
    TrayChecked { id: String, checked: bool },
    #[cfg(feature = "tray")]
    TrayPopup,
    #[cfg(feature = "menu")]
    MenuChecked { id: String, checked: bool },
}

/// What a caller wants changed, before it has been validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wanted {
    #[cfg(feature = "tray")]
    TrayTooltip(String),
    #[cfg(feature = "tray")]
    TrayChecked { id: String, checked: bool },
    #[cfg(feature = "tray")]
    TrayPopup,
    #[cfg(feature = "menu")]
    MenuChecked { id: String, checked: bool },
}

/// The app's tray and menu declarations, kept so a request can be answered
/// without a native object.
#[derive(Debug, Default)]
pub struct Declared {
    /// `None` when the app declared no tray.
    #[cfg(feature = "tray")]
    tray: Option<TrayDecl>,
    /// Ids of the app's check items, which are the only togglable ones.
    #[cfg(feature = "menu")]
    menu_checks: Vec<String>,
}

#[cfg(feature = "tray")]
#[derive(Debug)]
struct TrayDecl {
    /// Ids of the declared check items.
    checks: Vec<String>,
}

impl Declared {
    /// Record what the app asked for. Called once, before the loop starts.
    #[cfg(any(feature = "tray", feature = "menu"))]
    pub fn new(
        #[cfg(feature = "tray")] tray: Option<&crate::tray::TrayConfig>,
        #[cfg(feature = "menu")] menus: &[crate::menu::MenuGroup],
    ) -> Self {
        Self {
            #[cfg(feature = "tray")]
            tray: tray.map(|config| TrayDecl {
                checks: config
                    .items
                    .iter()
                    .filter(|item| item.checked.is_some())
                    .map(|item| item.id.clone())
                    .collect(),
            }),
            #[cfg(feature = "menu")]
            menu_checks: menus
                .iter()
                .flat_map(|group| group.items.iter())
                .filter(|item| item.checked.is_some())
                .map(|item| item.id.clone())
                .collect(),
        }
    }

    /// A declaration with no tray and no menus, for a detached handle.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Validate a request and turn it into a change the loop can apply.
    ///
    /// Every rejection here is a state the native call would have failed on
    /// anyway, moved to the caller's thread where it can still be handled.
    pub(crate) fn request(&self, wanted: Wanted) -> Result<Change, Error> {
        match wanted {
            #[cfg(feature = "menu")]
            Wanted::MenuChecked { id, checked } => {
                if !self.menu_checks.contains(&id) {
                    return Err(Error::Handler(format!(
                        "no menu item is named {id:?}, or it is not a check item; declare it with .check() or .checked(..)"
                    )));
                }
                Ok(Change::MenuChecked { id, checked })
            }
            #[cfg(feature = "tray")]
            Wanted::TrayTooltip(text) => {
                self.tray_ref()?;
                Ok(Change::TrayTooltip(text))
            }
            #[cfg(feature = "tray")]
            Wanted::TrayPopup => {
                self.tray_ref()?;
                Ok(Change::TrayPopup)
            }
            #[cfg(feature = "tray")]
            Wanted::TrayChecked { id, checked } => {
                let tray = self.tray_ref()?;
                if !tray.checks.contains(&id) {
                    return Err(Error::Handler(format!(
                        "no tray item is named {id:?}, or it is not a check item; declare it with .check() or .checked(..)"
                    )));
                }
                Ok(Change::TrayChecked { id, checked })
            }
        }
    }

    /// The declared tray, or the reason there is none to act on.
    #[cfg(feature = "tray")]
    fn tray_ref(&self) -> Result<&TrayDecl, Error> {
        let tray = self.tray.as_ref().ok_or_else(|| {
            Error::Handler("this app declared no tray, so there is nothing to update".to_owned())
        })?;
        if !TRAY_IS_LIVE {
            return Err(Error::Handler(
                "the tray cannot be changed at runtime on this platform: it is a posted notification, not a live icon"
                    .to_owned(),
            ));
        }
        Ok(tray)
    }
}

/// Apply one change to the live objects. UI thread only.
#[cfg(feature = "tray")]
pub(crate) fn apply_tray(tray: Option<&crate::tray::TrayHandle>, change: &Change) {
    let Some(tray) = tray else {
        if matches!(
            change,
            Change::TrayTooltip(_) | Change::TrayChecked { .. } | Change::TrayPopup
        ) {
            eprintln!("kiln: a tray update arrived but no tray is live");
        }
        return;
    };
    let outcome = match change {
        Change::TrayTooltip(text) => tray.set_tooltip(text),
        Change::TrayChecked { id, checked } => tray.set_checked(id, *checked),
        Change::TrayPopup => {
            tray.popup();
            return;
        }
        // A menu-only change is not this function's business.
        #[allow(unreachable_patterns)]
        _ => return,
    };
    if let Err(error) = outcome {
        eprintln!("kiln: tray update failed: {error}");
    }
}

/// Apply one change to the live menu. UI thread only.
#[cfg(feature = "menu")]
pub(crate) fn apply_menu(menu: Option<&crate::menu::MenuHandle>, change: &Change) {
    let Some(menu) = menu else {
        if matches!(change, Change::MenuChecked { .. }) {
            eprintln!("kiln: a menu update arrived but no menu is live");
        }
        return;
    };
    let outcome = match change {
        Change::MenuChecked { id, checked } => menu.set_checked(id, *checked),
        // A tray-only change is not this function's business.
        #[allow(unreachable_patterns)]
        _ => return,
    };
    if let Err(error) = outcome {
        eprintln!("kiln: menu update failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "tray")]
    fn tray_config() -> crate::tray::TrayConfig {
        crate::tray::TrayConfig::new("Notes")
            .item(crate::tray::TrayItem::new("auto", "Autosave").check())
            .item(crate::tray::TrayItem::new("quit", "Quit"))
    }

    #[cfg(all(feature = "menu", feature = "tray"))]
    fn menus() -> Vec<crate::menu::MenuGroup> {
        vec![crate::menu::MenuGroup::new("View")
            .item(crate::menu::MenuItem::new("wrap", "Wrap Lines").check())
            .item(crate::menu::MenuItem::new("save", "Save"))]
    }

    #[cfg(all(feature = "tray", feature = "menu"))]
    fn declared() -> Declared {
        Declared::new(Some(&tray_config()), &menus())
    }

    #[cfg(all(feature = "tray", feature = "menu"))]
    #[test]
    fn a_declared_check_item_accepts_a_toggle() {
        let declared = declared();
        // `Error` carries a source string and no equality, so compare the
        // change itself rather than the whole `Result`.
        assert_eq!(
            declared
                .request(Wanted::MenuChecked {
                    id: "wrap".to_owned(),
                    checked: true,
                })
                .ok(),
            Some(Change::MenuChecked {
                id: "wrap".to_owned(),
                checked: true,
            })
        );
    }

    #[cfg(all(feature = "tray", feature = "menu"))]
    #[test]
    fn a_plain_item_is_rejected_with_the_reason() {
        let declared = declared();
        let error = declared
            .request(Wanted::MenuChecked {
                id: "save".to_owned(),
                checked: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("not a check item"), "{error}");

        let error = declared
            .request(Wanted::MenuChecked {
                id: "missing".to_owned(),
                checked: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("no menu item"), "{error}");
    }

    #[cfg(all(feature = "tray", feature = "menu"))]
    #[test]
    fn an_app_without_a_tray_is_told_so() {
        let declared = Declared::new(None, &[]);
        let error = declared
            .request(Wanted::TrayTooltip("hello".to_owned()))
            .unwrap_err();
        assert!(error.to_string().contains("no tray"), "{error}");
    }

    #[cfg(all(feature = "tray", feature = "menu"))]
    #[test]
    fn a_tray_request_answers_whether_the_platform_allows_it() {
        let declared = declared();
        let outcome = declared.request(Wanted::TrayTooltip("Notes (2)".to_owned()));
        if TRAY_IS_LIVE {
            assert_eq!(
                outcome.unwrap(),
                Change::TrayTooltip("Notes (2)".to_owned())
            );
        } else {
            let error = outcome.unwrap_err();
            assert!(
                error.to_string().contains("cannot be changed at runtime"),
                "{error}"
            );
        }
    }

    #[cfg(all(feature = "tray", feature = "menu"))]
    #[test]
    fn a_tray_check_toggle_names_the_item_it_needs() {
        let declared = declared();
        let plain = declared.request(Wanted::TrayChecked {
            id: "quit".to_owned(),
            checked: true,
        });
        if TRAY_IS_LIVE {
            assert!(plain.unwrap_err().to_string().contains("not a check item"));
            assert_eq!(
                declared
                    .request(Wanted::TrayChecked {
                        id: "auto".to_owned(),
                        checked: false,
                    })
                    .ok(),
                Some(Change::TrayChecked {
                    id: "auto".to_owned(),
                    checked: false,
                })
            );
        } else {
            assert!(plain.is_err());
        }
    }

    #[test]
    fn an_empty_declaration_serves_a_detached_handle() {
        let declared = Declared::empty();
        // Whatever the feature set, a handle with nothing declared never panics
        // and never claims success for a tray it does not have.
        #[cfg(feature = "tray")]
        assert!(declared.request(Wanted::TrayPopup).is_err());
        #[cfg(feature = "menu")]
        assert!(declared
            .request(Wanted::MenuChecked {
                id: "wrap".to_owned(),
                checked: true,
            })
            .is_err());
        let _ = &declared;
    }
}
