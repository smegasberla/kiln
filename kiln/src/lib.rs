//! Kiln: a minimal desktop app shell.
//!
//! One window by default, the OS webview, and a single generic JSON bridge:
//!
//! ```no_run
//! static ASSETS: kiln::AssetTable = &[
//!     ("index.html", include_bytes!("../examples/hello/index.html")),
//!     ("main.js", include_bytes!("../examples/hello/main.js")),
//! ];
//!
//! fn main() -> Result<(), kiln::Error> {
//!     kiln::App::new()
//!         .config(include_str!("../examples/hello/kiln.json"))
//!         .assets(ASSETS)
//!         .command("greet", |_app, payload| {
//!             let name = payload["name"].as_str().unwrap_or("world");
//!             Ok(kiln::serde_json::json!(format!("Hello, {name}!")))
//!         })
//!         .run()
//! }
//! ```
//!
//! The page calls `kiln.invoke("greet", {name: "you"})` and awaits the result.
//! Rust calls `app.emit("event", payload)` to push data the other way.
//!
//! No plugin system, no capability files, no codegen: a command exists iff it
//! was registered here, in Rust.
//!
//! # What a "real app" needs
//!
//! Each integration is a cargo feature, so a minimal app pays nothing for it:
//!
//! | Feature | Gives you |
//! | --- | --- |
//! | `menu` | native menu bar with Cut/Copy/Paste/Quit, plus app items |
//! | `tray` | system tray icon with a context menu (implies `menu`) |
//! | `dialogs` | native open / save / folder dialogs as commands |
//! | `updater` | update check against a JSON endpoint |
//! | `devtools` | devtools in release builds, bound to F12 / Cmd-Alt-I |
//! | `full` | menu + dialogs + updater + devtools |
//!
//! Always present, because leaving them out is how apps break in production:
//! handler panic isolation, navigation lockdown, CSP, crash reporting, and
//! multi-window support.

mod assets;
mod config;
mod dispatch;
mod error;
mod event_loop;
#[cfg(feature = "hot-reload")]
mod hot;
mod ipc;
mod platform;
mod reporting;
mod security;

pub mod windows;

#[cfg(feature = "dialogs")]
pub mod dialogs;
#[cfg(feature = "menu")]
pub mod menu;
#[cfg(feature = "tray")]
pub mod tray;
#[cfg(feature = "updater")]
pub mod updater;

pub use assets::{content_type, lookup, AssetTable};
pub use config::Config;
pub use error::Error;
pub use ipc::Registry;
pub use platform::{data_dir, lifecycle, menu_clicked, runtime, tray_clicked};
/// Android only: the outer crate has to hand the activity in when the OS
/// creates one, because everything that starts an intent lives on it, and to
/// hand back the answer when the picker it started returns.
#[cfg(target_os = "android")]
pub use platform::{complete_dialog, fail_dialog, init_activity};
/// Re-exported so an app crate needs exactly one dependency: `kiln`.
pub use serde_json;
pub use security::{
    content_security_policy, navigation_allowed, APP_ORIGIN, PAGE_ORIGIN, REWRITTEN_APP_ORIGIN,
};
pub use windows::WindowOptions;

use std::sync::Arc;

use serde_json::Value;
use tao::{
    event_loop::{EventLoopBuilder, EventLoopProxy},
    keyboard::{KeyCode, ModifiersState},
    window::Window,
};
#[cfg(not(target_os = "ios"))]
use tao::window::WindowBuilder;
use wry::{
    http::Request,
    WebView, WebViewBuilder,
};
use crate::dispatch::{serve_async, serve_bridge};

/// The scheme pages are served from in production.
pub const SCHEME: &str = "kiln";

/// The JS shim injected into every page: defines `window.kiln`.
const SHIM: &str = include_str!("shim.js");

/// The invoke endpoint.
///
/// Android's WebView and Windows' WebView2 have no custom-scheme interception, so
/// wry rewrites `kiln://localhost` to `http://kiln.localhost` and intercepts that
/// instead. The shim must use the rewritten form there or its `fetch` never
/// reaches the handler.
#[cfg(any(target_os = "android", target_os = "windows"))]
fn invoke_base(dev: bool) -> &'static str {
    // A released page is served from the rewritten origin, so a relative path is
    // same-origin. In dev the page comes from the dev server, where only the
    // absolute rewritten URL still reaches the interceptor (CORS allows it).
    if cfg!(target_os = "windows") && dev {
        "http://kiln.localhost/__kiln/"
    } else {
        "/__kiln/"
    }
}
/// Reached through the custom protocol in both modes, so the page's own origin
/// never matters on desktop.
#[cfg(not(any(target_os = "android", target_os = "windows")))]
fn invoke_base(_dev: bool) -> &'static str {
    "kiln://localhost/__kiln/"
}

