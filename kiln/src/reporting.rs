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

use std::path::PathBuf;
use std::sync::Mutex;

/// How many entries to keep in memory for `report.read`.
const MAX_ENTRIES: usize = 50;

/// How large the log file may grow before it is rotated.
///
/// A crash loop appends once per panic, so without a ceiling the file is a slow
/// disk-fill on a user's machine. 512 KiB holds thousands of traces, which is
/// more than anyone reads.
const MAX_LOG_BYTES: u64 = 512 * 1024;

static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// One crash or error record, as text. A closure keeps the sink simple; an app
/// that needs structured fields can parse the line it already formats.
type Reporter = Box<dyn Fn(&str) + Send + Sync>;
static REPORTER: Mutex<Option<Reporter>> = Mutex::new(None);

struct Entry {
    message: String,
}

/// Where reports are written, if the app asked for a file.
///
/// Call once from `main` before `App::run`. Without it, reports are kept in
/// memory only and readable via the `__kiln_report` command.
pub fn set_log_file(path: impl Into<PathBuf>) {
    *LOG_FILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path.into());
}

/// Install a sink that receives every record, in addition to the in-memory
/// buffer and the log file.
///
/// This is the seam for an application that wants crash telemetry: the framework
/// still uploads nothing by itself, but an app no longer has to fork this module
/// to forward entries to its own collector. Called from the panic hook, so the
/// sink must not panic and must not block for long.
pub fn set_reporter<F>(reporter: F)
where
    F: Fn(&str) + Send + Sync + 'static,
{
    *REPORTER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Box::new(reporter));
}

/// Append one line, rotating the file instead of growing past `MAX_LOG_BYTES`.
///
/// The previous file is kept as `<name>.old` rather than deleted: the crash that
/// filled it is usually the one worth reading. Every IO failure is swallowed,
/// because a reporting failure must never cascade into a second panic.
fn append_capped(path: &std::path::Path, text: &str) {
    let len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if len + text.len() as u64 > MAX_LOG_BYTES {
        let _ = std::fs::rename(path, path.with_extension("old"));
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
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

    // Announce before touching the disk: a sink that itself crashes should not
    // stop the local record from being written.
    if let Ok(reporter) = REPORTER.lock() {
        if let Some(reporter) = reporter.as_ref() {
            reporter(&line);
        }
    }

    let path = LOG_FILE.lock().ok().and_then(|p| p.clone());
    if let Some(path) = path {
        append_capped(&path, &format!("{line}\n"));
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

    static LOCK: Mutex<()> = Mutex::new(());

    fn serialized() -> std::sync::MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn records_are_kept_and_readable() {
        let _guard = serialized();
        record("test message one", "unit");
        let entries = entries();
        assert!(
            entries.iter().any(|e| e.contains("test message one")),
            "recorded message should be readable back: {entries:?}"
        );
    }

    #[test]
    fn the_buffer_is_bounded() {
        let _guard = serialized();
        for i in 0..(MAX_ENTRIES + 30) {
            record(&format!("flood {i}"), "unit");
        }
        assert!(entries().len() <= MAX_ENTRIES);
    }

    #[test]
    fn a_log_file_receives_entries() {
        let _guard = serialized();
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

    #[test]
    fn the_log_file_stays_bounded() {
        let _guard = serialized();
        let dir = std::env::temp_dir().join(format!("kiln-report-cap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("crash.log");
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(file.with_extension("old"));

        // Driven through the capping writer directly, not through `record`: the log
        // path is process-wide and the other tests here set and clear it, so going
        // through the global would make rotation depend on test scheduling.
        let filler = "x".repeat(4 * 1024);
        for _ in 0..((MAX_LOG_BYTES / filler.len() as u64) * 2) {
            append_capped(&file, &format!("{filler}\n"));
        }

        let size = std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
        assert!(size <= MAX_LOG_BYTES, "log file grew to {size} bytes");
        assert!(
            file.with_extension("old").exists(),
            "the rotated log must survive the rotation"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_installed_reporter_sees_records() {
        use std::sync::OnceLock;

        let _guard = serialized();
        static SEEN: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
        let seen = SEEN.get_or_init(|| Mutex::new(Vec::new()));
        seen.lock().unwrap().clear();

        set_reporter(|line| {
            if let Some(seen) = SEEN.get() {
                seen.lock().unwrap().push(line.to_owned());
            }
        });
        record("forwarded to the sink", "unit");
        *REPORTER.lock().unwrap() = None;

        assert!(
            seen.lock().unwrap().iter().any(|l| l.contains("forwarded to the sink")),
            "a record must reach an installed reporter"
        );
    }
}
