//! Test fixture for `scripts/verify.ts`.
//!
//! Three commands exist only so the verifier can observe shell behaviour through
//! the real IPC path: `boom` panics on purpose, `alive` answers to prove the app
//! survived, and `report` prints the probe's findings to stdout so the harness
//! can assert on them without a human looking at a window.
//!
//! This is deliberately a separate crate: none of it belongs in the framework or
//! in the scaffold users get.

use kiln::serde_json::{json, Value};
use kiln::{App, AssetTable};

/// A typed command's argument type. `text` is required, so a payload without it
/// is rejected before the body runs, which is the behaviour the probe measures.
#[derive(kiln::serde::Deserialize)]
#[serde(crate = "kiln::serde")]
struct EchoArgs {
    text: String,
}

const INDEX: &[u8] = include_bytes!("../index.html");
const PROBE: &[u8] = include_bytes!("../probe.js");

/// The probe page is served with the hostile origin substituted in, because the
/// JS runs in the webview and cannot read the shell's environment.
static HOSTILE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn main() -> Result<(), kiln::Error> {
    let hostile = std::env::var("KILN_HOSTILE").unwrap_or_else(|_| "http://127.0.0.1:1".to_owned());
    let _ = HOSTILE.set(hostile.clone());

    // Built at startup so the embedded page can name the hostile origin and the
    // origin its own IPC actually reaches: WebView2 and Android serve the page
    // from the rewritten http origin, so `kiln://localhost` never resolves there.
    let probe_js: &'static [u8] = Box::leak(
        String::from_utf8_lossy(PROBE)
            .replace("__HOSTILE__", &hostile)
            .replace("__PROBE_ORIGIN__", kiln::PAGE_ORIGIN)
            .into_bytes()
            .into_boxed_slice(),
    );
    let assets: AssetTable =
        Box::leak(vec![("index.html", INDEX), ("probe.js", probe_js)].into_boxed_slice());

    App::new()
        .assets(assets)
        .command("boom", |_app, _payload| {
            let empty: Vec<u8> = Vec::new();
            // Intentionally out of bounds: this is the panic under test.
            let _ = empty[5];
            Ok(Value::Null)
        })
        .command("alive", |_app, _payload| Ok(json!("still alive")))
        // Shell-side state: written before a reload and read after one would be
        // the full proof, but a single run can still prove the store is shared
        // between a handler and the page rather than per-request.
        .command("remember", |_app, payload| {
            kiln::state::set("probe.remember", payload);
            Ok(kiln::state::get("probe.remember").unwrap_or(Value::Null))
        })
        // A page cannot reach this one from a narrowed window, which is what the
        // harness asserts: the label decides the lane, not the payload.
        .command("narrowed_out", |_app, _payload| {
            Ok(json!("should be unreachable"))
        })
        .command_typed("typed_echo", |_app, args: EchoArgs| Ok(json!(args.text)))
        .command("report", |_app, payload| {
            println!("PROBE {payload}");
            Ok(Value::Null)
        })
        .run()
}