/// Everything that can arrive on the UI thread from another thread.
pub(crate) enum UserEvent {
    Emit {
        label: Option<String>,
        event: String,
        payload: String,
    },
    OpenWindow(WindowOptions),
    CloseWindow(String),
    /// A menu item was clicked.
    Menu(String),
    /// A tray item was clicked.
    Tray(String),
    /// An out-of-band signal, e.g. "open devtools".
    Signal(String),
    /// Run a script in every window (used to resolve bridge promises).
    Eval(String),
}

/// A handle passed to command handlers. Cheap to clone, safe to send to other
/// threads, and the only way to push data into the page or open a window.
#[derive(Clone)]
pub struct AppHandle {
    proxy: Option<EventLoopProxy<UserEvent>>,
    commands: Option<Arc<Registry>>,
}

impl AppHandle {
    fn new(proxy: EventLoopProxy<UserEvent>, commands: Arc<Registry>) -> Self {
        Self {
            proxy: Some(proxy),
            commands: Some(commands),
        }
    }

    /// A handle with no event loop attached. Only useful with the shell's own
    /// unit tests; every method is a no-op or an error.
    pub fn detached_for_tests() -> Self {
        Self {
            proxy: None,
            commands: None,
        }
    }

    /// Send an event to every window, invoking `kiln.listen(event, fn)`.
    ///
    /// Thread-safe: the script runs on the UI thread on the next loop iteration.
    /// Events emitted before a page loads are dropped, so call this from a
    /// handler or `on_ready`, not from `main`.
    pub fn emit(&self, event: &str, payload: Value) {
        self.emit_to(None, event, payload);
    }

    /// Send an event to one window by label.
    pub fn emit_to(&self, label: Option<&str>, event: &str, payload: Value) {
        let Some(proxy) = &self.proxy else { return };
        let _ = proxy.send_event(UserEvent::Emit {
            label: label.map(str::to_owned),
            event: event.to_owned(),
            payload: payload.to_string(),
        });
    }

    pub(crate) fn emit_to_all(&self, event: &str, payload: Value) {
        self.emit(event, payload);
    }

    /// Open a window. Creation happens on the UI thread, so this returns
    /// immediately; the new page is announced through `on_ready`.
    pub fn open_window(&self, options: WindowOptions) -> Result<(), Error> {
        let Some(proxy) = &self.proxy else {
            return Err(Error::Runtime("no event loop attached".to_owned()));
        };
        proxy
            .send_event(UserEvent::OpenWindow(options))
            .map_err(|_| Error::Runtime("the app is shutting down".to_owned()))
    }

    /// Close a window by label. A no-op if it is already gone.
    pub fn close_window(&self, label: &str) {
        if let Some(proxy) = &self.proxy {
            let _ = proxy.send_event(UserEvent::CloseWindow(label.to_owned()));
        }
    }

    /// Run another registered command as if the page had called it.
    ///
    /// Async handlers reply on their own thread, so this never blocks. The
    /// result is discarded, which is the right shape for menu- and tray-driven
    /// actions; use `invoke_command` when you need the value.
    pub fn run_command(&self, method: &str, payload: Value) {
        if let Some(commands) = &self.commands {
            commands.dispatch_deferred(self, method, payload, |_| {});
        }
    }

    /// Run a registered command and wait for its result.
    ///
    /// Do not call this from inside another handler for the same method.
    pub fn invoke_command(&self, method: &str, payload: Value) -> Result<Value, Error> {
        let Some(commands) = &self.commands else {
            return Err(Error::Runtime("no registry attached".to_owned()));
        };
        parse_envelope(&commands.dispatch(self, method, payload))
    }

    /// Evaluate a script in every window, on the UI thread.
    ///
    /// Commands answered from a worker thread must not touch a webview directly,
    /// so the script is queued as a user event like any other emit.
    pub(crate) fn eval_all(&self, script: &str) {
        let Some(proxy) = &self.proxy else { return };
        let _ = proxy.send_event(UserEvent::Eval(script.to_owned()));
    }

    /// Open devtools on every window. Needs the `devtools` feature in release
    /// builds; always available in debug builds.
    pub fn open_devtools(&self) {
        self.emit_signal("devtools");
    }

    /// Internal bridge for crate modules that need to wake the loop.
    pub(crate) fn emit_signal(&self, signal: &str) {
        let Some(proxy) = &self.proxy else { return };
        // Menu and tray ids share this entry point but not their event name, so
        // the prefix decides which one fires; a single `signal` event would make
        // a page unable to tell a tray click from anything else on the platform.
        let event = if let Some(id) = signal.strip_prefix("menu:") {
            UserEvent::Menu(id.to_owned())
        } else if let Some(id) = signal.strip_prefix("tray:") {
            UserEvent::Tray(id.to_owned())
        } else {
            UserEvent::Signal(signal.to_owned())
        };
        let _ = proxy.send_event(event);
    }
}

