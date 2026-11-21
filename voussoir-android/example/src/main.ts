/// <reference path="./voussoir.d.ts" />

const result = document.getElementById("result")!;
const form = document.getElementById("greet") as HTMLFormElement;
const nameInput = document.getElementById("name") as HTMLInputElement;
const slowButton = document.getElementById("slow") as HTMLButtonElement;
const settingsButton = document.getElementById("settings") as HTMLButtonElement;

function show(text: string) {
  result.textContent = text;
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  show("invoking...");
  try {
    show(await window.voussoir.invoke<string>("greet", { name: nameInput.value }));
  } catch (error) {
    show("error: " + (error as Error).message);
  }
});

// An async command: the window stays responsive while this runs.
slowButton.addEventListener("click", async () => {
  slowButton.disabled = true;
  show("working off the UI thread...");
  try {
    const reply = await window.voussoir.invoke<{ slept_ms: number }>("slow_task", { ms: 800 });
    show(`async command finished after ${reply.slept_ms} ms`);
  } catch (error) {
    show("error: " + (error as Error).message);
  } finally {
    slowButton.disabled = false;
  }
});

// A second window is a desktop capability. Rather than offer a button that errors
// on Android, hide it: the page is shared, the affordances are not.
if (window.voussoir.platform === "android" || window.voussoir.platform === "ios") {
  settingsButton.hidden = true;
} else {
  settingsButton.addEventListener("click", () => {
    window.voussoir.invoke("open_settings").catch((error) => show("error: " + error.message));
  });
}

// Lifecycle transitions reach the page as events. Reported rather than
// interpreted: a web page has no state machine to run, but knowing that it was
// paused tells it to drop a timer rather than fire while nothing is visible.
// The `note` command forwards the line to logcat, which is how the device test
// in the README observes it.
window.voussoir.listen<{ state: string }>("lifecycle", ({ state }) => {
  show(`lifecycle: ${state}`);
  window.voussoir.invoke("note", { message: `lifecycle ${state}` }).catch(() => {});
});

// A rotation is a configuration change the activity absorbs rather than a
// restart, so the page has to prove it is still alive afterwards. Reporting the
// new size does that: the line only appears if this same page ran the script.
window.addEventListener("resize", () => {
  window.voussoir
    .invoke("note", { message: `resized to ${window.innerWidth}x${window.innerHeight}` })
    .catch(() => {});
});

// Menu clicks arrive as events, so a native menu needs no extra wiring here.
window.voussoir.listen<{ id: string }>("menu", (event) => show(`menu: ${event.id}`));

form.requestSubmit();
