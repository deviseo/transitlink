//! 本地保存的站点列表：`~/.transitlink/sites.json`。凭证不在这里，在系统钥匙串里。

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::sites::SiteInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteRecord {
    pub id: String,
    pub info: SiteInfo,
    /// 登录用的账号（用户名或邮箱），用于显示和重新登录。
    pub account: String,
    pub added_at: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct SitesFile {
    version: u32,
    sites: Vec<SiteRecord>,
}

pub struct SiteStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl SiteStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<SitesFile, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("read {}: {error}", self.path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SitesFile {
                version: 1,
                sites: Vec::new(),
            }),
            Err(error) => Err(format!("read {}: {error}", self.path.display())),
        }
    }

    /// 先写临时文件再改名，避免写到一半时崩溃把列表弄坏。
    fn write(&self, file: &SitesFile) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("create {}: {error}", dir.display()))?;
        }
        let temp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
        std::fs::write(&temp, bytes)
            .map_err(|error| format!("write {}: {error}", temp.display()))?;
        std::fs::rename(&temp, &self.path)
            .map_err(|error| format!("save {}: {error}", self.path.display()))
    }

    pub fn list(&self) -> Result<Vec<SiteRecord>, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        Ok(self.read()?.sites)
    }

    pub fn get(&self, id: &str) -> Result<Option<SiteRecord>, String> {
        Ok(self.list()?.into_iter().find(|site| site.id == id))
    }

    /// 找同一站点、同一账号的记录（重新登录时沿用原来的 ID）。
    pub fn find(&self, base_url: &str, account: &str) -> Result<Option<SiteRecord>, String> {
        Ok(self.list()?.into_iter().find(|site| {
            site.info.base_url == base_url && site.account.eq_ignore_ascii_case(account)
        }))
    }

    pub fn upsert(&self, record: SiteRecord) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        let mut file = self.read()?;
        file.version = 1;
        match file.sites.iter_mut().find(|site| site.id == record.id) {
            Some(existing) => *existing = record,
            None => file.sites.push(record),
        }
        self.write(&file)
    }

    pub fn remove(&self, id: &str) -> Result<Option<SiteRecord>, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "store lock poisoned".to_string())?;
        let mut file = self.read()?;
        let index = file.sites.iter().position(|site| site.id == id);
        let removed = index.map(|index| file.sites.remove(index));
        if removed.is_some() {
            self.write(&file)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sites::Platform;

    fn record(id: &str, account: &str) -> SiteRecord {
        SiteRecord {
            id: id.into(),
            info: SiteInfo {
                platform: Platform::Newapi,
                base_url: "https://relay.example.com".into(),
                name: "Relay".into(),
                logo_url: None,
                captcha_required: false,
                password_login_disabled: false,
                email_only: false,
                quota_per_unit: Some(500000.0),
                password_encryption: false,
                user_agent: None,
            },
            account: account.into(),
            added_at: 1,
        }
    }

    #[test]
    fn saves_finds_and_removes_sites() {
        let dir = std::env::temp_dir().join(format!("transitlink-store-{}", std::process::id()));
        let store = SiteStore::new(dir.join("sites.json"));
        assert!(store.list().unwrap().is_empty());

        store.upsert(record("a", "Alice")).unwrap();
        store.upsert(record("b", "bob")).unwrap();
        store
            .upsert(SiteRecord {
                added_at: 2,
                ..record("a", "Alice")
            })
            .unwrap();
        assert_eq!(store.list().unwrap().len(), 2, "upsert replaces by id");
        assert_eq!(
            store
                .find("https://relay.example.com", "alice")
                .unwrap()
                .unwrap()
                .id,
            "a",
            "accounts match case-insensitively"
        );
        assert_eq!(store.get("a").unwrap().unwrap().added_at, 2);

        assert_eq!(store.remove("a").unwrap().unwrap().id, "a");
        assert!(store.remove("a").unwrap().is_none());
        assert_eq!(store.list().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