/// Turn a response envelope back into a value, for `invoke_command`.
fn parse_envelope(body: &str) -> Result<Value, Error> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| Error::Runtime(format!("malformed response: {e}")))?;
    match (value.get("ok"), value.get("error")) {
        (_, Some(error)) => {
            // The structured envelope carries {message, kind}; the plain-string
            // form is still accepted so an older reply rejects with readable text.
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("command failed");
            Err(Error::Handler(message.to_owned()))
        }
        (Some(ok), None) => Ok(ok.clone()),
        _ => Err(Error::Runtime("malformed response envelope".to_owned())),
    }
}

/// Runs once after a window's page finishes loading.
type ReadyHook = Box<dyn Fn(&AppHandle) + Send + Sync>;

/// Builds a webview for a given target URL. Captures everything a window needs
/// to share: protocol handler, shim, navigation policy, ready hook.
pub(crate) type BuilderFactory =
    Arc<dyn Fn(String, Option<String>) -> WebViewBuilder<'static> + Send + Sync>;

/// The builder. Chain configuration, then `run()`.
pub struct App {
    config: Config,
    registry: Registry,
    assets: AssetTable,
    dev_url: Option<String>,
    entry: String,
    ready: Option<ReadyHook>,
    window: WindowOptions,
    #[cfg(feature = "menu")]
    menus: Vec<menu::MenuGroup>,
    #[cfg(feature = "tray")]
    tray: Option<tray::TrayConfig>,
    want_devtools: bool,
    /// The hot commands library, attached by `hot_commands_from_env`.
    #[cfg(feature = "hot-reload")]
    hot: Option<std::sync::Arc<hot::HotLib>>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            registry: Registry::new(),
            assets: &[],
            dev_url: dev_url(),
            entry: String::new(),
            ready: None,
            window: WindowOptions::default(),
            #[cfg(feature = "menu")]
            menus: Vec::new(),
            #[cfg(feature = "tray")]
            tray: None,
            want_devtools: false,
            #[cfg(feature = "hot-reload")]
            hot: None,
        }
    }

    /// Apply a `kiln.json` string, normally via `include_str!` so the config is
    /// compiled in and cannot go missing at runtime.
    ///
    /// Invalid config is a programming error; it is reported at startup rather
    /// than silently ignored, because a wrong window title is a bug worth seeing.
    pub fn config(mut self, json: &str) -> Self {
        match Config::from_json(json) {
            Ok(config) => self.config = config,
            Err(message) => {
                eprintln!("kiln: {message}");
                std::process::exit(1);
            }
        }
        self
    }

    /// The compile-time embedded frontend.
    pub fn assets(mut self, assets: AssetTable) -> Self {
        self.assets = assets;
        self
    }

    /// Register a command that runs on the UI thread.
    ///
    /// Keep these short: they block painting. For disk, network, or anything that
    /// can take longer than a frame, use `async_command`.
    pub fn command<F>(mut self, name: &str, handler: F) -> Self
    where
        F: Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.registry.add(name, handler);
        self
    }

    /// Register a command that runs on a worker thread.
    ///
    /// The page awaits the reply; the window keeps painting meanwhile, so a slow
    /// write or request no longer freezes the UI.
    pub fn async_command<F>(mut self, name: &str, handler: F) -> Self
    where
        F: Fn(AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.registry.add_async(name, handler);
        self
    }

    /// Load a specific page at startup, relative to the served root.
    pub fn entry(mut self, path: &str) -> Self {
        self.entry = path.trim_start_matches('/').to_owned();
        self
    }

    /// Override the first window's label, title, size, or entry page.
    pub fn window(mut self, options: WindowOptions) -> Self {
        self.window = options;
        self
    }

    /// Run once, right after a window's page finishes loading.
    ///
    /// This is the point where `emit` is safe: the page's listeners exist and
    /// the shim is installed.
    pub fn on_ready<F>(mut self, handler: F) -> Self
    where
        F: Fn(&AppHandle) + Send + Sync + 'static,
    {
        self.ready = Some(Box::new(handler));
        self
    }

    /// Add a native menu group. Requires the `menu` feature.
    #[cfg(feature = "menu")]
    pub fn menu(mut self, group: menu::MenuGroup) -> Self {
        self.menus.push(group);
        self
    }

    /// Add a system tray icon. Requires the `tray` feature.
    #[cfg(feature = "tray")]
    pub fn tray(mut self, config: tray::TrayConfig) -> Self {
        self.tray = Some(config);
        self
    }

    /// Write crash and error reports to `path` as well as keeping them in memory.
    ///
    /// Call this before `run`. Without it reports still exist and are readable
    /// through `__kiln_reports`, they are just not persisted.
    pub fn report_log(self, path: impl Into<std::path::PathBuf>) -> Self {
        reporting::set_log_file(path);
        self
    }

    /// Install an application sink that receives every crash and error record,
    /// in addition to the in-memory buffer and the optional log file.
    ///
    /// This is the seam for crash telemetry without forking the shell: the
    /// framework still uploads nothing by itself, an app forwards records to
    /// its own collector here. Called from the panic hook, so the sink must not
    /// panic and must not block for long.
    pub fn report_sink<F>(self, sink: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        reporting::set_reporter(sink);
        self
    }

    /// Keep devtools available in release builds, bound to F12 / Cmd-Alt-I.
    pub fn devtools(mut self, enabled: bool) -> Self {
        self.want_devtools = enabled;
        self
    }

    /// Attach the hot-reloaded commands library `kiln dev` prepares.
    ///
    /// Reads `KILN_HOT_LIB` (the initial cdylib) and `KILN_HOT_DIR` (where
    /// each rebuild publishes a new generation). This method only exists
    /// with the `hot-reload` feature, which `kiln build --release` never
    /// enables — there is nothing here for a shipped app to reach. Every
    /// failure is loud on stderr: a silently dead command table would look
    /// like a bug in the app rather than in the tooling.
    #[cfg(feature = "hot-reload")]
    pub fn hot_commands_from_env(mut self) -> Self {
        let lib = std::env::var("KILN_HOT_LIB").unwrap_or_default();
        if lib.is_empty() {
            eprintln!(
                "kiln: `hot-reload` is on but KILN_HOT_LIB is unset; run this app \
                 through `kiln dev` (commands will not resolve)"
            );
            return self;
        }
        let dir = std::env::var("KILN_HOT_DIR").ok().map(std::path::PathBuf::from);
        match hot::HotLib::open(std::path::Path::new(&lib), dir) {
            Ok(hot) => self.hot = Some(std::sync::Arc::new(hot)),
            Err(error) => eprintln!("kiln: hot commands unavailable: {error}"),
        }
        self
    }

    /// Open the window(s) and run until the last one closes.
    ///
    /// In a debug build, `KILN_DEV_URL` redirects the webview at a dev server so
    /// the frontend hot-reloads without touching Rust. Release builds ignore it.
    pub fn run(self) -> Result<(), Error> {
        reporting::install_panic_hook();

        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
        let proxy = event_loop.create_proxy();

        let mut config = self.config;
        if let Some(entry) = self.window.entry.clone() {
            config.entry = entry;
        }
        let entry = if self.entry.is_empty() {
            config.entry.clone()
        } else {
            self.entry.clone()
        };

        let mut registry = self.registry;
        register_builtins(&mut registry, &config);
        registry.set_allowlist(config.commands.clone());
        registry.set_window_allowlist(config.window_commands.clone());

        // Reloads must be running before the first window can call in.
        #[cfg(feature = "hot-reload")]
        if let Some(lib) = &self.hot {
            registry.set_hot(std::sync::Arc::clone(lib));
            hot::HotLib::watch(lib);
        }

        let registry = Arc::new(registry);
        let handle = AppHandle::new(proxy, Arc::clone(&registry));

        // Published before anything can call back, and on every platform: an
        // outer bootstrap (the Android one) has to answer with the same registry
        // the webview uses, and a platform callback can arrive before the first
        // window exists.
        platform::set_registry(&registry);
        platform::set_app_handle(&handle);

        let spec = Arc::new(windows::Spec {
            entry,
            default_title: config.title.clone(),
            default_size: (config.width, config.height),
            dev_origin: self.dev_url.clone(),
        });

        let show_devtools = self.want_devtools || cfg!(debug_assertions);
        let factory = build_factory(
            Arc::clone(&registry),
            handle.clone(),
            self.assets,
            self.dev_url.clone(),
            self.ready.map(Arc::new),
            show_devtools,
        );

        // First window.
        let first = WindowOptions {
            entry: Some(spec.entry.clone()),
            title: self.window.title.clone(),
            width: self.window.width,
            height: self.window.height,
            label: self.window.label.clone().or_else(|| Some(spec.entry.clone())),
        };
        #[cfg(not(target_os = "ios"))]
        let window = WindowBuilder::new()
            .with_title(spec.title_for(&first))
            .with_inner_size(windows::size_for(&first, spec.default_size))
            .build(&event_loop)
            .map_err(|e| Error::Runtime(e.to_string()))?;
        #[cfg(not(target_os = "ios"))]
        let webview = build_webview(
            factory(
                spec.url_for(&spec.entry_for(&first)),
                Some(spec.label_for(&first)),
            ),
            &window,
        )?;

        // tao builds a window against `UIApplication`, which does not exist
        // until `event_loop.run` calls `UIApplicationMain`; iOS asks for the
        // first window through the event a `window.open` command uses and
        // gets it once the loop is running.
        #[cfg(target_os = "ios")]
        let _ = handle
            .open_window(first)
            .map_err(|e| Error::Runtime(e.to_string()))?;

        #[cfg(feature = "menu")]
        let menu_commands = {
            let built = build_menu(&self.menus, &handle, &config.name);
            #[cfg(not(target_os = "ios"))]
            let attached = built.as_ref().map(|built| built.attach(&window));
            #[cfg(not(target_os = "ios"))]
            if let Some(Err(error)) = attached {
                eprintln!("kiln: menu could not be attached: {error}");
            }
            #[cfg(target_os = "ios")]
            let _ = &built;
            menu_command_map(&self.menus)
        };

        // Underscored because nothing reads it: the tray has to stay alive for the
        // whole run, and dropping it removes the icon.
        #[cfg(feature = "tray")]
        let _tray_handle = build_tray(&self.tray, &handle);

        let mut state = event_loop::LoopState {
            open: {
                #[cfg(not(target_os = "ios"))]
                let open = {
                    let mut open = windows::Windows::new();
                    windows::insert(
                        &mut open,
                        window.id(),
                        windows::WindowEntry {
                            label: spec.label_for(&first),
                            window,
                            webview,
                        },
                    );
                    open
                };
                #[cfg(target_os = "ios")]
                let open = windows::Windows::new();
                open
            },
            modifiers: ModifiersState::empty(),
            spec,
            factory,
            handle,
            show_devtools,
            #[cfg(feature = "menu")]
            menu_commands,
        };

        event_loop.run(move |event, event_loop, control_flow| {
            event_loop::dispatch(&mut state, event, event_loop, control_flow);
        });
    }
}

