//! 应用自己的设置：`~/.transitlink/settings.json`（界面语言、关掉窗口后是否留在托盘、当前站点、余额提醒）。

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::configure::files;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 关掉主窗口后留在托盘（macOS 菜单栏）里运行，可以从托盘切换站点。
    pub close_to_tray: bool,
    /// 界面语言，托盘菜单跟着用（由前端告诉我们）。
    pub language: String,
    /// 界面上的当前站点：概览、密钥、工单都看这个站点。不一定是 Claude Code / Codex 在用的站点
    /// （切换站点时用户可以选择工具不跟着切）。没选过时为空，界面自己挑一个。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_site: Option<String>,
    /// 余额和 Key 额度提醒。
    pub alerts: AlertSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            language: "zh-CN".to_string(),
            current_site: None,
            alerts: AlertSettings::default(),
        }
    }
}

/// 余额和 Key 额度提醒：系统通知加概览页的提示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertSettings {
    pub enabled: bool,
    /// 余额（美元）低于这个数就提醒；工具用的 Key 剩余额度低于它也提醒。
    pub balance_below: f64,
    /// 工具用的 Key 停用、过期、额度用完或快到期时提醒。
    pub keys: bool,
}

impl AlertSettings {
    /// 提醒线限制在 0 到 10 万美元之间（不是数字时用默认值）。
    pub fn cleaned(mut self) -> Self {
        self.balance_below = if self.balance_below.is_finite() {
            self.balance_below.clamp(0.0, 100_000.0)
        } else {
            Self::default().balance_below
        };
        self
    }
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            balance_below: 2.0,
            keys: true,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<AppSettings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let current = files::read(&path)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> AppSettings {
        self.current
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// 修改并保存；内容没变时不写文件。
    pub fn update(&self, change: impl FnOnce(&mut AppSettings)) -> std::io::Result<AppSettings> {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut next = current.clone();
        change(&mut next);
        if next != *current {
            let text = serde_json::to_string_pretty(&next).map_err(std::io::Error::other)?;
            files::write(&self.path, &text)?;
            *current = next;
        }
        Ok(current.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reloads_settings() {
        let dir = std::env::temp_dir().join(format!("transitlink-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let store = SettingsStore::load(path.clone());
        assert_eq!(store.get(), AppSettings::default());
        assert!(!path.exists(), "nothing is written until something changes");

        store
            .update(|settings| settings.close_to_tray = false)
            .unwrap();
        store
            .update(|settings| settings.language = "en-US".into())
            .unwrap();
        store
            .update(|settings| settings.current_site = Some("site-1".into()))
            .unwrap();
        let reloaded = SettingsStore::load(path);
        assert_eq!(
            reloaded.get(),
            AppSettings {
                close_to_tray: false,
                language: "en-US".into(),
                current_site: Some("site-1".into()),
                alerts: AlertSettings::default(),
            }
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
