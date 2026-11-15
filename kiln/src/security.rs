//! Navigation and security policy for the webview.
//!
//! A desktop app is not a browser. The page may only ever load from the app's
//! own origin (or the dev server in a debug build); anything else would replace
//! the UI with a foreign page and strand the user there, with no chrome to get
//! back. These rules are pure functions so they are testable without a window.

/// The origin pages are served from in production.
pub const APP_ORIGIN: &str = "kiln://localhost";

/// Whether `url` may be loaded in the app's webview.
///
/// Allowed:
/// - the app origin over the custom protocol
/// - `about:blank` (initial document, and what some libraries use)
/// - the dev-server origin, only when one was configured (debug builds)
///
/// Everything else is refused. `dev_origin` is `None` in release builds, so a
/// release binary cannot be redirected anywhere at runtime.
pub fn navigation_allowed(url: &str, dev_origin: Option<&str>) -> bool {
    if url == "about:blank" {
        return true;
    }
    if let Some(dev) = dev_origin {
        if url == dev || url.starts_with(&format!("{dev}/")) {
            return true;
        }
    }
    url == APP_ORIGIN || url.starts_with(&format!("{APP_ORIGIN}/"))
}

/// The Content-Security-Policy served with every response.
///
/// `script-src 'self'` matters most: without it a compromised or careless page
/// can pull a remote script into a context that holds the IPC bridge. `unsafe-inline`
/// for styles only, because inline `<style>` is normal in bundled CSS output and
/// style tags cannot reach the bridge.
///
/// In dev the policy is relaxed: Bun's HMR client uses inline scripts and an
/// eval-based module loader, and `ws:` for the reload channel. That relaxation
/// is scoped to the dev origin, never to a shipped app.
fn csp(dev: bool) -> &'static str {
    if dev {
        "default-src 'self'; \
         script-src 'self' 'unsafe-inline' 'unsafe-eval' kiln:; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; \
         font-src 'self' data:; \
         connect-src 'self' kiln: ws: http://127.0.0.1:* http://localhost:*; \
         object-src 'none'; \
         base-uri 'none'; \
         form-action 'none'"
    } else {
        "default-src 'self'; \
         script-src 'self'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; \
         font-src 'self' data:; \
         connect-src 'self' kiln:; \
         object-src 'none'; \
         base-uri 'none'; \
         form-action 'none'; \
         frame-ancestors 'none'"
    }
}

/// The CSP to attach to a response, or `None` if the caller asked for none.
pub fn content_security_policy(dev: bool) -> &'static str {
    csp(dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_origin_is_allowed() {
        assert!(navigation_allowed("kiln://localhost/", None));
        assert!(navigation_allowed("kiln://localhost/index.html", None));
        assert!(navigation_allowed("kiln://localhost/assets/app.js", None));
        assert!(navigation_allowed("about:blank", None));
    }

    #[test]
    fn remote_pages_are_refused() {
        for url in [
            "http://example.com",
            "https://example.com/",
            "http://127.0.0.1:5601/escaped",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<h1>hi</h1>",
            "kiln:/etc/passwd",
            "kiln://evil.com/",
        ] {
            assert!(!navigation_allowed(url, None), "{url} must be refused");
        }
    }

    #[test]
    fn prefix_match_does_not_leak_across_hosts() {
        // A naive starts_with("kiln://localhost") would wrongly allow these.
        assert!(!navigation_allowed("kiln://localhost.evil.com/", None));
        assert!(!navigation_allowed("kiln://localhost@evil.com/", None));
    }

    #[test]
    fn dev_origin_is_allowed_only_when_configured() {
        let dev = Some("http://127.0.0.1:5177");
        assert!(navigation_allowed("http://127.0.0.1:5177/", dev));
        assert!(navigation_allowed("http://127.0.0.1:5177/index.html", dev));
        // Different port and different host stay refused.
        assert!(!navigation_allowed("http://127.0.0.1:5178/", dev));
        assert!(!navigation_allowed("http://example.com/", dev));
        // And without a dev origin, the same URL is refused outright.
        assert!(!navigation_allowed("http://127.0.0.1:5177/", None));
    }

    #[test]
    fn release_csp_forbids_remote_script() {
        let policy = content_security_policy(false);
        assert!(policy.contains("script-src 'self'"));
        assert!(!policy.contains("http://"), "release CSP must not allow http");
        assert!(!policy.contains("'unsafe-eval'"));
        assert!(policy.contains("object-src 'none'"));
        assert!(policy.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn dev_csp_allows_what_hmr_needs_and_nothing_more() {
        let policy = content_security_policy(true);
        assert!(policy.contains("ws:"));
        assert!(policy.contains("http://127.0.0.1:*"));
        assert!(policy.contains("object-src 'none'"));
        assert!(!policy.contains("https://"), "dev CSP must not open the internet");
    }
}