/// Assemble the closure that builds a webview for any target URL.
fn build_factory(
    registry: Arc<Registry>,
    handle: AppHandle,
    assets: AssetTable,
    dev_origin: Option<String>,
    ready: Option<Arc<ReadyHook>>,
    devtools: bool,
) -> BuilderFactory {
    let shim = build_shim(dev_origin.as_deref());
    Arc::new(move |target: String, label: Option<String>| {
        let nav_origin = dev_origin.clone();
        let protocol_registry = Arc::clone(&registry);
        let protocol_handle = handle.clone();
        let protocol_origin = dev_origin.clone();
        let bridge_label = label.clone();

        let builder = WebViewBuilder::new()
            // Debug builds always have devtools; a release build gets them only
            // with the `devtools` feature (or `full`).
            .with_devtools(devtools && cfg!(any(debug_assertions, feature = "devtools")))
            .with_initialization_script(&shim)
            .with_navigation_handler(move |url| {
                security::navigation_allowed(&url, nav_origin.as_deref())
            })
            .with_asynchronous_custom_protocol(
                SCHEME.into(),
                move |_id, request: Request<Vec<u8>>, responder| {
                    serve_async(
                        Arc::clone(&protocol_registry),
                        protocol_handle.clone(),
                        assets,
                        protocol_origin.clone(),
                        label.clone(),
                        request,
                        responder,
                    )
                },
            )
            .with_ipc_handler({
                // Only Android sends bridge messages; on desktop this handler is
                // present but the shim never uses it.
                let registry = Arc::clone(&registry);
                let handle = handle.clone();
                move |request: Request<String>| {
                    serve_bridge(&registry, &handle, bridge_label.as_deref(), request.body().as_str());
                }
            })
            .with_url(target);

        match &ready {
            Some(ready) => {
                let ready = Arc::clone(ready);
                let handle = handle.clone();
                builder.with_on_page_load_handler(move |event, _url| {
                    if matches!(event, wry::PageLoadEvent::Finished) {
                        platform::page_loaded();
                        ready(&handle);
                    }
                })
            }
            // Without a ready hook the handler still has work: a lifecycle state
            // reported before the page could run a script is queued, and this is
            // the moment it can be delivered.
            None => builder.with_on_page_load_handler(|event, _url| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    platform::page_loaded();
                }
            }),
        }
    })
}

