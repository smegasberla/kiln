//! The registration surface an app calls.
//!
//! `add` (inline), `add_async` (worker thread), `add_async_fn` (shared runtime)
//! and `add_typed` (deserialised arguments) all end in the same table, so which
//! one an app picked is invisible to `dispatch` apart from where the work runs.

use serde_json::Value;

#[cfg(feature = "rt")]
use super::envelope::ok_or_error;
use super::handler::{decode_args, Handler};
use crate::{AppHandle, Error};

impl super::registry::Registry {
    /// Register a handler that runs on the UI thread. Registering the same name
    /// twice replaces it.
    pub fn add<F>(&mut self, method: &str, handler: F)
    where
        F: Fn(&AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.insert_handler(method, Handler::Sync(Box::new(handler)));
    }

    /// The one place a handler enters the table.
    ///
    /// Registration is last-wins in a flat namespace, so a name claimed twice is
    /// silent data loss: the first handler's command stops existing and the
    /// second answers calls meant for it. Two plugins that both claim `open` is
    /// the realistic case, and nothing else reports it, not even [`Self::callable`]
    /// since only the survivor is left in the map.
    fn insert_handler(&mut self, method: &str, handler: Handler) {
        if self.handlers.contains_key(method) {
            crate::diagnostics::record(
                crate::diagnostics::Kind::App,
                "command registered twice",
                serde_json::json!({ "command": method }),
            );
        }
        self.handlers.insert(method.to_owned(), handler);
    }

    /// Register a handler that runs on a worker thread.
    ///
    /// Use this for anything touching the disk, the network, or that can take
    /// longer than a frame: the reply arrives when the work is done and the
    /// window keeps painting meanwhile.
    pub fn add_async<F>(&mut self, method: &str, handler: F)
    where
        F: Fn(AppHandle, Value) -> Result<Value, Error> + Send + Sync + 'static,
    {
        self.insert_handler(method, Handler::Async(std::sync::Arc::new(handler)));
    }

    /// Register a command whose argument and reply are concrete types.
    ///
    /// The payload is deserialized before the handler runs, so a wrong shape
    /// becomes an [`Error::Payload`] the page can branch on instead of a
    /// handler string-match. The reply is serialized back into the same
    /// envelope, so the wire format does not change.
    pub fn add_typed<P, R, F>(&mut self, method: &str, handler: F)
    where
        P: crate::serde::de::DeserializeOwned + Send + 'static,
        R: crate::serde::Serialize + 'static,
        F: Fn(&AppHandle, P) -> Result<R, Error> + Send + Sync + 'static,
    {
        let handler = std::sync::Arc::new(handler);
        self.add(method, move |handle, payload| {
            let args: P = decode_args(payload)?;
            let reply = handler(handle, args)?;
            serde_json::to_value(reply).map_err(|error| Error::Handler(error.to_string()))
        });
    }

    /// Register a real `async fn` command. Requires the `rt` feature.
    ///
    /// The payload is deserialized on the calling thread, so a malformed
    /// argument is answered immediately; the future itself runs on the shared
    /// runtime, where a thousand in-flight requests cost a thousand cheap tasks
    /// rather than a thousand stacks. The reply goes through the same envelope
    /// as every other handler.
    ///
    /// A panic inside the future is caught and reported as `kind: "panic"`
    /// instead of killing the worker task, which would otherwise leave the page
    /// awaiting a reply forever.
    #[cfg(feature = "rt")]
    pub fn add_async_fn<P, R, F, Fut>(&mut self, method: &str, handler: F)
    where
        P: crate::serde::de::DeserializeOwned + Send + 'static,
        R: crate::serde::Serialize + 'static,
        F: Fn(AppHandle, P) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    {
        let handler = std::sync::Arc::new(handler);
        let method = method.to_owned();
        let name = method.clone();
        self.insert_handler(
            &name,
            Handler::Deferred(Box::new(move |handle, payload, done| {
                let args = match decode_args::<P>(payload) {
                    Ok(args) => args,
                    Err(error) => return done(ok_or_error(&method, Err(error))),
                };
                let handler = std::sync::Arc::clone(&handler);
                let method = method.clone();
                crate::rt::spawn(async move {
                    let result = match crate::rt::isolate(handler(handle, args)).await {
                        Ok(result) => result,
                        Err(message) => Err(Error::Panic(message)),
                    };
                    let result = result.and_then(|reply| {
                        serde_json::to_value(reply)
                            .map_err(|error| Error::Handler(error.to_string()))
                    });
                    done(ok_or_error(&method, result));
                });
            })),
        );
    }

    /// Names that can actually be called, sorted, for diagnostics.
    pub fn callable(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .handlers
            .keys()
            .filter(|name| self.permitted(name))
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// Whether `method` is registered and allowed.
    pub fn has(&self, method: &str) -> bool {
        self.permitted(method) && self.handlers.contains_key(method)
    }

    pub(super) fn permitted(&self, method: &str) -> bool {
        match &self.allow {
            Some(allow) => allow.iter().any(|a| a == method),
            None => true,
        }
    }
}
