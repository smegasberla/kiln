(async () => {
  const HOSTILE = "__HOSTILE__";
  const ORIGIN = "__PROBE_ORIGIN__";
  const out = { remoteScriptExecuted: typeof window.__evil !== "undefined" };

  try {
    await fetch(`${HOSTILE}/beacon`, { method: "POST", body: "x" });
    out.remoteFetch = "ALLOWED";
  } catch {
    out.remoteFetch = "blocked";
  }

  try {
    new Function("return 1")();
    out.eval = "ALLOWED";
  } catch {
    out.eval = "blocked";
  }

  try {
    const res = await fetch(`${ORIGIN}/__kiln/boom`, { method: "POST", body: "null" });
    out.panic = await res.text();
  } catch (error) {
    out.panic = "THREW " + error;
  }

  try {
    const res = await fetch(`${ORIGIN}/__kiln/alive`, { method: "POST", body: "null" });
    out.after = (await res.json()).ok;
  } catch (error) {
    out.after = "THREW " + error;
  }

  // Typed commands: a wrong shape must come back as `payload`, naming the
  // command, without the handler body ever running.
  try {
    const res = await fetch(`${ORIGIN}/__kiln/typed_echo`, {
      method: "POST",
      body: JSON.stringify({ text: 5 }),
    });
    const envelope = await res.json();
    out.typed = envelope.error ? `${envelope.error.kind}/${envelope.error.method}` : "ALLOWED";
  } catch (error) {
    out.typed = "THREW " + error;
  }

  // The same command with the right shape, to prove the typed path works at all
  // and not merely that it rejects.
  try {
    const res = await fetch(`${ORIGIN}/__kiln/typed_echo`, {
      method: "POST",
      body: JSON.stringify({ text: "ok" }),
    });
    out.typedOk = (await res.json()).ok;
  } catch (error) {
    out.typedOk = "THREW " + error;
  }

  // Storage: persist a value, read it back. This is the only check that proves
  // the platform data directory is writable on the target under test.
  try {
    await fetch(`${ORIGIN}/__kiln/storage.set`, {
      method: "POST",
      body: JSON.stringify({ key: "probe", value: { round: 1 } }),
    });
    const got = await (
      await fetch(`${ORIGIN}/__kiln/storage.get`, {
        method: "POST",
        body: JSON.stringify({ key: "probe" }),
      })
    ).json();
    out.storage = got.ok;
    const keys = await (
      await fetch(`${ORIGIN}/__kiln/storage.keys`, { method: "POST", body: "null" })
    ).json();
    out.storageKeys = Array.isArray(keys.ok) && keys.ok.indexOf("probe") !== -1;
  } catch (error) {
    out.storage = "THREW " + error;
  }

  out.nav = await new Promise((resolve) => {
    const before = location.href;
    try {
      location.href = `${HOSTILE}/escaped`;
    } catch {
      /* navigation may be refused synchronously */
    }
    setTimeout(() => resolve(location.href === before ? "stayed" : "MOVED:" + location.href), 700);
  });

  try {
    await fetch(`${ORIGIN}/__kiln/report`, { method: "POST", body: JSON.stringify(out) });
  } catch (error) {
    console.error("probe report failed: " + error);
  }
})();
