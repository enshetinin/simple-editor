//! Secret variable values.
//!
//! By default secrets live only in memory for the session. When the user
//! opts in ("Remember"), the value is stored in the OS credential store
//! (macOS Keychain, Windows Credential Manager). If that store fails, the
//! value stays in session memory and the user is told; there is **never** a
//! fallback to plain-text files.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// Where remembered secrets are kept.
pub trait SecretBackend: Send + Sync {
    fn name(&self) -> &str;
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

pub const SERVICE: &str = "SimpleEditor";

/// Stable credential key for a variable of a project scope.
pub fn secret_key(project_root: &Path, scope: &str, variable: &str) -> String {
    format!("{}|{scope}|{variable}", project_root.display())
}

/// In-memory backend (tests).
#[derive(Default)]
pub struct MemoryBackend {
    values: Mutex<HashMap<String, String>>,
}

impl SecretBackend for MemoryBackend {
    fn name(&self) -> &str {
        "memory"
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self
            .values
            .lock()
            .map_err(|e| e.to_string())?
            .get(key)
            .cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.values
            .lock()
            .map_err(|e| e.to_string())?
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.values.lock().map_err(|e| e.to_string())?.remove(key);
        Ok(())
    }
}

/// Backend that always fails (no OS store available).
pub struct Unavailable(pub String);

impl SecretBackend for Unavailable {
    fn name(&self) -> &str {
        "unavailable"
    }
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Err(self.0.clone())
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err(self.0.clone())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err(self.0.clone())
    }
}

/// The OS credential store through `keyring-core`.
pub struct OsStore;

fn init_os_store() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            let store =
                apple_native_keyring_store::keychain::Store::new().map_err(|e| e.to_string())?;
            keyring_core::set_default_store(store);
            Ok(())
        }
        #[cfg(windows)]
        {
            let store = windows_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
            keyring_core::set_default_store(store);
            Ok(())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Err("no supported credential store on this platform".to_owned())
        }
    })
    .clone()
}

impl OsStore {
    fn entry(key: &str) -> Result<keyring_core::Entry, String> {
        init_os_store()?;
        keyring_core::Entry::new(SERVICE, key).map_err(|e| e.to_string())
    }
}

impl SecretBackend for OsStore {
    fn name(&self) -> &str {
        if cfg!(target_os = "macos") {
            "macOS Keychain"
        } else if cfg!(windows) {
            "Windows Credential Manager"
        } else {
            "system credential store"
        }
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match Self::entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The backend the app uses. Tests set `SE_SECRET_BACKEND=memory` so they
/// never touch the user's real credential store.
pub fn default_backend() -> Arc<dyn SecretBackend> {
    if std::env::var_os("SE_SECRET_BACKEND").is_some_and(|v| v == "memory") {
        return Arc::new(MemoryBackend::default());
    }
    Arc::new(OsStore)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_backend_round_trip_and_unavailable_errors() {
        let m = MemoryBackend::default();
        let key = secret_key(Path::new("/p"), "env:local", "token");
        assert_eq!(m.get(&key).unwrap(), None);
        m.set(&key, "s3cr3t").unwrap();
        assert_eq!(m.get(&key).unwrap().as_deref(), Some("s3cr3t"));
        m.delete(&key).unwrap();
        assert_eq!(m.get(&key).unwrap(), None);
        let u = Unavailable("locked".into());
        assert_eq!(u.set("k", "v").unwrap_err(), "locked");
    }
}
