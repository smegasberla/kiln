/// <reference path="./kiln.d.ts" />

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
    show(await window.kiln.invoke<string>("greet", { name: nameInput.value }));
  } catch (error) {
    show("error: " + (error as Error).message);
  }
});

// An async command: the window stays responsive while this runs.
slowButton.addEventListener("click", async () => {
  slowButton.disabled = true;
  show("working off the UI thread...");
  try {
    const reply = await window.kiln.invoke<{ slept_ms: number }>("slow_task", { ms: 800 });
    show(`async command finished after ${reply.slept_ms} ms`);
  } catch (error) {
    show("error: " + (error as Error).message);
  } finally {
    slowButton.disabled = false;
  }
});

// A second window is a desktop capability. Rather than offer a button that errors
// on Android, hide it: the page is shared, the affordances are not.
if (window.kiln.platform === "android" || window.kiln.platform === "ios") {
  settingsButton.hidden = true;
} else {
  settingsButton.addEventListener("click", () => {
    window.kiln.invoke("open_settings").catch((error) => show("error: " + error.message));
  });
}

// Menu clicks arrive as events, so a native menu needs no extra wiring here.
window.kiln.listen<{ id: string }>("menu", (event) => show(`menu: ${event.id}`));

form.requestSubmit();