#[cfg(feature = "menu")]
fn menu_command_map(groups: &[menu::MenuGroup]) -> std::collections::HashMap<String, String> {
    groups
        .iter()
        .flat_map(|g| g.items.iter())
        .filter_map(|item| item.command.clone().map(|cmd| (item.id.clone(), cmd)))
        .collect()
}

#[cfg(feature = "menu")]
fn build_menu(
    groups: &[menu::MenuGroup],
    handle: &AppHandle,
    app_name: &str,
) -> Option<menu::MenuHandle> {
    let built = match menu::build(app_name, groups) {
        Ok(menu) => menu,
        Err(error) => {
            eprintln!("kiln: menu could not be built: {error}");
            return None;
        }
    };

    // Desktop menus report clicks through muda's global handler. Android has no
    // menu bar: its menu is a Java `PopupMenu` that calls back through
    // `platform::menu_clicked`, so it only needs the groups in a form the
    // activity can render, and emits through `platform::handle` when a click
    // arrives.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let proxy = handle.clone();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            proxy.emit_signal(&format!("menu:{}", event.id.0));
        }));
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = handle;
        // There is no menu bar to hang the items on, so Android hands them over
        // once and shows them later by `__kiln_show_menu`, when there is an
        // anchor view. iOS has no menu backend at all yet: the handle is inert.
        #[cfg(target_os = "android")]
        {
            let json = menu::to_json(&built.groups()).to_string();
            if !crate::platform::install_menu(&json) {
                eprintln!("kiln: menu installed before the activity existed; it will not show");
            }
        }
    }

    Some(built)
}

