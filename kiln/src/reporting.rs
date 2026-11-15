//! Crash and error reporting.
//!
//! Scope is deliberately narrow: Kiln records what went wrong to a file next to
//! the app's own data and can hand a summary back to the page. It does not phone
//! home. Sending a crash report somewhere is an app decision, not a framework
//! default, and a desktop framework that silently uploads on crash is a liability.
//!
//! A panic hook is installed so panics anywhere in the app (not just in command
//! handlers) leave a record, and the same file can be read back through the
//! built-in `__kiln_report` command.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

/// How many entries to keep in memory for `report.read`.
const MAX_ENTRIES: usize = 50;

static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

struct Entry {
    message: String,
}

/// Where reports are written, if the app asked for a file.
///
/// Call once from `main` before `App::run`. Without it, reports are kept in
/// memory only and readable via the `__kiln_report` command.
pub fn set_log_file(path: impl Into<PathBuf>) {
    *LOG_FILE.lock().unwrap() = Some(path.into());
}

/// Record a message. Called by the panic hook and by the IPC layer.
pub fn record(message: &str, context: &str) {
    let line = format!("[{context}] {message}");

    if let Ok(mut entries) = ENTRIES.lock() {
        if entries.len() == MAX_ENTRIES {
            entries.remove(0);
        }
        entries.push(Entry { message: line.clone() });
    }

    eprintln!("kiln: {line}");

    let path = LOG_FILE.lock().ok().and_then(|p| p.clone());
    if let Some(path) = path {
        let mut text = String::new();
        // Append with a trailing newline; ignore IO failures, since a reporting
        // failure must never cascade into a second panic.
        let _ = writeln!(text, "{line}");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
    }
}

/// The most recent entries, oldest first.
pub fn entries() -> Vec<String> {
    ENTRIES
        .lock()
        .map(|entries| entries.iter().map(|e| e.message.clone()).collect())
        .unwrap_or_default()
}

/// Install a hook that records panics instead of relying on stderr being visible.
///
/// A desktop app launched from a file manager has nowhere to print, so without
/// this a crash leaves no trace at all.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_owned());
        let message = payload_text(info.payload());
        record(&format!("{message} at {location}"), "panic");
        previous(info);
    }));
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_kept_and_readable() {
        record("test message one", "unit");
        let entries = entries();
        assert!(
            entries.iter().any(|e| e.contains("test message one")),
            "recorded message should be readable back: {entries:?}"
        );
    }

    #[test]
    fn the_buffer_is_bounded() {
        for i in 0..(MAX_ENTRIES + 30) {
            record(&format!("flood {i}"), "unit");
        }
        assert!(entries().len() <= MAX_ENTRIES);
    }

    #[test]
    fn a_log_file_receives_entries() {
        let dir = std::env::temp_dir().join(format!("kiln-report-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("crash.log");
        let _ = std::fs::remove_file(&file);

        set_log_file(&file);
        record("written to disk", "unit");

        let text = std::fs::read_to_string(&file).expect("log file should exist");
        assert!(text.contains("written to disk"), "{text}");

        // Detach so other tests are not affected by a shared log path.
        *LOG_FILE.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
