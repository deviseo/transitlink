//! 凭证存放在系统钥匙串里（macOS 钥匙串、Windows 凭据管理器），每个站点一条。

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

const SERVICE: &str = "com.deviseo.transitlink";

pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn set(&self, id: &str, value: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

pub struct KeychainStore;

impl SecretStore for KeychainStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, id).map_err(|error| error.to_string())?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, id: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, id).map_err(|error| error.to_string())?;
        entry.set_password(value).map_err(|error| error.to_string())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, id).map_err(|error| error.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// 内存实现（测试用）。
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().map_err(|_| "poisoned")?.get(id).cloned())
    }

    fn set(&self, id: &str, value: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "poisoned")?
            .insert(id.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        self.0.lock().map_err(|_| "poisoned")?.remove(id);
        Ok(())
    }
}
