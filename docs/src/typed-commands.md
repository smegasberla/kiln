# Typed commands

`Value` in, `Value` out is the wire format, but it is a poor API: the handler has
to re-check every field, and a wrong shape becomes a string the page cannot act
on. `command_typed` moves the deserialization in front of the handler and answers a
malformed request with `kind: "payload"` naming the command.

## A typed command

```rust
use voussoir::serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct EchoArgs {
    text: String,
    repeat: Option<u32>,
}

#[derive(Serialize)]
struct EchoReply {
    text: String,
    count: u32,
}

App::new().command_typed("echo", |_app: &AppHandle, args: EchoArgs| {
    let count = args.repeat.unwrap_or(1);
    Ok(EchoReply {
        text: args.text.repeat(count as usize),
        count,
    })
})
```

The page sees no difference: `invoke("echo", { text: "hi", repeat: 2 })` resolves
to `{"text": "hihi", "count": 2}`. What changed is the failure path. Send
`repeat: "two"` and the handler never runs; the page gets:

```json
{"error": {"message": "...", "kind": "payload", "method": "echo"}}
```

## Why this matters on the wire

The envelope does not change, which is deliberate: a typed command and a plain one
are indistinguishable to the page, so converting a command cannot break a client
that already calls it. What does change is that a `payload` error now means "the
request shape was wrong" rather than "the handler decided to say no".

`verify.ts` asserts both halves over the real transport: a bad shape comes back as
`payload/typed_echo`, and the same command with the right shape answers.

## Types across the FFI boundary

The command crate is a `cdylib` in development. Rust types do not cross that
boundary, so the table itself is a fixed C-compatible shape and the *payloads* are
JSON. That is why commands in the table take `&Value`: the typed wrapper is
generated in the shell, where the concrete type is visible to both sides.

Practically, this means typed commands live on `App` in `main.rs`, not in the
hot-reloaded table. A command that must be both typed and hot-reloadable is two
commands: the table one takes `Value`, and a thin typed wrapper in `main.rs`
validates before delegating.

## Async typed commands

```rust
App::new().async_fn("lookup", |_app, args: LookupArgs| async move {
    let row = db.get(args.id).await?;
    Ok(row)
})
```

The argument is deserialized on the calling thread, so a malformed request is
answered before a task is spawned; only the work moves to the runtime.

## Guidance

- Type the argument when the page is yours. It is a few lines and it turns a class
  of bug into a wire error.
- Leave `Value` for genuinely dynamic payloads, such as a plugin registry or a
  command that forwards another app's request.
- Do not validate business rules in the type. A `payload` error should mean the
  request was malformed, not that the user is not allowed to do this; that is a
  `handler` error with a message the page can show.
