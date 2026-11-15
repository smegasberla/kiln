// Injected before every page load, on every frame.
//
// This is the whole frontend-side runtime: keep it dependency-free and small.
// Two rules it enforces on its own, so a mistake in the native layer cannot leak
// the bridge:
//
//   1. The bridge only exists on the app's own origin. Navigation to a foreign
//      origin must not leave `window.kiln` reachable, because `invoke` executes
//      app commands.
//   2. It is torn down if the document ever becomes remote anyway.
(() => {
  const base = "__INVOKE_BASE__";
  const allowed = __ALLOWED_ORIGINS__;
  const listeners = new Map();

  function originAllowed() {
    if (allowed.length === 0) return true;
    const here = location.origin;
    if (allowed.indexOf(here) !== -1) return true;
    // Custom protocols report an opaque origin on some platforms; fall back to
    // comparing the scheme the page was actually loaded from.
    return location.protocol === "kiln:";
  }

  function teardown() {
    listeners.clear();
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

  async function invoke(method, payload) {
    if (!originAllowed()) {
      teardown();
      throw new Error("kiln: bridge unavailable on this origin");
    }
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
    if (envelope.error !== undefined) throw new Error(envelope.error);
    return envelope.ok;
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

  window.kiln = { invoke, listen };
})();
