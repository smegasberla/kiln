/// <reference path="./kiln.d.ts" />

/**
 * Scratch's page: a text editor bound to one note at a time.
 *
 * The rule this file follows is that Rust owns the notes and the page owns the
 * draft. Everything the user types lives in the textarea until it is saved, so
 * typing never waits on a lock or a disk; what comes back from `save` replaces the
 * draft only if the user has not typed since, which is what stops a slow save from
 * reverting the last sentence.
 */

type Summary = {
  key: string;
  title: string;
  modified: number;
  words: number;
  characters: number;
  open: boolean;
};

type Snapshot = {
  documents: Summary[];
  open: string | null;
  body: string | null;
  directory: string | null;
};

type Counts = { words: number; characters: number; lines: number };

const platform = window.kiln.platform;
const isMobile = platform === "android" || platform === "ios";

const titleLabel = document.getElementById("title") as HTMLElement;
const savedLabel = document.getElementById("saved") as HTMLElement;
const editor = document.getElementById("editor") as HTMLTextAreaElement;
const list = document.getElementById("documents") as HTMLUListElement;
const emptyHint = document.getElementById("empty") as HTMLElement;
const counts = document.getElementById("counts") as HTMLElement;
const locationLabel = document.getElementById("location") as HTMLElement;
const sidebar = document.getElementById("sidebar") as HTMLElement;

/** The note the editor is showing, and whether the text differs from the disk. */
let openKey: string | null = null;
let dirty = false;
/** Bumped on every edit, so a save that returns late can tell it is stale. */
let generation = 0;

