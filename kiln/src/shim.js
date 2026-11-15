// Injected before every page load (all frames).
// Keep this dependency-free and small: it is the whole frontend-side runtime.
(() => {
  const base = "__INVOKE_BASE__";
  const listeners = new Map();

  async function invoke(method, payload) {
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
