// Phase 0 probe: decides the IPC transport, and the dev-mode origin story.
//
//   A) wry `with_ipc_handler` (window.ipc.postMessage) - fire and forget.
//   B) POST to the custom protocol - request/response, response read as text.
//
// Serve-time behaviour:
//   - KILN_PAGE_URL unset  -> page served from kiln:// (production path)
//   - KILN_PAGE_URL set    -> page served from a Bun dev server over http
//
// The page reports each transport's outcome over the IPC channel; the probe
// prints it and exits, so this is verifiable without a human looking at pixels.

use std::io::Write;
use std::sync::mpsc;

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::{
    http::{header, Request, Response},
    WebViewBuilder,
};

const PAGE: &str = r#"<!doctype html><html><body>
<pre id="out">running</pre>
<script>
const log = [];
function say(s) { log.push(s); document.getElementById('out').textContent = log.join('\n'); }
(async () => {
  const results = {};
  try {
    const r = await fetch('kiln://localhost/__kiln/greet', { method: 'POST', body: '{"name":"json"}' });
    results.protocol = r.status + ' ' + (await r.text());
  } catch (e) { results.protocol = 'THREW ' + e; }
  say('protocol: ' + results.protocol);
  window.ipc.postMessage(JSON.stringify({ method: 'greet', payload: { name: 'ipc' } }));
  results.ipc = 'sent';
  say('ipc: postMessage sent');
  window.ipc.postMessage(JSON.stringify({ method: '__probe_done', payload: results }));
})();
</script></body></html>"#;

fn main() -> wry::Result<()> {
    let (tx, rx) = mpsc::channel::<String>();
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("probe")
        .with_inner_size(tao::dpi::LogicalSize::new(700.0, 400.0))
        .build(&event_loop)
        .unwrap();

    let dev_url = std::env::var("KILN_PAGE_URL").ok();
    let origin = dev_url.as_deref().map(|u| {
        let scheme = u.split("://").next().unwrap_or("http");
        let host = u.split("://").nth(1).unwrap_or("").trim_end_matches('/');
        format!("{scheme}://{host}")
    });

    let tx2 = tx.clone();
    let builder = WebViewBuilder::new()
        .with_custom_protocol("kiln".into(), move |_id, req: Request<Vec<u8>>| {
            let cors = origin.clone();
            let mut b = Response::builder();
            if let Some(o) = &cors {
                b = b
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, o)
                    .header(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, GET, OPTIONS")
                    .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "*");
            }
            let path = req.uri().path().to_string();
            if req.method() == "OPTIONS" {
                println!("[protocol] preflight {path}");
                let _ = std::io::stdout().flush();
                return b.status(204).body(Vec::new().into()).unwrap();
            }
            if path == "/__kiln/greet" {
                let body = String::from_utf8_lossy(req.body()).to_string();
                println!("[protocol] POST {path} body={body}");
                let _ = std::io::stdout().flush();
                let out = format!(
                    "{{\"reply\":\"hello from rust\",\"echo\":{}}}",
                    if body.is_empty() {
                        "null".to_string()
                    } else {
                        body
                    }
                );
                return b
                    .header(header::CONTENT_TYPE, "application/json")
                    .status(200)
                    .body(out.into_bytes().into())
                    .unwrap();
            }
            println!("[protocol] GET {path}");
            let _ = std::io::stdout().flush();
            b.header(header::CONTENT_TYPE, "text/html")
                .status(200)
                .body(PAGE.as_bytes().to_vec().into())
                .unwrap()
        })
        .with_ipc_handler(move |req: Request<String>| {
            println!("[ipc] {}", req.body());
            let _ = std::io::stdout().flush();
            let _ = tx2.send(req.body().clone());
        });

    let builder = match dev_url {
        Some(url) => builder.with_url(url),
        None => builder.with_url("kiln://localhost/"),
    };

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let _webview = builder.build(&window)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().unwrap())?
    };

    std::thread::spawn(move || {
        if let Ok(msg) = rx.recv_timeout(std::time::Duration::from_secs(15)) {
            println!("[probe] REPORT {msg}");
        } else {
            println!("[probe] TIMEOUT");
        }
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
