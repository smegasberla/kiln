//! A JSON-file key/value store in the platform data directory. Requires the
//! `storage` feature.
//!
//! The scope is deliberately small: a page needs to remember a setting across
//! launches, and every app was writing the same "one JSON file next to the
//! binary" code to do it. This is that file, done once, with an atomic write so
//! a crash mid-save cannot truncate the data to nothing.
//!
//! ```text
//! file:   <data dir>/<app name>/storage.json
//! shape:  {"theme":"dark","window":{"w":800}}
//! ```
//!
//! Reads and writes go through the command registry, so a page reaches the
//! store with `voussoir.invoke("storage.get", {key})` — no new transport, and the
//! same allowlist and per-window gating as everything else.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value};

use crate::{AppHandle, Error};

/// How many bytes the store may reach before a write is refused.
///
/// A settings file that grows past this is a bug in the app (usually a key
/// derived from user input), and silently filling the user's disk is worse than
/// an error the page can show.
pub const SIZE_CAP: usize = 4 * 1024 * 1024;

/// The store as it lives on disk: one flat map of key to JSON value.
///
/// Keys are dots-free by design, so `"a.b"` is a literal key rather than a path
/// into a nested object. Nesting belongs in the value the app chooses to store.
pub struct Store {
    path: PathBuf,
}

impl Store {
    /// Open the store for `app_name` in the platform data directory.
    ///
    /// Returns an error, never a silently empty store, when the directory
    /// cannot be determined or created: the page has to know that a `set` will
    /// not survive a restart.
    pub fn open(app_name: &str) -> Result<Self, Error> {
        let dir = crate::platform::data_dir(app_name).ok_or_else(|| {
            Error::Runtime("no writable data directory on this platform".to_owned())
        })?;
        Ok(Self {
            path: dir.join("storage.json"),
        })
    }

    /// Open a store at an explicit path. Tests use this; apps should not need
    /// to, because the data directory is what makes the data follow the user.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where this store persists, for diagnostics and `storage.path`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the whole map. A missing file is an empty store; a corrupt one is
    /// an error the caller sees rather than being silently reset.
    pub fn read(&self) -> Result<BTreeMap<String, Value>, Error> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new())
            }
            Err(error) => return Err(Error::Runtime(format!("storage read failed: {error}"))),
        };
        if bytes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| Error::Runtime(format!("storage is not valid JSON: {error}")))?;
        // Accept a top-level object only. Anything else is corruption, and
        // guessing what was meant would lose the rest of the file.
        let Value::Object(map) = value else {
            return Err(Error::Runtime("storage must be a JSON object".to_owned()));
        };
        Ok(map.into_iter().collect())
    }

    /// Replace the whole map, atomically.
    ///
    /// Writes to `storage.json.tmp` in the same directory and renames over the
    /// target: a rename inside one filesystem is atomic, so a crash leaves
    /// either the old file or the new one, never a half-written one.
    pub fn write(&self, map: &BTreeMap<String, Value>) -> Result<(), Error> {
        let object: Map<String, Value> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let body = serde_json::to_vec_pretty(&Value::Object(object))
            .map_err(|error| Error::Handler(format!("storage is not encodable: {error}")))?;
        if body.len() > SIZE_CAP {
            return Err(Error::Handler(format!(
                "storage would reach {} bytes, over the {SIZE_CAP} byte cap",
                body.len()
            )));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| Error::Runtime("storage path has no parent".to_owned()))?;
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::Runtime(format!("storage directory: {error}")))?;
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp)
                .map_err(|error| Error::Runtime(format!("storage write failed: {error}")))?;
            file.write_all(&body)
                .and_then(|()| file.sync_all())
                .map_err(|error| Error::Runtime(format!("storage write failed: {error}")))?;
        }
        std::fs::rename(&tmp, &self.path)
            .map_err(|error| Error::Runtime(format!("storage replace failed: {error}")))
    }

    /// One key, or `null` when it was never set.
    pub fn get(&self, key: &str) -> Result<Value, Error> {
        Ok(self.read()?.remove(key).unwrap_or(Value::Null))
    }

    /// Set one key. Returns the previous value, or `null`.
    pub fn set(&self, key: &str, value: Value) -> Result<Value, Error> {
        let mut map = self.read()?;
        let previous = map.insert(key.to_owned(), value).unwrap_or(Value::Null);
        self.write(&map)?;
        Ok(previous)
    }

    /// Remove one key. Returns whether it existed.
    pub fn delete(&self, key: &str) -> Result<bool, Error> {
        let mut map = self.read()?;
        let existed = map.remove(key).is_some();
        if existed {
            self.write(&map)?;
        }
        Ok(existed)
    }

    /// Every key, sorted. The values are deliberately not included: a frontend
    /// that wants one calls `get`, so a large store never crosses the bridge in
    /// full.
    pub fn keys(&self) -> Result<Vec<String>, Error> {
        Ok(self.read()?.into_keys().collect())
    }
}

