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
    const res = await fetch(`${ORIGIN}/__voussoir/boom`, { method: "POST", body: "null" });
    out.panic = await res.text();
  } catch (error) {
    out.panic = "THREW " + error;
  }

  try {
    const res = await fetch(`${ORIGIN}/__voussoir/alive`, { method: "POST", body: "null" });
    out.after = (await res.json()).ok;
  } catch (error) {
    out.after = "THREW " + error;
  }

  // Typed commands: a wrong shape must come back as `payload`, naming the
  // command, without the handler body ever running.
  try {
    const res = await fetch(`${ORIGIN}/__voussoir/typed_echo`, {
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
    const res = await fetch(`${ORIGIN}/__voussoir/typed_echo`, {
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
    await fetch(`${ORIGIN}/__voussoir/storage.set`, {
      method: "POST",
      body: JSON.stringify({ key: "probe", value: { round: 1 } }),
    });
    const got = await (
      await fetch(`${ORIGIN}/__voussoir/storage.get`, {
        method: "POST",
        body: JSON.stringify({ key: "probe" }),
      })
    ).json();
    out.storage = got.ok;
    const keys = await (
      await fetch(`${ORIGIN}/__voussoir/storage.keys`, { method: "POST", body: "null" })
    ).json();
    out.storageKeys = Array.isArray(keys.ok) && keys.ok.indexOf("probe") !== -1;
  } catch (error) {
    out.storage = "THREW " + error;
  }

  // Windows: open one with a narrower allowlist, confirm the label round-trips
  // and that the lane is enforced. The restricted command must be unreachable
  // from the new window while the page itself still works.
  try {
    const opened = await (
      await fetch(`${ORIGIN}/__voussoir/__voussoir_open_window`, {
        method: "POST",
        body: JSON.stringify({
          label: "probe-narrow",
          title: "probe narrow",
          width: 320,
          height: 240,
          // Multi-window bookkeeping stays reachable so this window can be
          // listed and closed; the app's own surface is narrowed to one command.
          allowlist: ["__voussoir_windows", "__voussoir_close_window", "alive"],
        }),
      })
    ).json();
    out.openLabel = opened.ok && opened.ok.label;

    const listed = await (
      await fetch(`${ORIGIN}/__voussoir/__voussoir_windows`, { method: "POST", body: "null" })
    ).json();
    out.windows = listed.ok && listed.ok.windows;
    out.windowsIncludes = Array.isArray(out.windows) && out.windows.indexOf("probe-narrow") !== -1;

    // An unnamed second window must get a distinct label, or the page could not
    // address either of them.
    const second = await (
      await fetch(`${ORIGIN}/__voussoir/__voussoir_open_window`, {
        method: "POST",
        body: JSON.stringify({ width: 200, height: 150 }),
      })
    ).json();
    out.unnamedLabel = second.ok && second.ok.label;

    await fetch(`${ORIGIN}/__voussoir/__voussoir_close_window`, {
      method: "POST",
      body: JSON.stringify({ label: "probe-narrow" }),
    });
  } catch (error) {
    out.openLabel = "THREW " + error;
  }

  // State that lives in the shell rather than in the page or the command crate.
  try {
    const res = await fetch(`${ORIGIN}/__voussoir/remember`, {
      method: "POST",
      body: JSON.stringify({ round: 2 }),
    });
    out.state = (await res.json()).ok;
  } catch (error) {
    out.state = "THREW " + error;
  }

  // The shim version, which is what a frontend feature-detects on.
  try {
    const shim = await (
      await fetch(`${ORIGIN}/__voussoir/__voussoir_version`, { method: "POST", body: "null" })
    ).json();
    out.voussoirVersion = shim.ok && shim.ok.voussoir;
  } catch (error) {
    out.voussoirVersion = "THREW " + error;
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
    await fetch(`${ORIGIN}/__voussoir/report`, { method: "POST", body: JSON.stringify(out) });
  } catch (error) {
    console.error("probe report failed: " + error);
  }
})();
