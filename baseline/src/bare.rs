// Baseline: the bare wry + tao hello world that Kiln is measured against.
// No IPC, no framework code. `KILN_BENCH=1` prints time-to-first-paint and exits.

use std::io::Write;
use std::time::Instant;

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

const HTML: &str = r#"<!doctype html><html><body style="margin:0;display:grid;place-items:center;height:100vh;font:16px system-ui">
<h1>bare wry + tao</h1></body></html>"#;

fn main() -> wry::Result<()> {
    let t0 = Instant::now();
    let bench = std::env::var_os("KILN_BENCH").is_some();

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("bare")
        .with_inner_size(tao::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .unwrap();

    let builder = WebViewBuilder::new()
        .with_html(HTML)
        .with_on_page_load_handler(move |event, _url| {
            if bench && matches!(event, wry::PageLoadEvent::Finished) {
                println!("READY {}", t0.elapsed().as_millis());
                let _ = std::io::stdout().flush();
                std::process::exit(0);
            }
        });

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let _webview = builder.build(&window)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().unwrap())?
    };

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