function formatModified(millis: number): string {
  if (!millis) return "";
  const seconds = Math.round((Date.now() - millis) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.round(seconds / 3600)}h ago`;
  return new Date(millis).toLocaleDateString();
}

function renderList(snapshot: Snapshot) {
  list.replaceChildren();
  emptyHint.hidden = snapshot.documents.length > 0;

  for (const note of snapshot.documents) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = note.open ? "note current" : "note";
    button.dataset.key = note.key;
    if (note.open) button.setAttribute("aria-current", "true");

    const name = document.createElement("span");
    name.className = "name";
    name.textContent = note.title;

    const meta = document.createElement("span");
    meta.className = "meta";
    meta.textContent = `${note.words} words · ${formatModified(note.modified)}`;

    button.append(name, meta);
    button.addEventListener("click", () => void selectNote(note.key));
    item.append(button);
    list.append(item);
  }
}

function renderSnapshot(snapshot: Snapshot, options: { keepDraft?: boolean } = {}) {
  renderList(snapshot);
  openKey = snapshot.open;

  const open = snapshot.documents.find((note) => note.open);
  titleLabel.textContent = open ? open.title : "Scratch";

  // The draft wins over the disk when the user is mid-sentence: a save, a rename
  // from another window, or a reload must not scroll the caret back.
  if (!options.keepDraft && open && snapshot.body !== null) {
    editor.value = snapshot.body;
    dirty = false;
  }

  if (snapshot.directory && !locationLabel.textContent) {
    locationLabel.textContent = snapshot.directory;
    locationLabel.hidden = false;
    locationLabel.title = snapshot.directory;
  }

  // Mobile has no menu bar, so the overflow button is the only way to reach the
  // shell's PopupMenu. On desktop the menu bar is already there and the button
  // would be redundant chrome.
  (document.getElementById("menu") as HTMLElement).hidden = !isMobile;
}

async function selectNote(key: string) {
  if (key === openKey) {
    if (isMobile) sidebar.classList.remove("open");
    return;
  }
  await flushSave();
  try {
    renderSnapshot(await window.kiln.invoke<Snapshot>("open", { key }));
  } catch (error) {
    showStatus(`error: ${(error as Error).message}`);
  }
  if (isMobile) sidebar.classList.remove("open");
}

function showStatus(text: string) {
  savedLabel.textContent = text;
}

async function flushSave() {
  if (!dirty || !openKey) return;
  const body = editor.value;
  const mine = generation;
  dirty = false;
  showStatus("saving…");
  try {
    const reply = await window.kiln.invoke<{ statistics: Counts; snapshot: Snapshot }>("save", {
      key: openKey,
      body,
    });
    renderCounts(reply.statistics);
    // Only adopt the reply's view if nothing was typed while the save was in
    // flight; otherwise this would overwrite the newer draft with the older text.
    renderSnapshot(reply.snapshot, { keepDraft: generation !== mine });
    if (generation === mine) showStatus("saved");
  } catch (error) {
    dirty = true;
    showStatus(`not saved: ${(error as Error).message}`);
  }
}

function renderCounts(statistics: Counts) {
  counts.textContent = `${statistics.words} words · ${statistics.characters} chars · ${statistics.lines} lines`;
}

/** Save once typing pauses, which is the behaviour a notebook app is judged on. */
let timer: number | undefined;
function scheduleSave() {
  window.clearTimeout(timer);
  timer = window.setTimeout(() => void flushSave(), 700);
}

editor.addEventListener("input", () => {
  dirty = true;
  generation += 1;
  showStatus("unsaved");
  // Counted locally for the status bar: this runs per keystroke and a round trip
  // to Rust for every character would be felt on a slow device.
  const text = editor.value;
  renderCounts({
    words: text.split(/\s+/).filter(Boolean).length,
    characters: [...text].length,
    lines: text === "" ? 0 : text.split("\n").length,
  });
  scheduleSave();
});

// Leaving the page is the last chance to write. Not awaited: the browser will not
// wait for a promise here, and the save is already off the UI thread in Rust.
window.addEventListener("pagehide", () => {
  if (dirty && openKey) {
    void window.kiln.invoke("save", { key: openKey, body: editor.value }).catch(() => {});
  }
});

async function createNote() {
  await flushSave();
  try {
    renderSnapshot(await window.kiln.invoke<Snapshot>("create", {}));
    editor.focus();
    if (isMobile) sidebar.classList.remove("open");
  } catch (error) {
    showStatus(`error: ${(error as Error).message}`);
  }
}

async function deleteNote() {
  if (!openKey) return;
  const sure = window.confirm("Delete this note? The file is removed from disk.");
  if (!sure) return;
  try {
    dirty = false;
    renderSnapshot(await window.kiln.invoke<Snapshot>("delete", { key: openKey }));
  } catch (error) {
    showStatus(`error: ${(error as Error).message}`);
  }
}

(document.getElementById("new") as HTMLElement).addEventListener("click", () => void createNote());
(document.getElementById("delete") as HTMLElement).addEventListener("click", () => void deleteNote());

// Mobile: the sidebar is a drawer, because a 400px-wide window cannot afford a
// permanent column. Desktop: it is always visible, and toggling it would be odd.
if (isMobile) {
  (document.getElementById("menu") as HTMLElement).remove();
  titleLabel.addEventListener("click", () => sidebar.classList.toggle("open"));
} else {
  (document.getElementById("menu") as HTMLElement).remove();
}

// The shell's own menu, wherever it exists: the desktop menu bar and the Android
// PopupMenu both arrive as this event, so there is one handler for both.
window.kiln.listen<{ id: string }>("menu", ({ id }) => {
  if (id === "new") void createNote();
  else if (id === "save") void flushSave();
  else if (id === "delete") void deleteNote();
});

// A save that happened elsewhere, including in the second desktop window: adopt
// the list, but never the body of the note this window is editing.
window.kiln.listen<{ key: string; statistics: Counts; snapshot: Snapshot }>("saved", (event) => {
  if (event.snapshot) renderList(event.snapshot);
  if (event.key === openKey) renderCounts(event.statistics);
});

window.kiln.listen<{ state: string }>("lifecycle", ({ state }) => {
  // Leaving the foreground is exactly when an unsaved draft would be lost.
  if (state === "pause" || state === "stop") void flushSave();
});

window.kiln.listen<string>("location", (directory) => {
  locationLabel.textContent = directory;
  locationLabel.hidden = false;
  locationLabel.title = directory;
});

void (async () => {
  try {
    renderSnapshot(await window.kiln.invoke<Snapshot>("current"));
  } catch (error) {
    showStatus(`error: ${(error as Error).message}`);
  }
})();
