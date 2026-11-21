//! A deterministic stand-in for `cargo fuzz`.
//!
//! `fuzz/` holds the libFuzzer harness for long runs, but it needs nightly and
//! a nightly-only dependency, so it cannot run on every push. This module is
//! the part that always runs: a fixed-seed mutational loop over the three
//! parsers that take untrusted input. It is not a substitute for coverage
//! guidance, it is the cheap version that would have caught the multi-byte
//! `%XX` panic in `method_from_path` on the first iteration.
//!
//! Determinism is the point: a failure reproduces with the same seed on any
//! machine, so a CI failure is actionable rather than "try again".

use super::*;

use serde_json::Value;

use crate::AppHandle;

/// Small, fast, and platform-independent: xorshift64 rather than anything
/// drawing from the OS, so a seed means the same corpus everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// Byte values chosen to reach the interesting edges, not a uniform spread:
/// escapes, sign bytes, quotes, braces, NUL, and multi-byte UTF-8 starts.
const ALPHABET: &[u8] = b"%{}[]\":,null0123456789abcXYZ \t\n/\\\x00\xc3\xa9\xe2\x82\xac";

fn corpus(rng: &mut Rng) -> Vec<u8> {
    let len = rng.below(48);
    (0..len).map(|_| ALPHABET[rng.below(ALPHABET.len())]).collect()
}

#[test]
fn the_path_decoder_never_panics_and_never_invents_a_command() {
    let mut rng = Rng(0x5eed_1234);
    for _ in 0..20_000 {
        let raw = corpus(&mut rng);
        let Ok(text) = std::str::from_utf8(&raw) else {
            continue;
        };
        let path = format!("/__kiln/{text}");
        // The property is total: any input, an `Option`, never a panic.
        if let Some(method) = method_from_path(&path) {
            assert!(!method.is_empty(), "an empty method is not a command");
            // Idempotence holds only when there is nothing left to decode:
            // a name that still contains `%` is re-interpreted on the next
            // pass, which is correct (the escape is real) and not a function
            // of the decoder being sloppy. None of these may panic either way.
            if !method.contains('%') {
                assert_eq!(
                    method_from_path(&format!("/__kiln/{method}")),
                    Some(method),
                    "a name with no escapes must decode to itself"
                );
            }
        }
    }
}

#[test]
fn the_payload_parser_accepts_or_refuses_but_never_panics() {
    let mut rng = Rng(0xdec0_de01);
    for _ in 0..20_000 {
        let raw = corpus(&mut rng);
        match crate::transport::parse_payload(&raw) {
            Ok(value) => {
                // Whatever it parsed as must re-serialize to valid JSON, or
                // the reply built from it would break the page's parser.
                assert!(serde_json::to_string(&value).is_ok());
            }
            Err(message) => assert!(!message.is_empty(), "a refusal must say why"),
        }
    }
}

#[test]
fn the_bridge_parser_accepts_or_refuses_but_never_panics() {
    let mut rng = Rng(0x0110_1010);
    for _ in 0..20_000 {
        let raw = corpus(&mut rng);
        let Ok(text) = std::str::from_utf8(&raw) else {
            continue;
        };
        match crate::transport::parse_message(text) {
            Ok((method, _payload)) => {
                assert!(!method.is_empty(), "an empty method is not a command")
            }
            Err(message) => assert!(!message.is_empty(), "a refusal must say why"),
        }
    }
}

#[test]
fn a_decoded_path_that_reaches_the_registry_always_gets_an_envelope() {
    // The whole path, not just the parser: junk in, a JSON envelope out.
    let mut reg = Registry::new();
    reg.add("greet", |_h, _p| Ok(serde_json::json!("hi")));
    let handle = AppHandle::detached_for_tests();
    let mut rng = Rng(0xfeed_face);
    for _ in 0..2_000 {
        let raw = corpus(&mut rng);
        let Ok(text) = std::str::from_utf8(&raw) else {
            continue;
        };
        let path = format!("/__kiln/{text}");
        let Some(method) = method_from_path(&path) else {
            continue;
        };
        let body = reg.dispatch(&handle, &method, Value::Null);
        assert!(
            body.starts_with("{\"ok\"") || body.starts_with("{\"error\""),
            "not an envelope for {method:?}: {body}"
        );
    }
}
