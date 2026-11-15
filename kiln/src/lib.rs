//! Kiln: a minimal desktop app shell.
//!
//! One window, the OS webview, and a single generic JSON bridge:
//!
//! ```no_run
//! // The frontend, embedded at compile time. No filesystem access at runtime.
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

mod assets;
mod config;
mod error;
mod ipc;

pub use assets::{content_type, lookup, AssetTable};
/// Re-exported so an app crate needs exactly one dependency: `kiln`.
pub use serde_json;
pub use config::Config;
pub use error::Error;
pub use ipc::Registry;

use std::borrow::Cow;
use std::sync::Arc;

use serde_json::Value;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use wry::{
    http::{header, Request, Response},
    WebView, WebViewBuilder,
};

/// The scheme pages are served from in production.
pub const SCHEME: &str = "kiln";

/// The JS shim injected into every page: defines `window.kiln`.
const SHIM: &str = include_str!("shim.js");

/// A handle passed to command handlers. Cheap to clone, safe to send to other
/// threads, and the only way to push data into the page.
#[derive(Clone)]
pub struct AppHandle {
    proxy: Option<EventLoopProxy<UserEvent>>,
}

enum UserEvent {
    Emit { event: String, payload: String },
}

impl AppHandle {
    fn new(proxy: Option<EventLoopProxy<UserEvent>>) -> Self {
        Self { proxy }
    }

    /// A handle with no event loop attached. Only useful with the shell's own
    /// unit tests; `emit` on it is a no-op.
    pub fn detached_for_tests() -> Self {
        Self::new(None)
    }

    /// Send an event to the page, invoking every `kiln.listen(event, fn)`
    /// callback with `payload`.
    ///
    /// This is thread-safe: the script runs on the UI thread on the next loop
    /// iteration. Events emitted before the page loads are dropped, so call it
    /// from a handler or after load rather than from `main`.
    pub fn emit(&self, event: &str, payload: Value) {
        let Some(proxy) = &self.proxy else { return };
        let envelope = UserEvent::Emit {
            event: event.to_owned(),
            payload: payload.to_string(),
        };
        let _ = proxy.send_event(envelope);
    }
}

/// The builder. Chain configuration, then `run()`.
pub struct App {
    config: Config,
    registry: Registry,
    assets: AssetTable,
    dev_url: Option<String>,
    entry: String,
    ready: Option<ReadyHook>,
}

/// Runs once after the page finishes loading.
type ReadyHook = Box<dyn Fn(&AppHandle) + Send + Sync>;

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

    /// Register a command reachable from the page as `kiln.invoke(name, payload)`.
    ///
    /// Handlers run on the UI thread and block it, so keep them short; data
    /// produced here is returned as JSON.
    pub fn command<F>(mut self, name: &str, handler: F) -> Self
    where
        F: Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.registry.add(name, handler);
        self
    }

    /// Load a specific page at startup, relative to the served root. Defaults
    /// to the config's `entry`, which defaults to `index.html`.
    pub fn entry(mut self, path: &str) -> Self {
        self.entry = path.trim_start_matches('/').to_owned();
        self
    }

    /// Run once, right after the page finishes loading.
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

    /// Open the window and run until it closes.
    ///
    /// In a debug build, `KILN_DEV_URL` redirects the webview at a dev server so
    /// the frontend hot-reloads without touching Rust. Release builds ignore it.
    pub fn run(mut self) -> Result<(), Error> {
        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
        let proxy = event_loop.create_proxy();
        let handle = AppHandle::new(Some(proxy));

        self.registry.set_allowlist(self.config.commands.clone());
        let registry = Arc::new(self.registry);
        let assets = self.assets;
        let entry = if self.entry.is_empty() {
            &self.config.entry
        } else {
            &self.entry
        };
        let target = match &self.dev_url {
            Some(url) => format!("{}/{entry}", url.trim_end_matches('/')),
            None => format!("{SCHEME}://localhost/{entry}"),
        };

        let window = WindowBuilder::new()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
            .build(&event_loop)
            .map_err(|e| Error::Runtime(e.to_string()))?;

        let origin = self.dev_url.as_deref().map(origin_of);
        let handler = handle.clone();
        let protocol_registry = Arc::clone(&registry);
        let ready = self.ready.map(Arc::new);
        let page_handle = handle.clone();

        let builder = WebViewBuilder::new()
            .with_on_page_load_handler(move |event, _url| {
                if let (wry::PageLoadEvent::Finished, Some(ready)) = (event, &ready) {
                    ready(&page_handle);
                }
            })
            .with_initialization_script(SHIM.replace("__INVOKE_BASE__", INVOKE_BASE))
            .with_custom_protocol(SCHEME.into(), move |_id, request: Request<Vec<u8>>| {
                serve(&protocol_registry, &handler, assets, origin.as_deref(), request)
            })
            .with_url(&target);

        let webview = build_webview(builder, &window)?;

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => *control_flow = ControlFlow::Exit,
                Event::UserEvent(UserEvent::Emit { event, payload }) => {
                    let script = format!(
                        "window.__kiln_emit&&window.__kiln_emit({},{})",
                        serde_json::to_string(&event).unwrap_or_else(|_| "\"\"".to_owned()),
                        payload
                    );
                    if let Err(error) = webview.evaluate_script(&script) {
                        eprintln!("kiln: emit failed: {error}");
                    }
                }
                _ => {}
            }
        });
    }
}

