use std::fmt;

/// Everything that can go wrong at the framework boundary.
#[derive(Debug)]
pub enum Error {
    /// No handler was registered for the requested method.
    UnknownMethod(String),
    /// A handler returned an error; the string is shown to the page.
    Handler(String),
    /// The request payload could not be read as the handler's argument type.
    Payload(String),
    /// A handler panicked. The app keeps running; the page gets this message.
    Panic(String),
    /// The webview or event loop failed.
    Runtime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownMethod(m) => write!(f, "unknown command: {m}"),
            Error::Handler(m) => f.write_str(m),
            Error::Payload(m) => write!(f, "invalid payload: {m}"),
            Error::Panic(m) => write!(f, "command panicked: {m}"),
            Error::Runtime(m) => write!(f, "runtime error: {m}"),
        }
    }
}

impl Error {
    /// Machine-readable class of this failure, carried on the wire so a page
    /// branches on the kind instead of matching message text. `unknown` covers
    /// both an unregistered command and an allowlist denial on purpose: the
    /// page must not learn which commands exist but were withheld.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::UnknownMethod(_) => "unknown",
            Error::Handler(_) => "handler",
            Error::Payload(_) => "payload",
            Error::Panic(_) => "panic",
            Error::Runtime(_) => "runtime",
        }
    }
}

impl std::error::Error for Error {}

impl From<wry::Error> for Error {
    fn from(e: wry::Error) -> Self {
        Error::Runtime(e.to_string())
    }
}
