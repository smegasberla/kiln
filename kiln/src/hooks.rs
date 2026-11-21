//! The lifecycle hooks an app can install, and the types the loop carries for
//! them.
//!
//! `Fired` is the piece worth reading: both hooks are documented as once per
//! window, and both can be reached twice in practice (a reload, or tao delivering
//! `CloseRequested` for a window already on its way out), so "exactly once" is
//! enforced here rather than hoped for.

use std::sync::Arc;

use wry::WebViewBuilder;

use crate::AppHandle;

/// Which windows have already fired a lifecycle hook.
///
/// Both hooks are documented as once per window, and both can be reached twice
/// in practice: a page can finish loading after a reload, and tao can deliver
/// `CloseRequested` for a window that is already on its way out. Keeping the
/// set here makes "exactly once" a property of the code rather than of the
/// platform's event habits.
#[derive(Default)]
pub(crate) struct Fired {
    ready: std::sync::Mutex<std::collections::HashSet<String>>,
    closed: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl Fired {
    /// `true` the first time a label is seen for `ready`, `false` after.
    pub(crate) fn first_ready(&self, label: &str) -> bool {
        self.first(&self.ready, label)
    }

    /// `true` the first time a label is seen for `window close`, `false` after.
    pub(crate) fn first_close(&self, label: &str) -> bool {
        self.first(&self.closed, label)
    }

    fn first(
        &self,
        set: &std::sync::Mutex<std::collections::HashSet<String>>,
        label: &str,
    ) -> bool {
        let mut set = set.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        set.insert(label.to_owned())
    }
}

/// Runs once after a window's page finishes loading.
pub(crate) type ReadyHook = Box<dyn Fn(&AppHandle, &str) + Send + Sync>;
/// Runs once when a window is closing, before its webview is dropped. The queue
/// of pending window state is dropped with the window, so this is the last
/// point an app can persist anything it kept per window.
pub(crate) type CloseHook = Box<dyn Fn(&AppHandle, &str) + Send + Sync>;

/// Builds a webview for a given target URL. Captures everything a window needs
/// to share: protocol handler, shim, navigation policy, ready hook.
pub(crate) type BuilderFactory =
    Arc<dyn Fn(String, Option<String>) -> WebViewBuilder<'static> + Send + Sync>;

