//! 一键配置：把站点地址和专用 Key 写进 Claude Code、Codex 的配置。
//!
//! 流程：找到（或新建）这个工具的专用 Key → 用它请求站点的 `/v1/models`，确认能用 →
//! 备份 → 写配置 → 记下配到了哪里。测试不通过时一个字都不写，原有配置保持不变。
//! 还原只放回第一次配置前那几个关键字段的原值，用户的其他设置不动。

pub mod claude;
pub mod codex;
pub(crate) mod files;
pub mod probe;
mod state;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::accounts::Accounts;
use crate::sites::http::Http;
use crate::sites::{GroupSummary, Platform, SiteError};
use crate::tools::{self, ToolKind, ToolStatus};
use probe::TestResult;
use state::{Applied, Choice, ToolState};

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    /// 配置文件格式有误（解析阶段，还不知道是哪个文件）。
    Invalid,
    /// 这个配置文件格式有误，不能自动修改。
    InvalidFile(String),
    /// 读写配置文件失败。
    Io(String),
    /// 这个工具还没有用 TransitLink 配置过。
    NotConfigured,
    /// 测试没通过（配置前测试失败时，什么都没写）。
    Test(TestResult),
    Site(SiteError),
}

impl ConfigError {
    fn at(self, path: &Path) -> Self {
        match self {
            ConfigError::Invalid => ConfigError::InvalidFile(path.display().to_string()),
            other => other,
        }
    }
}

