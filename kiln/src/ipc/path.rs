//! The endpoint path decoder.
//!
//! This is the first thing untrusted input touches: a page-controlled URL becomes
//! a command name here, before the allowlist is consulted. It is therefore small
//! and byte-oriented on purpose.

use super::ENDPOINT_PREFIX;

/// Extract the method name from an endpoint path, decoding `%XX` escapes.
///
/// Returns `None` for anything outside the endpoint namespace so the caller can
/// fall through to static asset serving, and for a path that is not valid UTF-8
/// after decoding: no command can have such a name, and the alternative is
/// passing a lossy string to the registry.
///
/// Decoding works on bytes, not on the `&str`. Slicing the string by index looks
/// equivalent for ASCII and panics on anything else, because a byte offset can
/// land inside a multi-byte character: `/__kiln/%€` used to abort the handler
/// thread from inside the request path.
pub fn method_from_path(path: &str) -> Option<String> {
    let raw = path.strip_prefix(ENDPOINT_PREFIX)?;
    if raw.is_empty() {
        return None;
    }
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // `%XX` needs two bytes after the sign; a trailing `%4` is a literal.
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// One hex digit, in either case.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
