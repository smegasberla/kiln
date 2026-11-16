# Scratch

A notebook that keeps each note as a plain `.md` file, built to exercise the parts
of Kiln that only a real app does.

```sh
kiln dev                  # desktop, hot reload
kiln build --size         # desktop release binary
kiln android build --abi x86_64   # emulators on x86 hosts
kiln android run
```

## Why this app

Every framework decision here is borrowed from a capability the device suite now
proves, rather than from a list of features to show off:

| The app does | Because the shell has |
| --- | --- |
| Saves per keystroke, off the UI thread | `async_command`, so typing never waits on the disk |
| Writes the draft on `pause` and `stop` | lifecycle events, which are the last warning before the OS kills the page |
| Shows the notes directory | `data_dir`, which is per-platform and has to be named rather than guessed |
| Menu with real `Ctrl/Cmd+N` / `S` | desktop menu bar, and the same ids as Android's PopupMenu |
| A drawer on phones, a column on desktop | one shared page, two layouts |
| Tray: new note, show window | a tray on desktop, a notification on Android |

## One command list, two shells

The interesting structural choice: the app itself lives in `native/src/app.rs` and
the Android crate includes that file rather than keeping a second copy.

```rust
// android/src/lib.rs
#[path = "../../native/src/app.rs"]
mod app;

kiln_android::run(app::build("Scratch"))
```

Kiln can check command parity between the two crates, and it does, but a check
that can pass while the lists differ is worth less than a single list that cannot
drift. `kiln android build` still reports `8 commands on both platforms`, which is
now a proof rather than a coincidence.

What stays platform-specific is what is genuinely different: `native/src/main.rs`
adds the menu bar, the tray, and the accelerators; Android gets the same groups as
a PopupMenu the page raises, and the page hides the overflow button on desktop
where the bar is already there.

## Storage

One file per note in the platform data directory, named after the note's key:

```
~/.local/share/Scratch/welcome.md
```

A note's title is its first non-empty line, so there is one source of truth and a
rename rewrites the file rather than storing a second name. Plain markdown rather
than a database, because a notebook whose contents need this app to read is a
worse deal for the user than one that does not.

Verified on the emulator and on Linux; the page is shared, so a fix to either
layout is a fix to both.
