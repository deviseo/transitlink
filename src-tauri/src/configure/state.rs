//! 一键配置的记录：`~/.transitlink/tools.json`。记下每个工具配到了哪个站点、用的哪个 Key，
//! 第一次配置前关键字段的原值（还原用），以及在每个站点上选过的分组和模型（托盘切换站点时沿用）。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::files;
use crate::tools::ToolKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub site_id: String,
    pub site_name: String,
    pub base_url: String,
    pub key_id: String,
    pub key_name: String,
    /// 分组的显示名。
    pub group: Option<String>,
    pub model: Option<String>,
    pub applied_at: u64,
    /// 第一次配置前关键字段的原值（各工具自己的格式，见 claude::Snapshot、codex::Snapshot）。
    pub snapshot: Value,
}

/// 某个工具在某个站点上选过的分组和模型。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub group_id: Option<String>,
    pub model: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    tools: BTreeMap<String, Applied>,
    /// 站点 ID → 工具 ID → 选择。
    #[serde(default)]
    choices: BTreeMap<String, BTreeMap<String, Choice>>,
}

pub struct ToolState {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ToolState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> StateFile {
        files::read(&self.path)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write(&self, state: &StateFile) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
        files::write(&self.path, &text)
    }

    pub fn get(&self, kind: ToolKind) -> Option<Applied> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.read().tools.get(kind.id()).cloned()
    }

    pub fn set(&self, kind: ToolKind, applied: Applied) -> std::io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut state = self.read();
        state.version = 1;
        state.tools.insert(kind.id().to_string(), applied);
        self.write(&state)
    }

    /// 用 TransitLink 配置着的工具和它们的记录。
    pub fn all(&self) -> Vec<(ToolKind, Applied)> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let state = self.read();
        ToolKind::ALL
            .iter()
            .filter_map(|kind| {
                state
                    .tools
                    .get(kind.id())
                    .map(|applied| (*kind, applied.clone()))
            })
            .collect()
    }

    pub fn choice(&self, site_id: &str, kind: ToolKind) -> Option<Choice> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.read()
            .choices
            .get(site_id)
            .and_then(|tools| tools.get(kind.id()))
            .cloned()
    }

    pub fn remember(&self, site_id: &str, kind: ToolKind, choice: Choice) -> std::io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut state = self.read();
        state.version = 1;
        state
            .choices
            .entry(site_id.to_string())
            .or_default()
            .insert(kind.id().to_string(), choice);
        self.write(&state)
    }

    pub fn remove(&self, kind: ToolKind) -> std::io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut state = self.read();
        if state.tools.remove(kind.id()).is_some() {
            self.write(&state)?;
        }
        Ok(())
    }
}