fn build_webview(builder: WebViewBuilder<'_>, window: &tao::window::Window) -> Result<WebView, Error> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        Ok(builder.build(window)?)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let container = window
            .default_vbox()
            .ok_or_else(|| Error::Runtime("window has no gtk container".to_owned()))?;
        Ok(builder.build_gtk(container)?)
    }
}

/// One request in, one response out. Never panics on page input.
fn serve(
    registry: &Registry,
    handle: &AppHandle,
    assets: AssetTable,
    origin: Option<&str>,
    request: Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    let mut response = Response::builder();
    if let Some(origin) = origin {
        response = response
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)
            .header(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, GET, OPTIONS")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type");
    }

    if request.method() == "OPTIONS" {
        return response.status(204).body(Cow::Borrowed(&[][..])).unwrap();
    }

    let path = request.uri().path();

    if let Some(method) = ipc::method_from_path(path) {
        if request.method() != "POST" {
            return json_error(response, 405, "invoke requires POST");
        }
        let payload = if request.body().is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(request.body()) {
                Ok(value) => value,
                Err(_) => return json_error(response, 400, "payload is not valid JSON"),
            }
        };
        let body = registry.dispatch(handle, &method, payload);
        return response
            .header(header::CONTENT_TYPE, "application/json")
            .status(200)
            .body(Cow::Owned(body.into_bytes()))
            .unwrap();
    }

    match lookup(assets, path) {
        Some((name, bytes)) => response
            .header(header::CONTENT_TYPE, content_type(name))
            .status(200)
            .body(bytes)
            .unwrap(),
        None => response
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .status(404)
            .body(Cow::Borrowed(b"not found".as_slice()))
            .unwrap(),
    }
}

fn json_error(
    response: wry::http::response::Builder,
    status: u16,
    message: &str,
) -> Response<Cow<'static, [u8]>> {
    let body = format!(r#"{{"error":{}}}"#, serde_json::Value::from(message));
    response
        .header(header::CONTENT_TYPE, "application/json")
        .status(status)
        .body(Cow::Owned(body.into_bytes()))
        .unwrap()
}

/// Where the shim sends `invoke`. The endpoint is reached through the same
/// custom protocol in both modes, so the page's own origin never matters;
/// only the dev server adds CORS headers to keep the browser layer happy.
const INVOKE_BASE: &str = "kiln://localhost/__kiln/";

fn dev_url() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("KILN_DEV_URL").ok().filter(|s| !s.is_empty())
    } else {
        None
    }
}

