//! The shared async runtime behind `async_fn` commands. Requires the `rt`
//! feature.
//!
//! One multi-threaded runtime for the whole process, created on first use.
//! A future submitted here costs a task, not a thread: the thread-per-command
//! path ([`crate::Registry::add_async`]) still exists for handlers that only
//! need to get off the UI thread, but anything that awaits belongs here.
//!
//! Workers outlive the windows on purpose. A request that is mid-flight when
//! the last window closes has nowhere to reply to, but a panicking teardown
//! would abort the process from inside a worker; letting the task finish and
//! dropping its reply is the quiet failure.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;

use futures_util::FutureExt;

/// The process-wide runtime, built on first use.
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// How many worker threads to start.
///
/// One per core keeps a slow handler from starving the others, and
/// `available_parallelism` is already capped by the OS (cgroups included) where
/// it matters. The floor of two is deliberate: a single worker plus a blocking
/// future is a deadlock waiting for the first timeout to be added.
fn worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .max(2)
}

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads())
            .thread_name("voussoir-rt")
            .enable_all()
            .build()
            // Building a multi-thread runtime against the OS thread API has no
            // recoverable failure here: if it cannot be created the app has no
            // way to run an `async_fn` command at all, and a lazy panic with this
            // message is a better report than a command that silently never
            // answers. It is the one place in the crate where that trade is right.
            .expect("voussoir: could not build the async runtime")
    })
}

/// Run a future to completion on this thread, driven by the shared runtime.
///
/// Used where a synchronous caller has to wait for an async handler, so the
/// future still gets a real reactor instead of being polled by hand.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    runtime().block_on(future)
}

/// Submit a future and stop tracking it.
///
/// The task runs on a worker thread; its output is dropped if nobody awaits it,
/// which is what the reply-carrying futures here do themselves (they call their
/// completion callback internally).
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    runtime().spawn(future);
}

/// Sleep for a duration on the shared runtime.
///
/// Re-exported so a handler can await a delay without depending on a particular
/// async runtime: which one backs `async_fn` is `voussoir`'s choice, not an app's, and
/// hard-coding `tokio::time` in an app couples it to that choice.
pub async fn sleep(duration: std::time::Duration) {
    tokio::time::sleep(duration).await;
}

/// `true` once the runtime has been built. Tests use it to prove that a
/// synchronous app never pays for the runtime.
pub fn started() -> bool {
    RUNTIME.get().is_some()
}

/// Run a future, turning a panic into the message a handler would report.
///
/// Without this a panic in an async command would poison the worker task: the
/// page would wait forever for a reply that can never arrive. The registry
/// converts the `Err` into the same `panic` envelope a sync handler produces.
pub async fn isolate<F>(future: F) -> Result<F::Output, String>
where
    F: Future,
{
    match AssertUnwindSafe(future).catch_unwind().await {
        Ok(output) => Ok(output),
        Err(payload) => Err(crate::ipc::panic_message(payload.as_ref())),
    }
}
