# Errors the page can branch on

A failed call answers an envelope, never a thrown transport error:

```json
{"error": {"message": "invalid payload: missing field `name`", "kind": "payload", "method": "greet"}}
```

The shim turns that into a rejected promise, so ordinary `try`/`catch` works:

```ts
try {
  await window.kiln.invoke("save", { path });
} catch (error) {
  const { kind, method, message } = error as {
    kind: "unknown" | "payload" | "handler" | "panic" | "runtime";
    method: string;
    message: string;
  };
  if (kind === "unknown") hideSaveButton();
  else showToast(`${method}: ${message}`);
}
```

## The five kinds

| `kind` | Raised by | What the page should do |
| --- | --- | --- |
| `unknown` | no handler, or the allowlist withheld it | treat the command as absent; do not retry |
| `payload` | the request did not match the typed argument | a bug in the page; log it |
| `handler` | the handler returned `Err` | show the message; it is meant for a user |
| `panic` | the handler panicked | show the message, keep the window; report it |
| `runtime` | the webview or event loop failed | fatal for the window; offer a restart |

`unknown` covers both a missing command and a denied one on purpose. A page must
not be able to probe which commands exist but were withheld, so the two are
indistinguishable from inside the webview.

## Writing an error

```rust
fn save(payload: &Value) -> Result<Value, kiln::Error> {
    let path = payload
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| kiln::Error::Payload("save needs a \"path\" string".to_owned()))?;
    std::fs::write(path, b"")
        .map_err(|error| kiln::Error::Handler(format!("could not write {path}: {error}")))?;
    Ok(Value::Null)
}
```

Use `Payload` for a malformed request and `Handler` for everything a user might
see. A `Handler` message is rendered verbatim in a toast by most apps, so write it
as a sentence, not a debug string.

## Panics

A panic inside a handler is caught before it crosses the webview's FFI boundary,
where unwinding would abort the process. The page gets `kind: "panic"` and the app
keeps running with its windows and unsaved work intact. The panic is also recorded
in the diagnostics buffer and in `__kiln_reports`, which is why a crash in the
field is diagnosable rather than a blank window.

## Errors that never reach the page

A request dropped by the allowlist check and a bridge message that is not JSON are
recorded in `kiln dev --verbose` instead. They are programming errors in the shell
or the page, not something a user action can cause, and answering them would leak
which commands exist.