#[cfg(feature = "tray")]
fn build_tray(config: &Option<tray::TrayConfig>, handle: &AppHandle) -> Option<tray::TrayHandle> {
    let config = config.as_ref()?;
    match tray::build(config, handle) {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("kiln: tray could not be created: {error}");
            None
        }
    }
}

/// Open devtools if this build has them.
///
/// `wry` only exposes `open_devtools` when devtools are compiled in, which is
/// automatic in debug builds and requires the `devtools` feature in release. The
/// shell promises the shortcut, so it must not fail to compile without it; a
/// release build that opted out simply gets no devtools.
pub(crate) fn open_devtools(webview: &WebView) {
    #[cfg(any(debug_assertions, feature = "devtools"))]
    webview.open_devtools();
    #[cfg(not(any(debug_assertions, feature = "devtools")))]
    {
        let _ = webview;
        eprintln!("kiln: devtools are not compiled into this build");
    }
}

/// F12 on Linux and Windows, Cmd-Alt-I on macOS.
pub(crate) fn is_devtools_shortcut(key: KeyCode, modifiers: ModifiersState) -> bool {
    #[cfg(target_os = "macos")]
    {
        key == KeyCode::KeyI && modifiers.super_key() && modifiers.alt_key()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = modifiers;
        key == KeyCode::F12
    }
}

/// Attach a webview to a window.
///
/// Three shapes, because the platforms genuinely differ:
///
/// - **Windows, macOS, Android, iOS** — the window is a handle the webview can
///   attach to directly, so `build` is enough.
/// - **GTK platforms** (Linux and the BSDs) — the webview is a GTK widget that
///   has to be added to the window's container explicitly.
fn build_webview(builder: WebViewBuilder<'_>, window: &Window) -> Result<WebView, Error> {
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "ios"
    ))]
    {
        Ok(builder.build(window)?)
    }
    #[cfg(kiln_gtk_webview)]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let container = window
            .default_vbox()
            .ok_or_else(|| Error::Runtime("window has no gtk container".to_owned()))?;
        Ok(builder.build_gtk(container)?)
    }
}

/// Build the JS shim with the origin allowlist baked in.
///
/// The allowlist is computed in Rust and injected as JSON, so the page side never
/// guesses which origins are legitimate.
fn build_shim(dev_origin: Option<&str>) -> String {
    // Android's WebResourceRequest carries no body, so a protocol POST cannot
    // deliver a payload there; wry's IPC handler is the only option.
    let bridge = cfg!(target_os = "android");
    let mut origins = vec![APP_ORIGIN.to_owned()];
    if let Some(dev) = dev_origin {
        origins.push(dev.to_owned());
    }
    // Android's WebView and Windows' WebView2 rewrite the page origin to
    // http://kiln.localhost, which wry intercepts. On Windows the rewrite is a
    // navigation to that URL, so it is the origin the page actually reports.
    #[cfg(any(target_os = "android", target_os = "windows"))]
    origins.push(REWRITTEN_APP_ORIGIN.to_owned());
    let allowed = serde_json::to_string(&origins).unwrap_or_else(|_| "[]".to_owned());
    SHIM.replace("__INVOKE_BASE__", invoke_base(dev_origin.is_some()))
        .replace("__ALLOWED_ORIGINS__", &allowed)
        .replace("__BRIDGE__", if bridge { "true" } else { "false" })
        .replace("__PLATFORM__", platform_name())
}

/// The parts of the shell that do not depend on a desktop window loop.
///
/// A mobile build replaces the *transport* (wry's IPC handler on Android,
/// `WKScriptMessageHandler` on iOS) and the *entry point* (JNI instead of
/// `fn main`), but the envelope, the registry, the allowlist, the CSP and the
/// navigation policy are identical. Keeping them in one place is what makes that
/// possible without forking behaviour; `transport` is the seam.
pub mod transport {
    use super::*;