/// The store the registry's commands use, created on first call.
///
/// A `Mutex` rather than a lock-free map because the lock is only taken for the
/// length of one file read or write, and serializing writers is what keeps the
/// last write from winning by accident.
pub struct Shared {
    app_name: String,
    store: Mutex<Option<&'static Store>>,
}

impl Shared {
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            store: Mutex::new(None),
        }
    }

    /// The store for this app, opening it once and remembering the result so a
    /// failed open is not retried on every command.
    fn store(&self) -> Result<&'static Store, Error> {
        let mut slot = self
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(store) = *slot {
            return Ok(store);
        }
        let store: &'static Store = Box::leak(Box::new(Store::open(&self.app_name)?));
        *slot = Some(store);
        Ok(store)
    }

    /// `storage.get` — `{key}` in, the value out.
    pub fn get(&self, payload: &Value) -> Result<Value, Error> {
        let key = key_from(payload)?;
        self.store()?.get(&key)
    }

    /// `storage.set` — `{key, value}` in, the previous value out.
    pub fn set(&self, payload: &Value) -> Result<Value, Error> {
        let key = key_from(payload)?;
        let value = payload
            .get("value")
            .cloned()
            .ok_or_else(|| Error::Payload("storage.set needs a `value` field".to_owned()))?;
        self.store()?.set(&key, value)
    }

    /// `storage.delete` — `{key}` in, whether it existed out.
    pub fn delete(&self, payload: &Value) -> Result<Value, Error> {
        let key = key_from(payload)?;
        Ok(Value::Bool(self.store()?.delete(&key)?))
    }

    /// `storage.keys` — every key, sorted.
    pub fn keys(&self, _payload: &Value) -> Result<Value, Error> {
        Ok(serde_json::json!(self.store()?.keys()?))
    }

    /// `storage.path` — where the file lives, so an app can show it in a
    /// settings dialog or back it up.
    pub fn path(&self, _payload: &Value) -> Result<Value, Error> {
        let path = self
            .store()?
            .path()
            .to_str()
            .map(str::to_owned)
            .map(Value::String)
            .unwrap_or(Value::Null);
        Ok(serde_json::json!({ "path": path }))
    }
}

fn key_from(payload: &Value) -> Result<String, Error> {
    payload
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Error::Payload("storage commands need a non-empty `key`".to_owned()))
}

/// Register the `storage.*` commands on `registry`.
pub fn register(registry: &mut crate::Registry, config: &crate::Config) {
    let shared = std::sync::Arc::new(Shared::new(config.name.clone()));
    let get = std::sync::Arc::clone(&shared);
    registry.add_typed(
        "storage.get",
        move |_app: &AppHandle, args: crate::storage::KeyArgs| {
            get.get(&serde_json::to_value(args).unwrap_or(Value::Null))
        },
    );
    let set = std::sync::Arc::clone(&shared);
    registry.add_typed("storage.set", move |_app: &AppHandle, args: SetArgs| {
        set.set(&serde_json::to_value(args).unwrap_or(Value::Null))
    });
    let delete = std::sync::Arc::clone(&shared);
    registry.add_typed("storage.delete", move |_app: &AppHandle, args: KeyArgs| {
        delete.delete(&serde_json::to_value(args).unwrap_or(Value::Null))
    });
    let keys = std::sync::Arc::clone(&shared);
    registry.add_typed("storage.keys", move |_app: &AppHandle, _args: Value| {
        keys.keys(&Value::Null)
    });
    let path = std::sync::Arc::clone(&shared);
    registry.add_typed("storage.path", move |_app: &AppHandle, _args: Value| {
        path.path(&Value::Null)
    });
}

/// `{key}` for the commands that take only a key.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct KeyArgs {
    pub key: String,
}

