// Injected before every page load, on every frame.
//
// The whole frontend-side runtime: keep it dependency-free and small.
//
// Two transports, because the platforms genuinely differ:
//
//   - Desktop: `invoke` is a POST to a custom protocol, which answers directly.
//   - Android: `WebResourceRequest` carries no body, so wry's custom protocol
//     cannot receive a payload there. Instead requests go over `window.ipc`, which
//     wry wires to a native handler; the reply comes back through
//     `__kiln_resolve` on the next event-loop turn.
//
// The page sees one API either way: `kiln.invoke(method, payload)`.
(() => {
  const base = "__INVOKE_BASE__";
  const allowed = __ALLOWED_ORIGINS__;
  const bridge = __BRIDGE__;
  const listeners = new Map();
  const pending = new Map();
  let nextId = 1;

  function originAllowed() {
    if (allowed.length === 0) return true;
    const here = location.origin;
    if (allowed.indexOf(here) !== -1) return true;
    // Custom protocols report an opaque origin on some platforms; fall back to
    // the scheme the page was actually loaded from.
    return location.protocol === "kiln:";
  }

  function teardown() {
    listeners.clear();
    pending.clear();
    try {
      delete window.kiln;
    } catch {
      window.kiln = undefined;
    }
  }

  if (!originAllowed()) {
    teardown();
    return;
  }

  /** Resolve a bridge request. Called from Rust via evaluate_script. */
  window.__kiln_resolve = (id, envelope) => {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    if (envelope.error !== undefined) entry.reject(toError(envelope.error));
    else entry.resolve(envelope.ok);
  };

  // Errors arrive as {message, kind, method} since the structured envelope
  // shipped; the plain-string form is still accepted so an older shell or the
  // hot-reload dylib answering a newer page rejects with readable text either
  // way. The kind rides along on the Error so frontends can branch on it.
  function toError(error) {
    if (typeof error === "string") return new Error(error);
    const err = new Error((error && error.message) || "command failed");
    if (error && error.kind) err.kind = error.kind;
    if (error && error.method) err.method = error.method;
    return err;
  }

  function invokeOverBridge(method, payload) {
    if (typeof window.ipc === "undefined" || !window.ipc.postMessage) {
      return Promise.reject(new Error("kiln: bridge unavailable"));
    }
    const id = nextId++;
    const message = JSON.stringify({
      id,
      method,
      payload: payload === undefined ? null : payload,
    });
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      window.ipc.postMessage(message);
    });
  }

  async function invokeOverProtocol(method, payload) {
    const res = await fetch(base + encodeURIComponent(method), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload === undefined ? null : payload),
    });
    const text = await res.text();
    let envelope;
    try {
      envelope = JSON.parse(text);
    } catch {
      throw new Error("kiln: malformed response for " + method);
    }
    if (envelope.error !== undefined) throw toError(envelope.error);
    return envelope.ok;
  }

  async function invoke(method, payload) {
    if (!originAllowed()) {
      teardown();
      throw new Error("kiln: bridge unavailable on this origin");
    }
    return bridge ? invokeOverBridge(method, payload) : invokeOverProtocol(method, payload);
  }

  function listen(event, fn) {
    let set = listeners.get(event);
    if (!set) listeners.set(event, (set = new Set()));
    set.add(fn);
    return () => set.delete(fn);
  }

  window.__kiln_emit = (event, payload) => {
    const set = listeners.get(event);
    if (set) for (const fn of set) fn(payload);
  };

  // The open window labels, in the order they opened. A page that opens a window
  // gets its label back, but it still needs to know what closed.
  async function windows() {
    const reply = await invoke("__kiln_windows");
    return (reply && reply.windows) || [];
  }

  // Tray and menu state, so a page changes a tick without re-declaring anything.
  // Each resolves to null; a platform without the affordance rejects with the
  // shell's reason rather than pretending it worked.
  const tray = {
    tooltip: (text) => invoke("__kiln_tray_tooltip", { text }),
    checked: (id, checked) => invoke("__kiln_tray_checked", { id, checked }),
    popup: () => invoke("__kiln_tray_popup"),
  };
  const menu = {
    checked: (id, checked) => invoke("__kiln_menu_checked", { id, checked }),
  };

  // The page is shared across platforms, so it needs to know which one it is on.
  // Injected at build time rather than sniffed: a WebView user-agent lies more
  // often than it helps.
  // `protocol` is the shim's own version, not the shell's. Frontend code
  // feature-detects with it instead of probing for methods: a page that needs
  // `windows()` asks `kiln.protocol >= 2` and degrades once, in one place.
  window.kiln = {
    invoke,
    listen,
    windows,
    tray,
    menu,
    platform: "__PLATFORM__",
    // The origin this page is actually served from. On Windows it is the
    // rewritten host form, because WebView2 cannot intercept a custom scheme, so
    // a page that builds an absolute URL from a literal is correct on four
    // targets and broken on one. Read it here instead of assuming.
    pageOrigin: "__PAGE_ORIGIN__",
    protocol: __SHIM_PROTOCOL__,
  };
})();