    /// Which transport a page's calls arrive through.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        /// wry custom protocol: `POST kiln://localhost/__kiln/<method>`.
        Protocol,
        /// A native message handler that receives `{method, payload}` as a
        /// string and answers with an envelope.
        MessageBridge,
    }

    /// Build the response envelope for one request on any transport.
    ///
    /// This is the single place that decides what a page is told, so desktop and
    /// mobile cannot drift apart.
    pub fn respond(
        registry: &Registry,
        handle: &AppHandle,
        method: &str,
        payload: Value,
    ) -> String {
        registry.dispatch(handle, method, payload)
    }

    /// Parse a payload body, treating an empty body as JSON `null`.
    pub fn parse_payload(body: &[u8]) -> Result<Value, String> {
        if body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(body).map_err(|_| "payload is not valid JSON".to_owned())
    }

    /// The method name and payload a mobile bridge receives in one string,
    /// mirroring the protocol's path-plus-body shape.
    pub fn parse_message(message: &str) -> Result<(String, Value), String> {
        let value: Value = serde_json::from_str(message)
            .map_err(|_| "message is not valid JSON".to_owned())?;
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| "message has no method".to_owned())?
            .to_owned();
        let payload = value.get("payload").cloned().unwrap_or(Value::Null);
        Ok((method, payload))
    }
}