fn origin_of(url: &str) -> String {
    let mut parts = url.splitn(2, "://");
    let scheme = parts.next().unwrap_or("http");
    let rest = parts.next().unwrap_or("").trim_end_matches('/');
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}")
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static ASSETS: AssetTable = &[
        ("index.html", b"<!doctype html>hi"),
        ("assets/app.js", b"js"),
    ];

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.add("ping", |_h, payload| Ok(json!({ "pong": payload })));
        registry
    }

    fn get(path: &str) -> Request<Vec<u8>> {
        Request::builder().uri(path).method("GET").body(Vec::new()).unwrap()
    }

    fn post(path: &str, body: &str) -> Request<Vec<u8>> {
        Request::builder()
            .uri(path)
            .method("POST")
            .body(body.as_bytes().to_vec())
            .unwrap()
    }

    #[test]
    fn serves_index_for_root() {
        let response = serve(&registry(), &AppHandle::detached_for_tests(), ASSETS, None, get("/"));
        assert_eq!(response.status(), 200);
        assert_eq!(response.body().as_ref(), b"<!doctype html>hi");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    }

    #[test]
    fn serves_nested_assets_with_the_right_type() {
        let response = serve(
            &registry(),
            &AppHandle::detached_for_tests(),
            ASSETS,
            None,
            get("/assets/app.js"),
        );
        assert_eq!(response.status(), 200);
        assert!(response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/javascript"));
    }

    #[test]
    fn missing_asset_is_404() {
        let response = serve(&registry(), &AppHandle::detached_for_tests(), ASSETS, None, get("/x.css"));
        assert_eq!(response.status(), 404);
    }

    #[test]
    fn invoke_round_trips_json() {
        let response = serve(
            &registry(),
            &AppHandle::detached_for_tests(),
            ASSETS,
            None,
            post("/__kiln/ping", r#"{"x":1}"#),
        );
        assert_eq!(response.status(), 200);
        let body: Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(body, json!({ "ok": { "pong": { "x": 1 } } }));
    }

    #[test]
    fn invoke_rejects_get_and_bad_json() {
        let response = serve(
            &registry(),
            &AppHandle::detached_for_tests(),
            ASSETS,
            None,
            get("/__kiln/ping"),
        );
        assert_eq!(response.status(), 405);

        let response = serve(
            &registry(),
            &AppHandle::detached_for_tests(),
            ASSETS,
            None,
            post("/__kiln/ping", "not json"),
        );
        assert_eq!(response.status(), 400);
        let body: Value = serde_json::from_slice(response.body()).unwrap();
        assert!(body["error"].is_string());
    }

    #[test]
    fn unknown_command_returns_an_error_envelope() {
        let response = serve(
            &registry(),
            &AppHandle::detached_for_tests(),
            ASSETS,
            None,
            post("/__kiln/nope", "null"),
        );
        let body: Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(body["error"], "unknown command: nope");
    }

    #[test]
    fn dev_origin_gets_cors_headers() {
        let response = serve(
            &registry(),
            &AppHandle::detached_for_tests(),
            ASSETS,
            Some("http://127.0.0.1:8799"),
            post("/__kiln/ping", "null"),
        );
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://127.0.0.1:8799"
        );
    }

    #[test]
    fn origin_is_extracted_from_urls() {
        assert_eq!(origin_of("http://127.0.0.1:8799/"), "http://127.0.0.1:8799");
        assert_eq!(origin_of("http://localhost:3000"), "http://localhost:3000");
    }

    #[test]
    fn assets_are_looked_up_by_full_extension_not_prefix() {
        assert_eq!(content_type("a.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("a.css"), "text/css; charset=utf-8");
    }

    #[test]
    fn repeated_invokes_are_independent() {
        let registry = registry();
        let handle = AppHandle::detached_for_tests();
        for i in 0..3 {
            let response = serve(
                &registry,
                &handle,
                ASSETS,
                None,
                post("/__kiln/ping", &format!(r#"{{"i":{i}}}"#)),
            );
            let body: Value = serde_json::from_slice(response.body()).unwrap();
            assert_eq!(body["ok"]["pong"]["i"], i);
        }
    }
}
