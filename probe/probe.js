(async () => {
  const HOSTILE = "__HOSTILE__";
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
    const res = await fetch("kiln://localhost/__kiln/boom", { method: "POST", body: "null" });
    out.panic = await res.text();
  } catch (error) {
    out.panic = "THREW " + error;
  }

  try {
    const res = await fetch("kiln://localhost/__kiln/alive", { method: "POST", body: "null" });
    out.after = (await res.json()).ok;
  } catch (error) {
    out.after = "THREW " + error;
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
    await fetch("kiln://localhost/__kiln/report", { method: "POST", body: JSON.stringify(out) });
  } catch (error) {
    console.error("probe report failed: " + error);
  }
})();