/// The platform name the page receives as `kiln.platform`.
///
/// Shared frontend code uses it to hide affordances that cannot work: a second
/// window on Android, a file dialog on a platform without one, and so on.
pub fn platform_name() -> &'static str {
    #[cfg(target_os = "android")]
    {
        "android"
    }
    #[cfg(target_os = "ios")]
    {
        "ios"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    {
        "bsd"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
}

fn dev_url() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("KILN_DEV_URL").ok().filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// Register the commands the shell provides itself.
fn register_builtins(registry: &mut Registry, config: &Config) {
    registry.add("__kiln_version", {
        let name = config.name.clone();
        move |_app, _payload| {
            Ok(serde_json::json!({
                "app": name,
                "kiln": env!("CARGO_PKG_VERSION"),
            }))
        }
    });

    registry.add("__kiln_reports", |_app, _payload| {
        Ok(serde_json::json!(reporting::entries()))
    });

    // A page that writes user data needs somewhere to put it, and the answer
    // differs per OS. Answered here rather than in the shim so the page never
    // has to guess a platform. `null` means it could not be determined, which
    // the page should treat as "no persistence available".
    registry.add("__kiln_data_dir", {
        let name = config.name.clone();
        move |_app, _payload| {
            let path = platform::data_dir(&name)
                .and_then(|dir| dir.to_str().map(str::to_owned));
            Ok(serde_json::json!({ "path": path }))
        }
    });

    registry.add("__kiln_open_window", |app, payload| {
        app.open_window(WindowOptions::from_json(&payload))?;
        Ok(Value::Null)
    });

    registry.add("__kiln_close_window", |app, payload| {
        if let Some(label) = payload.get("label").and_then(Value::as_str) {
            app.close_window(label);
        }
        Ok(Value::Null)
    });

    registry.add("__kiln_devtools", |app, _payload| {
        app.open_devtools();
        Ok(Value::Null)
    });

    // Android has no menu bar, so the menu is a `PopupMenu` raised on demand.
    // The page owns the button that raises it, because Android expects app chrome
    // (an overflow button or drawer) rather than OS chrome.
    #[cfg(all(feature = "menu", target_os = "android"))]
    registry.add("__kiln_show_menu", |_app, _payload| {
        if platform::show_menu() {
            Ok(Value::Null)
        } else {
            Err(Error::Handler("no menu is installed to show".to_owned()))
        }
    });

    #[cfg(feature = "dialogs")]
    dialogs::register(registry);

    #[cfg(feature = "updater")]
    updater::register(registry, config);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const INDEX: &[u8] = b"<!doctype html>hi";
    static ASSETS: AssetTable = &[("index.html", INDEX)];

    #[test]
    fn shim_carries_the_app_origin_and_any_dev_origin() {
        let shim = build_shim(None);
        assert!(shim.contains(APP_ORIGIN), "app origin must be allowed");
        assert!(!shim.contains("__ALLOWED_ORIGINS__"), "placeholder must be filled");
        assert!(!shim.contains("__INVOKE_BASE__"));

        let shim = build_shim(Some("http://127.0.0.1:5177"));
        assert!(shim.contains("http://127.0.0.1:5177"));
        assert!(shim.contains(APP_ORIGIN));
    }

    #[test]
    fn shim_names_the_origin_the_page_is_actually_served_from() {
        // On Windows and Android wry serves the page from the rewritten http
        // origin; a shim built for the custom scheme there would allowlist an
        // origin the page never reports and post to an endpoint that is not
        // intercepted, which is exactly how the WebView2 harness failed.
        let shim = build_shim(None);
        assert!(shim.contains(PAGE_ORIGIN), "page origin must be allowlisted");
        assert!(shim.contains(&format!("\"{}\"", invoke_base(false))));
        let rewritten = cfg!(any(target_os = "android", target_os = "windows"));
        assert_eq!(shim.contains("http://kiln.localhost"), rewritten);

        // In dev the page comes from the dev server, so a relative base would be
        // resolved against that server; on Windows the absolute rewritten URL is
        // the only spelling that still reaches the interceptor.
        if cfg!(target_os = "windows") {
            assert_eq!(invoke_base(true), "http://kiln.localhost/__kiln/");
            assert!(build_shim(Some("http://127.0.0.1:5177")).contains(invoke_base(true)));
        }
    }

    #[test]
    fn envelopes_round_trip_through_invoke_command_parsing() {
        assert_eq!(parse_envelope(r#"{"ok":{"a":1}}"#).unwrap(), json!({ "a": 1 }));
        assert!(parse_envelope(r#"{"error":"nope"}"#).is_err());
        assert!(parse_envelope("not json").is_err());
    }

    #[test]
    fn assets_are_looked_up_by_full_extension_not_prefix() {
        assert_eq!(lookup(ASSETS, "/").unwrap().0, "index.html");
        assert!(lookup(ASSETS, "/nope.css").is_none());
        assert_eq!(content_type("a.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("a.css"), "text/css; charset=utf-8");
    }

    #[test]
    fn devtools_shortcut_is_platform_appropriate() {
        #[cfg(not(target_os = "macos"))]
        {
            assert!(is_devtools_shortcut(KeyCode::F12, ModifiersState::empty()));
            assert!(!is_devtools_shortcut(KeyCode::F11, ModifiersState::empty()));
        }
        #[cfg(target_os = "macos")]
        {
            let mods = ModifiersState::SUPER | ModifiersState::ALT;
            assert!(is_devtools_shortcut(KeyCode::KeyI, mods));
            assert!(!is_devtools_shortcut(KeyCode::KeyI, ModifiersState::SUPER));
        }
    }

    #[test]
    fn both_transports_share_the_envelope() {
        // The mobile bridge and the desktop protocol must not be able to answer
        // differently for the same command.
        let mut registry = Registry::new();
        registry.add("echo", |_app, payload| Ok(payload));

        let via_protocol = transport::respond(
            &registry,
            &AppHandle::detached_for_tests(),
            "echo",
            transport::parse_payload(br#"{"a":1}"#).unwrap(),
        );

        let (method, payload) =
            transport::parse_message(r#"{"method":"echo","payload":{"a":1}}"#).unwrap();
        let via_bridge =
            transport::respond(&registry, &AppHandle::detached_for_tests(), &method, payload);

        assert_eq!(via_protocol, via_bridge);
        assert_eq!(via_protocol, r#"{"ok":{"a":1}}"#);
    }

    #[test]
    fn an_empty_body_is_null_and_junk_is_rejected() {
        assert_eq!(transport::parse_payload(b"").unwrap(), Value::Null);
        assert!(transport::parse_payload(b"not json").is_err());
        assert_eq!(
            transport::parse_payload(br#"{"x":1}"#).unwrap(),
            json!({ "x": 1 })
        );
    }

    #[test]
    fn bridge_messages_need_a_method() {
        assert!(transport::parse_message(r#"{"payload":{}}"#).is_err());
        assert!(transport::parse_message("nope").is_err());
        let (method, payload) = transport::parse_message(r#"{"method":"m"}"#).unwrap();
        assert_eq!(method, "m");
        assert_eq!(payload, Value::Null);
    }

    #[test]
    fn builtin_commands_are_registered() {
        let mut registry = Registry::new();
        register_builtins(&mut registry, &Config::default());
        for name in [
            "__kiln_version",
            "__kiln_reports",
            "__kiln_open_window",
            "__kiln_close_window",
            "__kiln_devtools",
        ] {
            assert!(registry.has(name), "{name} should exist");
        }
    }

    #[test]
    fn builtins_respect_an_explicit_allowlist() {
        let mut registry = Registry::new();
        let config = Config {
            commands: Some(vec!["__kiln_version".to_owned()]),
            ..Config::default()
        };
        register_builtins(&mut registry, &config);
        registry.set_allowlist(config.commands.clone());

        assert!(registry.has("__kiln_version"));
        assert!(!registry.has("__kiln_reports"));
    }

    #[test]
    fn window_commands_are_reachable_from_the_page() {
        let handle = AppHandle::detached_for_tests();
        // Detached: opening a window reports the missing loop instead of lying.
        assert!(handle
            .open_window(WindowOptions::default())
            .is_err());
        // Closing something that cannot exist is a silent no-op.
        handle.close_window("nope");
    }

    #[test]
    fn empty_responses_are_never_served_as_success() {
        // A 404 must not carry an empty body that the page could parse as JSON.
        assert!(parse_envelope(r#"{"ok":null}"#).is_ok());
        assert!(parse_envelope("").is_err());
    }
}