impl From<SiteError> for ConfigError {
    fn from(error: SiteError) -> Self {
        ConfigError::Site(error)
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(error: std::io::Error) -> Self {
        ConfigError::Io(error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigState {
    /// 没有配置第三方服务（用的是官方账号，或者还没配置过）。
    NotConfigured,
    /// 用 TransitLink 配置好了，配置文件和记录一致。
    Configured,
    /// 用 TransitLink 配置过，但配置文件后来被别的程序或手动改了。
    Changed,
    /// 用户自己配置了别的服务。
    Other,
    /// 配置文件格式有误。
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedView {
    pub site_id: String,
    pub site_name: String,
    pub key_name: String,
    pub group: Option<String>,
    pub model: Option<String>,
    pub applied_at: u64,
}

/// 工具页的一张卡片：检测结果加配置状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOverview {
    #[serde(flatten)]
    pub status: ToolStatus,
    pub state: ConfigState,
    pub applied: Option<AppliedView>,
    /// 配置文件里现在的服务地址、模型；Codex 还有服务商 ID。
    pub current_base_url: Option<String>,
    pub current_model: Option<String>,
    pub current_provider: Option<String>,
}

/// 一键配置前给界面的选项：分组、每个工具推荐的分组。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupPlan {
    pub groups: Vec<GroupSummary>,
    pub balance_usd: Option<f64>,
    pub tools: Vec<ToolPlan>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPlan {
    pub kind: ToolKind,
    pub recommended_group: Option<String>,
    /// 推荐的分组是按名称或平台认出来的（否则只是用户自己的或第一个分组）。
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub key_name: String,
    pub key_created: bool,
    pub group: Option<String>,
    pub model: Option<String>,
    pub test: TestResult,
}

/// 配置文件里现在的连接。
#[derive(Debug, Clone, Default)]
struct Connection {
    base_url: Option<String>,
    token: Option<String>,
    model: Option<String>,
    provider: Option<String>,
}

type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct Configurator {
    http: Http,
    home: PathBuf,
    data_dir: PathBuf,
    env: EnvLookup,
    state: ToolState,
    /// 同一时间只做一次写配置，避免两次操作交错。
    lock: tauri::async_runtime::Mutex<()>,
}

impl Configurator {
    pub fn new(http: Http, home: PathBuf, data_dir: PathBuf, env: EnvLookup) -> Self {
        Self {
            http,
            home,
            state: ToolState::new(data_dir.join("tools.json")),
            data_dir,
            env,
            lock: tauri::async_runtime::Mutex::new(()),
        }
    }

    fn config_path(&self, kind: ToolKind) -> PathBuf {
        kind.config_path(&self.home, &*self.env)
    }

    fn backup_dir(&self, kind: ToolKind) -> PathBuf {
        self.data_dir.join("backups").join(kind.backup_dir())
    }

    fn connection(&self, kind: ToolKind) -> Result<Connection, ConfigError> {
        let path = self.config_path(kind);
        let text = files::read(&path)?;
        let connection = match kind {
            ToolKind::ClaudeCode => claude::read(text.as_deref()).map(|current| Connection {
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: None,
            }),
            ToolKind::Codex => codex::read(text.as_deref()).map(|current| Connection {
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: current.provider,
            }),
        };
        connection.map_err(|error| error.at(&path))
    }

    /// 给检测结果补上配置状态。
    pub fn overview(&self, statuses: Vec<ToolStatus>) -> Vec<ToolOverview> {
        statuses
            .into_iter()
            .map(|status| {
                let kind = status.kind;
                let applied = self.state.get(kind);
                let connection = self.connection(kind);
                let state = match (&connection, &applied) {
                    (Err(_), _) => ConfigState::Invalid,
                    (Ok(current), Some(applied)) => {
                        let ours = kind != ToolKind::Codex
                            || current.provider.as_deref() == Some(codex::PROVIDER_ID);
                        if ours && current.base_url.as_deref() == Some(applied.base_url.as_str()) {
                            ConfigState::Configured
                        } else {
                            ConfigState::Changed
                        }
                    }
                    (Ok(current), None) => {
                        if current.base_url.is_some() || current.provider.is_some() {
                            ConfigState::Other
                        } else {
                            ConfigState::NotConfigured
                        }
                    }
                };
                let current = connection.unwrap_or_default();
                ToolOverview {
                    status,
                    state,
                    applied: applied.map(view),
                    current_base_url: current.base_url,
                    current_model: current.model,
                    current_provider: current.provider,
                }
            })
            .collect()
    }

    /// 一键配置前的选项：站点的分组和每个工具推荐的分组。
    pub async fn plan(&self, accounts: &Accounts, site_id: &str) -> Result<SetupPlan, ConfigError> {
        let site = accounts.site(site_id)?;
        let groups = match accounts.groups(site_id).await {
            Ok(groups) => groups,
            Err(SiteError::SessionExpired) => return Err(SiteError::SessionExpired.into()),
            // 老版本站点可能没有分组接口：用默认分组。
            Err(_) => Vec::new(),
        };
        let account = accounts.account(site_id).await.ok();
        let user_group = account.as_ref().and_then(|account| account.group.clone());
        let tools = ToolKind::ALL
            .iter()
            .map(|kind| {
                let (recommended_group, matched) =
                    recommend(*kind, site.info.platform, &groups, user_group.as_deref());
                ToolPlan {
                    kind: *kind,
                    recommended_group,
                    matched,
                }
            })
            .collect();
        Ok(SetupPlan {
            groups,
            balance_usd: account.map(|account| account.balance_usd),
            tools,
        })
    }

    /// 一键配置一个工具。`group_id` 为空时用站点的默认分组；`model` 为空时 Claude Code 用它自己的
    /// 默认模型，Codex 自动挑一个。
    pub async fn apply(
        &self,
        accounts: &Accounts,
        kind: ToolKind,
        site_id: &str,
        group_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<ApplyOutcome, ConfigError> {
        let _guard = self.lock.lock().await;
        let site = accounts.site(site_id)?;
        let path = self.config_path(kind);
        // 先确认配置文件能改，免得建好 Key 才发现写不进去。
        let text = files::read(&path)?;
        match kind {
            ToolKind::ClaudeCode => claude::read(text.as_deref()).map(|_| ()),
            ToolKind::Codex => codex::read(text.as_deref()).map(|_| ()),
        }
        .map_err(|error| error.at(&path))?;

        let (key, key_created) = accounts
            .tool_key(site_id, kind.key_name(), group_id)
            .await?;
        let base_url = site.info.base_url.clone();
        let ids = probe::list_models(&self.http, &base_url, &key.key)
            .await
            .map_err(ConfigError::Test)?;
        let test = probe::summarize(kind, &ids);
        if !test.passed() {
            return Err(ConfigError::Test(test));
        }
        // 指定的模型这个站点没有（例如切换站点时沿用的旧模型）：Claude Code 改用它的默认模型，
        // Codex 自动挑一个。
        let offered = model
            .filter(|model| ids.iter().any(|id| id == model))
            .map(str::to_string);
        let model = match kind {
            ToolKind::ClaudeCode => offered,
            ToolKind::Codex => Some(offered.or_else(|| probe::pick_codex_model(&ids)).ok_or(
                ConfigError::Test(TestResult::NoFamilyModels { models: ids.len() }),
            )?),
        };

        let previous = self.state.get(kind);
        let now = crate::clock::now_secs();
        let backups = self.backup_dir(kind);
        files::backup(&path, &backups, now, previous.is_none())?;
        let snapshot = match kind {
            ToolKind::ClaudeCode => {
                let previous = previous.as_ref().map(|applied| {
                    serde_json::from_value::<claude::Snapshot>(applied.snapshot.clone())
                        .unwrap_or_default()
                });
                let target = claude::Target {
                    base_url: &base_url,
                    token: &key.key,
                    model: model.as_deref(),
                };
                let (out, snapshot) = claude::apply(text.as_deref(), &target, previous.as_ref())
                    .map_err(|error| error.at(&path))?;
                files::write(&path, &out)?;
                self.mark_claude_onboarded(&backups, now);
                serde_json::to_value(snapshot)
            }
            ToolKind::Codex => {
                let previous = previous.as_ref().map(|applied| {
                    serde_json::from_value::<codex::Snapshot>(applied.snapshot.clone())
                        .unwrap_or_default()
                });
                let target = codex::Target {
                    site_name: &site.info.name,
                    base_url: &base_url,
                    token: &key.key,
                    model: model.as_deref().unwrap_or_default(),
                };
                let (out, snapshot) = codex::apply(text.as_deref(), &target, previous.as_ref())
                    .map_err(|error| error.at(&path))?;
                files::write(&path, &out)?;
                serde_json::to_value(snapshot)
            }
        }
        .map_err(|error| ConfigError::Io(error.to_string()))?;

        let group = key.group.clone();
        self.state.remember(
            &site.id,
            kind,
            Choice {
                group_id: group_id.map(str::to_string),
                model: model.clone(),
            },
        )?;
        self.state.set(
            kind,
            Applied {
                site_id: site.id.clone(),
                site_name: site.info.name.clone(),
                base_url,
                key_id: key.id.clone(),
                key_name: key.name.clone(),
                group: group.clone(),
                model: model.clone(),
                applied_at: now,
                snapshot,
            },
        )?;
        Ok(ApplyOutcome {
            key_name: key.name,
            key_created,
            group,
            model,
            test,
        })
    }

    /// Claude Code 第一次启动会要求登录 Anthropic 账号；标记已完成引导后直接可用。
    /// `.claude.json` 是 Claude Code 自己的状态文件，读不了或格式有误时跳过，不影响配置本身。
    fn mark_claude_onboarded(&self, backups: &Path, now: u64) {
        let path = tools::claude_user_config_path(&self.home, &*self.env);
        let Ok(text) = files::read(&path) else {
            return;
        };
        if let Ok(Some(out)) = claude::mark_onboarded(text.as_deref())
            && files::backup(&path, backups, now, true).is_ok()
        {
            let _ = files::write(&path, &out);
        }
    }

    /// 用工具配置里现在的地址和 Key 测试连通。
    pub async fn test(&self, kind: ToolKind) -> Result<TestResult, ConfigError> {
        let connection = self.connection(kind)?;
        let (Some(base_url), Some(token)) = (connection.base_url, connection.token) else {
            return Err(ConfigError::NotConfigured);
        };
        Ok(probe::test(&self.http, kind, &base_url, &token).await)
    }

    /// 可选的模型（适合这个工具的排在前面）。
    pub async fn models(&self, kind: ToolKind) -> Result<Vec<String>, ConfigError> {
        let connection = self.connection(kind)?;
        let (Some(base_url), Some(token)) = (connection.base_url, connection.token) else {
            return Err(ConfigError::NotConfigured);
        };
        let ids = probe::list_models(&self.http, &base_url, &token)
            .await
            .map_err(ConfigError::Test)?;
        Ok(probe::order_for(kind, ids))
    }

    /// 换模型。Claude Code 可以不指定（用它自己的默认模型）；Codex 必须指定。
    pub async fn set_model(&self, kind: ToolKind, model: Option<&str>) -> Result<(), ConfigError> {
        let _guard = self.lock.lock().await;
        let mut applied = self.state.get(kind).ok_or(ConfigError::NotConfigured)?;
        let path = self.config_path(kind);
        let text = files::read(&path)?;
        let out = match (kind, model) {
            (ToolKind::ClaudeCode, model) => claude::set_model(text.as_deref(), model),
            (ToolKind::Codex, Some(model)) => codex::set_model(text.as_deref(), model),
            (ToolKind::Codex, None) => {
                return Err(ConfigError::Test(TestResult::NoFamilyModels { models: 0 }));
            }
        }
        .map_err(|error| error.at(&path))?;
        let now = crate::clock::now_secs();
        files::backup(&path, &self.backup_dir(kind), now, false)?;
        files::write(&path, &out)?;
        applied.model = model.map(str::to_string);
        let mut choice = self
            .state
            .choice(&applied.site_id, kind)
            .unwrap_or_default();
        choice.model = applied.model.clone();
        self.state.remember(&applied.site_id, kind, choice)?;
        self.state.set(kind, applied)?;
        Ok(())
    }

    /// 用 TransitLink 配置着的工具（还原过的不算）。
    pub fn configured(&self) -> Vec<(ToolKind, AppliedView)> {
        self.state
            .all()
            .into_iter()
            .map(|(kind, applied)| (kind, view(applied)))
            .collect()
    }

    /// 把一个已经配置过的工具切换到另一个站点：沿用在那个站点上选过的分组和模型；
    /// 没选过就用推荐的分组，模型沿用现在的（那个站点没有时自动换）。
    pub async fn switch(
        &self,
        accounts: &Accounts,
        kind: ToolKind,
        site_id: &str,
    ) -> Result<ApplyOutcome, ConfigError> {
        let (group_id, model) = match self.state.choice(site_id, kind) {
            Some(choice) => (choice.group_id, choice.model),
            None => {
                let site = accounts.site(site_id)?;
                let groups = accounts.groups(site_id).await.unwrap_or_default();
                let user_group = accounts
                    .account(site_id)
                    .await
                    .ok()
                    .and_then(|account| account.group);
                let (group, _) =
                    recommend(kind, site.info.platform, &groups, user_group.as_deref());
                (
                    group,
                    self.state.get(kind).and_then(|applied| applied.model),
                )
            }
        };
        self.apply(
            accounts,
            kind,
            site_id,
            group_id.as_deref(),
            model.as_deref(),
        )
        .await
    }

    /// 还原到第一次用 TransitLink 配置之前。
    pub async fn restore(&self, kind: ToolKind) -> Result<(), ConfigError> {
        let _guard = self.lock.lock().await;
        let applied = self.state.get(kind).ok_or(ConfigError::NotConfigured)?;
        let path = self.config_path(kind);
        let text = files::read(&path)?;
        let restored = match kind {
            ToolKind::ClaudeCode => {
                let snapshot: claude::Snapshot =
                    serde_json::from_value(applied.snapshot).unwrap_or_default();
                claude::restore(text.as_deref(), &snapshot)
            }
            ToolKind::Codex => {
                let snapshot: codex::Snapshot =
                    serde_json::from_value(applied.snapshot).unwrap_or_default();
                codex::restore(text.as_deref(), &snapshot)
            }
        }
        .map_err(|error| error.at(&path))?;
        files::backup(
            &path,
            &self.backup_dir(kind),
            crate::clock::now_secs(),
            false,
        )?;
        match restored {
            Some(out) => files::write(&path, &out)?,
            None => files::remove(&path)?,
        }
        self.state.remove(kind)?;
        Ok(())
    }
}

fn view(applied: Applied) -> AppliedView {
    AppliedView {
        site_id: applied.site_id,
        site_name: applied.site_name,
        key_name: applied.key_name,
        group: applied.group,
        model: applied.model,
        applied_at: applied.applied_at,
    }
}

/// 给工具推荐分组：sub2api 按分组的平台（Claude Code 用 anthropic，Codex 用 openai）；new-api 按分组
/// 名称和说明里的关键词。都没认出来时，用用户自己的分组、`default` 或第一个分组。
fn recommend(
    kind: ToolKind,
    platform: Platform,
    groups: &[GroupSummary],
    user_group: Option<&str>,
) -> (Option<String>, bool) {
    if let Some(group) = groups
        .iter()
        .find(|group| fits_group(kind, platform, group))
    {
        return (Some(group.id.clone()), true);
    }
    let fallback = user_group
        .and_then(|name| groups.iter().find(|group| group.name == name))
        .or_else(|| groups.iter().find(|group| group.name == "default"))
        .or_else(|| groups.first());
    (fallback.map(|group| group.id.clone()), false)
}

fn fits_group(kind: ToolKind, platform: Platform, group: &GroupSummary) -> bool {
    match platform {
        Platform::Sub2api => {
            group.platform.as_deref()
                == Some(match kind {
                    ToolKind::ClaudeCode => "anthropic",
                    ToolKind::Codex => "openai",
                })
        }
        Platform::Newapi => {
            let text = format!("{} {}", group.name, group.description).to_lowercase();
            let mut words = text.split(|c: char| !c.is_ascii_alphanumeric());
            match kind {
                ToolKind::ClaudeCode => {
                    text.contains("claude")
                        || text.contains("anthropic")
                        || words.any(|word| word == "cc")
                }
                ToolKind::Codex => {
                    text.contains("codex") || text.contains("openai") || text.contains("gpt")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