/// `{key, value}` for `storage.set`.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SetArgs {
    pub key: String,
    pub value: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_store(name: &str) -> Store {
        let dir =
            std::env::temp_dir().join(format!("voussoir-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store::at(dir.join("storage.json"))
    }

    #[test]
    fn a_missing_file_reads_as_empty() {
        let store = temp_store("missing");
        assert!(store.read().unwrap().is_empty());
        assert_eq!(store.get("nope").unwrap(), Value::Null);
    }

    #[test]
    fn set_get_delete_round_trip() {
        let store = temp_store("roundtrip");
        assert_eq!(store.set("theme", json!("dark")).unwrap(), Value::Null);
        assert_eq!(store.get("theme").unwrap(), json!("dark"));
        assert_eq!(store.set("theme", json!("light")).unwrap(), json!("dark"));
        assert_eq!(store.get("theme").unwrap(), json!("light"));
        assert!(store.delete("theme").unwrap());
        assert!(!store.delete("theme").unwrap());
        assert_eq!(store.get("theme").unwrap(), Value::Null);
    }

    #[test]
    fn values_survive_a_reopen() {
        let store = temp_store("reopen");
        store.set("window", json!({ "w": 800, "h": 600 })).unwrap();
        let reopened = Store::at(store.path());
        assert_eq!(
            reopened.get("window").unwrap(),
            json!({ "w": 800, "h": 600 })
        );
    }

    #[test]
    fn keys_are_sorted_and_complete() {
        let store = temp_store("keys");
        store.set("b", json!(1)).unwrap();
        store.set("a", json!(2)).unwrap();
        assert_eq!(store.keys().unwrap(), vec!["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn corrupt_storage_is_reported_not_reset() {
        let store = temp_store("corrupt");
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), b"{not json").unwrap();
        assert!(store.read().is_err());
        assert!(store.get("a").is_err());
        // The broken file is still there: reporting beats deleting user data.
        assert!(store.path().exists());
    }

    #[test]
    fn a_non_object_top_level_is_a_corrupt_store() {
        let store = temp_store("array");
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), b"[1,2,3]").unwrap();
        assert!(store.read().is_err());
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file() {
        let store = temp_store("atomic");
        store.set("a", json!(1)).unwrap();
        store.set("a", json!(2)).unwrap();
        assert!(!store.path().with_extension("json.tmp").exists());
        assert_eq!(store.get("a").unwrap(), json!(2));
    }

    #[test]
    fn an_oversized_value_is_refused_without_truncating_the_file() {
        let store = temp_store("cap");
        store.set("keep", json!("small")).unwrap();
        let huge = "x".repeat(SIZE_CAP + 1);
        let error = store.set("huge", json!(huge)).unwrap_err();
        assert!(error.to_string().contains("cap"), "{error}");
        // The previous content is untouched.
        assert_eq!(store.get("keep").unwrap(), json!("small"));
    }

    #[test]
    fn commands_reject_a_bad_payload_with_the_payload_kind() {
        let store = temp_store("payload");
        let shared = Shared {
            app_name: "test".to_owned(),
            store: Mutex::new(Some(Box::leak(Box::new(store)))),
        };
        let error = shared.get(&json!({})).unwrap_err();
        assert_eq!(error.kind(), "payload");
        let error = shared.set(&json!({ "key": "a" })).unwrap_err();
        assert_eq!(error.kind(), "payload");
        // An empty key is refused too, so `""` cannot become a real entry.
        let error = shared.get(&json!({ "key": "" })).unwrap_err();
        assert_eq!(error.kind(), "payload");
    }

    #[test]
    fn commands_round_trip_through_the_shared_store() {
        let store = temp_store("shared");
        let shared = Shared {
            app_name: "test".to_owned(),
            store: Mutex::new(Some(Box::leak(Box::new(store)))),
        };
        assert_eq!(
            shared.set(&json!({ "key": "a", "value": 1 })).unwrap(),
            Value::Null
        );
        assert_eq!(shared.get(&json!({ "key": "a" })).unwrap(), json!(1));
        assert_eq!(shared.keys(&Value::Null).unwrap(), json!(["a"]));
        assert_eq!(shared.delete(&json!({ "key": "a" })).unwrap(), json!(true));
        let path = shared.path(&Value::Null).unwrap();
        assert!(
            path["path"].as_str().unwrap().ends_with("storage.json"),
            "{path}"
        );
    }
}
