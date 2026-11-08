//! 一键配置：把站点地址和专用 Key 写进 Claude Code、Codex 的配置。
//!
//! 流程：找到（或新建）这个工具的专用 Key → 用它请求站点的 `/v1/models`，确认能用 →
//! 备份 → 写配置 → 记下配到了哪里。测试不通过时一个字都不写，原有配置保持不变。
//! 还原只放回第一次配置前那几个关键字段的原值，用户的其他设置不动。

pub mod claude;
pub mod codex;
pub mod dsh;
pub(crate) mod files;
pub mod gemini;
pub mod grok;
mod jsonc;
pub mod kimi;
pub mod mcode;
pub(crate) mod multi;
pub mod omp;
pub mod openclaw;
pub mod opencode;
pub mod pi;
pub mod probe;
mod state;
mod yaml_map;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::accounts::Accounts;
use crate::siteconfig::{self, SiteConfig};
use crate::sites::http::Http;
use crate::sites::{GroupSummary, Platform, SiteError};
use crate::tools::{self, ToolKind, ToolStatus};
use probe::TestResult;
pub use state::Choice;
use state::{Applied, ToolState};

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
    /// 配置没法自动修改（用了特殊写法，或者被别的设置盖住了），附上是哪个文件或哪一项。
    Unsupported(String),
    /// 工具自己正在保存配置（文件被锁住了），稍后再试。
    Busy,
    /// 工具还有没做完的迁移（例如老版本的设置还没搬到新位置）：请用户先运行一次这个命令。
    RunFirst(String),
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
    /// 推荐的分组是站长指定的，或者按名称、平台认出来的（否则只是用户自己的或第一个分组）。
    pub matched: bool,
    /// 站长推荐的模型（没有时由工具自己决定，Codex 自动挑）。
    pub recommended_model: Option<String>,
    /// 分组和模型是在这个站点上选过的（或者从备份导入的），不是推荐的。
    pub remembered: bool,
}

/// 体检时用工具配置里的 Key 拉模型列表的结果。
#[derive(Debug, Clone)]
pub struct Diagnosis {
    pub test: TestResult,
    /// 站点给这个 Key 的模型（测试没通过时是空的）。
    pub ids: Vec<String>,
    /// 配置里现在的模型。
    pub model: Option<String>,
    pub elapsed_ms: u64,
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
pub(crate) struct Connection {
    pub(crate) base_url: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) provider: Option<String>,
    /// 请求发到我们写的服务商（Codex、Grok Build、OpenCode 可能被用户换成别的服务商）。
    pub(crate) ours: bool,
    /// 用户自己配置了别的服务（没用 TransitLink 配置时才看）。
    pub(crate) other: bool,
}

/// 用通用的多文件写法的工具（见 `multi`）。
pub fn multi(kind: ToolKind) -> Option<&'static dyn multi::MultiFile> {
    match kind {
        ToolKind::Pi => Some(&pi::Adapter),
        ToolKind::OhMyPi => Some(&omp::Adapter),
        ToolKind::KimiCode => Some(&kimi::Adapter),
        ToolKind::MinimaxCode => Some(&mcode::Adapter),
        ToolKind::OpenClaw => Some(&openclaw::Adapter),
        _ => None,
    }
}

/// 按工具的写法读出现在的连接。
fn read_connection(kind: ToolKind, text: Option<&str>) -> Result<Connection, ConfigError> {
    Ok(match kind {
        ToolKind::ClaudeCode => {
            let current = claude::read(text)?;
            Connection {
                other: current.base_url.is_some(),
                ours: true,
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: None,
            }
        }
        ToolKind::Codex => {
            let current = codex::read(text)?;
            Connection {
                ours: current.provider.as_deref() == Some(codex::PROVIDER_ID),
                other: current.base_url.is_some() || current.provider.is_some(),
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: current.provider,
            }
        }
        ToolKind::GeminiCli => {
            let current = gemini::read(text)?;
            Connection {
                other: current.base_url.is_some(),
                ours: true,
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: None,
            }
        }
        ToolKind::GrokBuild => {
            let current = grok::read(text)?;
            Connection {
                ours: current.default.as_deref() == Some(grok::MODEL_KEY),
                // 默认模型可能是 Grok 自己的模型（官方账号），不算别的服务。
                other: false,
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: current.default,
            }
        }
        // 几个文件，由 Configurator::connection 单独读；Antigravity CLI 不能一键连接。
        ToolKind::DeepseekHarness
        | ToolKind::Pi
        | ToolKind::OhMyPi
        | ToolKind::KimiCode
        | ToolKind::MinimaxCode
        | ToolKind::OpenClaw
        | ToolKind::Antigravity => Connection::default(),
        ToolKind::OpenCode => {
            let current = opencode::read(text)?;
            let ours = current.provider.as_deref() == Some(opencode::PROVIDER_ID);
            Connection {
                ours,
                other: current.provider.is_some() && !ours,
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: current.provider,
            }
        }
    })
}

type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// 按名字查环境变量。
pub type EnvFn<'a> = &'a dyn Fn(&str) -> Option<String>;

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

    /// DeepSeek Harness 的数据目录（`DSH_HOME` 或 `~/.dsh`）。
    fn dsh_home(&self) -> PathBuf {
        dsh::home(&self.home, &*self.env)
    }

    /// 读出多文件工具的这几个文件。
    fn multi_docs(&self, adapter: &dyn multi::MultiFile) -> Result<Vec<multi::Doc>, ConfigError> {
        multi::read(adapter.files(&self.home, &*self.env))
    }

    fn connection(&self, kind: ToolKind) -> Result<Connection, ConfigError> {
        if !kind.configurable() {
            return Ok(Connection::default());
        }
        if let Some(adapter) = multi(kind) {
            return adapter.read(&self.multi_docs(adapter)?);
        }
        if kind == ToolKind::DeepseekHarness {
            let current = dsh::read(&self.dsh_home())?;
            let ours = current.provider.as_deref() == Some(dsh::PROVIDER_ID);
            return Ok(Connection {
                ours,
                other: current.provider.is_some() && !ours,
                base_url: current.base_url,
                token: current.token,
                model: current.model,
                provider: current.provider,
            });
        }
        let path = self.config_path(kind);
        let text = files::read(&path)?;
        read_connection(kind, text.as_deref()).map_err(|error| error.at(&path))
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
                        if current.ours
                            && current.base_url.as_deref() == Some(applied.base_url.as_str())
                        {
                            ConfigState::Configured
                        } else {
                            ConfigState::Changed
                        }
                    }
                    (Ok(current), None) => {
                        if current.other {
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
            .filter(|kind| kind.configurable())
            .map(|kind| {
                // 在这个站点上给它选过（或者从备份导入了）分组和模型：默认还用它们。
                if let Some(choice) = self.state.choice(site_id, *kind)
                    && let Some(group) = choice
                        .group_id
                        .as_ref()
                        .filter(|group| groups.iter().any(|known| &&known.id == group))
                {
                    return ToolPlan {
                        kind: *kind,
                        recommended_group: Some(group.clone()),
                        matched: true,
                        recommended_model: choice.model.clone().filter(|model| !model.is_empty()),
                        remembered: true,
                    };
                }
                let site_recommend = operator_recommend(site.config.as_ref(), *kind);
                let (recommended_group, matched) = recommend(
                    *kind,
                    site.info.platform,
                    &groups,
                    user_group.as_deref(),
                    site_recommend,
                );
                ToolPlan {
                    kind: *kind,
                    recommended_group,
                    matched,
                    recommended_model: site_recommend
                        .map(|recommend| recommend.model.clone())
                        .filter(|model| !model.is_empty()),
                    remembered: false,
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
        if !kind.configurable() {
            return Err(ConfigError::Unsupported(kind.key_name().into()));
        }
        let site = accounts.site(site_id)?;
        let path = self.config_path(kind);
        // 先确认配置文件能改，免得建好 Key 才发现写不进去。
        let text = files::read(&path)?;
        if let Some(adapter) = multi(kind) {
            adapter.ready(&self.home, &*self.env)?;
            let docs = self.multi_docs(adapter)?;
            let probe = multi::Target {
                site_name: "",
                base_url: "https://relay.example.com",
                token: "check",
                model: "check",
                models: &[],
            };
            adapter.apply(&docs, &probe, None)?;
        } else if kind == ToolKind::DeepseekHarness {
            dsh::check(&self.dsh_home())?;
        } else {
            read_connection(kind, text.as_deref()).map_err(|error| error.at(&path))?;
        }

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
            _ => Some(offered.or_else(|| probe::pick_model(kind, &ids)).ok_or(
                ConfigError::Test(TestResult::NoFamilyModels { models: ids.len() }),
            )?),
        };

        let previous = self.state.get(kind);
        let now = crate::clock::now_secs();
        let backups = self.backup_dir(kind);
        if let Some(adapter) = multi(kind) {
            multi::backup(
                &self.multi_docs(adapter)?,
                &backups,
                now,
                previous.is_none(),
            )?;
        } else if kind == ToolKind::DeepseekHarness {
            for (file, name) in dsh::files(&self.dsh_home()) {
                files::backup_named(&file, &name, &backups, now, previous.is_none())?;
            }
        } else {
            files::backup(&path, &backups, now, previous.is_none())?;
        }
        let snapshot = match kind {
            ToolKind::Pi
            | ToolKind::OhMyPi
            | ToolKind::KimiCode
            | ToolKind::MinimaxCode
            | ToolKind::OpenClaw => {
                let adapter = multi(kind).expect("multi-file tool");
                let docs = self.multi_docs(adapter)?;
                if adapter.busy(&docs) {
                    return Err(ConfigError::Busy);
                }
                adapter.ready(&self.home, &*self.env)?;
                let chosen = model.as_deref().unwrap_or_default();
                let family = adapter.models(kind, chosen, &ids);
                let target = multi::Target {
                    site_name: &site.info.name,
                    base_url: &base_url,
                    token: &key.key,
                    model: chosen,
                    models: &family,
                };
                let previous = previous.as_ref().map(|applied| &applied.snapshot);
                let (next, snapshot) = adapter.apply(&docs, &target, previous)?;
                multi::write(&docs, &next)?;
                Ok(snapshot)
            }
            ToolKind::Antigravity => unreachable!("not configurable"),
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
            ToolKind::GeminiCli => {
                let previous = previous.as_ref().map(|applied| {
                    serde_json::from_value::<gemini::Snapshot>(applied.snapshot.clone())
                        .unwrap_or_default()
                });
                let target = gemini::Target {
                    base_url: &base_url,
                    token: &key.key,
                    model: model.as_deref().unwrap_or_default(),
                };
                let (out, mut snapshot) =
                    gemini::apply(text.as_deref(), &target, previous.as_ref())
                        .map_err(|error| error.at(&path))?;
                // settings.json 里把登录方式设成 API Key；第一次配置时记下原来的登录方式。
                let auth = self.set_gemini_auth(&backups, now)?;
                if snapshot.auth.is_none() {
                    snapshot.auth = Some(auth);
                }
                files::write(&path, &out)?;
                serde_json::to_value(snapshot)
            }
            ToolKind::GrokBuild => {
                let previous = previous.as_ref().map(|applied| {
                    serde_json::from_value::<grok::Snapshot>(applied.snapshot.clone())
                        .unwrap_or_default()
                });
                let target = grok::Target {
                    site_name: &site.info.name,
                    base_url: &base_url,
                    token: &key.key,
                    model: model.as_deref().unwrap_or_default(),
                };
                let (out, snapshot) = grok::apply(text.as_deref(), &target, previous.as_ref())
                    .map_err(|error| error.at(&path))?;
                files::write(&path, &out)?;
                serde_json::to_value(snapshot)
            }
            ToolKind::DeepseekHarness => {
                let previous = previous.as_ref().map(|applied| {
                    serde_json::from_value::<dsh::Snapshot>(applied.snapshot.clone())
                        .unwrap_or_default()
                });
                let chosen = model.as_deref().unwrap_or_default();
                let family = probe::dsh_family(chosen, &ids);
                let target = dsh::Target {
                    site_name: &site.info.name,
                    base_url: &base_url,
                    token: &key.key,
                    model: chosen,
                    models: &family,
                };
                let snapshot = dsh::apply(&self.dsh_home(), &target, previous.as_ref())?;
                serde_json::to_value(snapshot)
            }
            ToolKind::OpenCode => {
                let previous = previous.as_ref().map(|applied| {
                    serde_json::from_value::<opencode::Snapshot>(applied.snapshot.clone())
                        .unwrap_or_default()
                });
                let chosen = model.as_deref().unwrap_or_default();
                let family = probe::opencode_family(chosen, &ids);
                let small = probe::opencode_small_model(&family);
                let target = opencode::Target {
                    site_name: &site.info.name,
                    base_url: &base_url,
                    token: &key.key,
                    model: chosen,
                    models: &family,
                    small_model: small.as_deref(),
                };
                let (out, snapshot) = opencode::apply(text.as_deref(), &target, previous.as_ref())
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

    /// Gemini CLI 的 settings.json：登录方式设成 API Key，返回原来的登录方式（还原用）。
    fn set_gemini_auth(
        &self,
        backups: &Path,
        now: u64,
    ) -> Result<gemini::AuthSnapshot, ConfigError> {
        let path = tools::gemini_settings_path(&self.home, &*self.env);
        let text = files::read(&path)?;
        let (out, snapshot) = gemini::set_auth(text.as_deref()).map_err(|error| error.at(&path))?;
        files::backup(&path, backups, now, true)?;
        files::write(&path, &out)?;
        Ok(snapshot)
    }

    /// 还原 Gemini CLI 的登录方式。
    fn restore_gemini_auth(&self, snapshot: &gemini::AuthSnapshot) -> Result<(), ConfigError> {
        let path = tools::gemini_settings_path(&self.home, &*self.env);
        let text = files::read(&path)?;
        let restored =
            gemini::restore_auth(text.as_deref(), snapshot).map_err(|error| error.at(&path))?;
        files::backup(
            &path,
            &self.backup_dir(ToolKind::GeminiCli),
            crate::clock::now_secs(),
            false,
        )?;
        match restored {
            Some(out) => files::write(&path, &out)?,
            None => files::remove(&path)?,
        }
        Ok(())
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
        if let Some(adapter) = multi(kind) {
            return Ok(adapter.models(kind, &connection.model.unwrap_or_default(), &ids));
        }
        if kind == ToolKind::OpenCode {
            // 一个服务商只用一种接口：现在用的是 Claude 模型，就只能换别的 Claude 模型，反之亦然。
            let current = connection.model.unwrap_or_default();
            return Ok(probe::opencode_family(&current, &ids));
        }
        if kind == ToolKind::DeepseekHarness {
            let current = connection.model.unwrap_or_default();
            return Ok(probe::dsh_family(&current, &ids));
        }
        Ok(probe::order_for(kind, ids))
    }

    /// 换模型。Claude Code 可以不指定（用它自己的默认模型）；Codex 必须指定。
    pub async fn set_model(&self, kind: ToolKind, model: Option<&str>) -> Result<(), ConfigError> {
        let _guard = self.lock.lock().await;
        let mut applied = self.state.get(kind).ok_or(ConfigError::NotConfigured)?;
        if let Some(adapter) = multi(kind) {
            let model = model.ok_or(ConfigError::Test(TestResult::NoFamilyModels { models: 0 }))?;
            let docs = self.multi_docs(adapter)?;
            if adapter.busy(&docs) {
                return Err(ConfigError::Busy);
            }
            multi::backup(
                &docs,
                &self.backup_dir(kind),
                crate::clock::now_secs(),
                false,
            )?;
            let next = adapter.set_model(&docs, model)?;
            multi::write(&docs, &next)?;
            return self.remember_model(kind, &mut applied, Some(model));
        }
        if kind == ToolKind::DeepseekHarness {
            let model = model.ok_or(ConfigError::Test(TestResult::NoFamilyModels { models: 0 }))?;
            let home = self.dsh_home();
            let now = crate::clock::now_secs();
            for (file, name) in dsh::files(&home) {
                files::backup_named(&file, &name, &self.backup_dir(kind), now, false)?;
            }
            dsh::set_model(&home, model)?;
            return self.remember_model(kind, &mut applied, Some(model));
        }
        let path = self.config_path(kind);
        let text = files::read(&path)?;
        let out = match (kind, model) {
            (ToolKind::ClaudeCode, model) => claude::set_model(text.as_deref(), model),
            (_, None) => {
                return Err(ConfigError::Test(TestResult::NoFamilyModels { models: 0 }));
            }
            (ToolKind::Codex, Some(model)) => codex::set_model(text.as_deref(), model),
            (ToolKind::GeminiCli, Some(model)) => gemini::set_model(text.as_deref(), model),
            (ToolKind::GrokBuild, Some(model)) => grok::set_model(text.as_deref(), model),
            (ToolKind::OpenCode, Some(model)) => opencode::set_model(text.as_deref(), model),
            (
                ToolKind::DeepseekHarness
                | ToolKind::Pi
                | ToolKind::OhMyPi
                | ToolKind::KimiCode
                | ToolKind::MinimaxCode
                | ToolKind::OpenClaw
                | ToolKind::Antigravity,
                Some(_),
            ) => unreachable!("handled above"),
        }
        .map_err(|error| error.at(&path))?;
        let now = crate::clock::now_secs();
        files::backup(&path, &self.backup_dir(kind), now, false)?;
        files::write(&path, &out)?;
        self.remember_model(kind, &mut applied, model)
    }

    /// 记下换好的模型（下次切换到这个站点时沿用）。
    fn remember_model(
        &self,
        kind: ToolKind,
        applied: &mut Applied,
        model: Option<&str>,
    ) -> Result<(), ConfigError> {
        applied.model = model.map(str::to_string);
        let mut choice = self
            .state
            .choice(&applied.site_id, kind)
            .unwrap_or_default();
        choice.model = applied.model.clone();
        self.state.remember(&applied.site_id, kind, choice)?;
        self.state.set(kind, applied.clone())?;
        Ok(())
    }

    /// 用 TransitLink 配置过的工具和当时的记录（体检、提醒、迁移用）。
    pub fn applied_all(&self) -> Vec<(ToolKind, Applied)> {
        self.state.all()
    }

    /// 在这个站点上给这个工具选过的分组和模型。
    pub fn choice(&self, site_id: &str, kind: ToolKind) -> Option<Choice> {
        self.state.choice(site_id, kind)
    }

    /// 记下在某个站点上给工具选的分组和模型（测试里准备数据用）。
    #[cfg(test)]
    pub fn remember(
        &self,
        site_id: &str,
        kind: ToolKind,
        choice: Choice,
    ) -> Result<(), ConfigError> {
        self.state.remember(site_id, kind, choice)?;
        Ok(())
    }

    /// 导入备份：一次记下多个选择（下次一键配置、切换都沿用它们），这台电脑上已经有的不覆盖。
    pub fn remember_missing(
        &self,
        choices: &[(String, ToolKind, Choice)],
    ) -> Result<usize, ConfigError> {
        Ok(self.state.remember_missing(choices)?)
    }

    /// 体检用：用工具配置里现在的地址和 Key 拉一次模型列表，记下用了多久。
    pub async fn diagnose(&self, kind: ToolKind) -> Result<Diagnosis, ConfigError> {
        let connection = self.connection(kind)?;
        let (Some(base_url), Some(token)) = (connection.base_url, connection.token) else {
            return Err(ConfigError::NotConfigured);
        };
        let started = std::time::Instant::now();
        let listed = probe::list_models(&self.http, &base_url, &token).await;
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (test, ids) = match listed {
            Ok(ids) => (probe::summarize(kind, &ids), ids),
            Err(result) => (result, Vec::new()),
        };
        Ok(Diagnosis {
            test,
            ids,
            model: connection.model,
            elapsed_ms,
        })
    }

    /// 工具读的配置文件。`env` 为 None 时按应用自己的环境变量算（也就是 TransitLink 写的位置），
    /// 体检时传终端里的环境变量，看工具实际读的是不是同一个地方。
    pub fn config_files(&self, kind: ToolKind, env: Option<EnvFn<'_>>) -> Vec<PathBuf> {
        let env = env.unwrap_or(&*self.env);
        if let Some(adapter) = multi(kind) {
            return adapter
                .files(&self.home, env)
                .into_iter()
                .map(|(path, _)| path)
                .collect();
        }
        if kind == ToolKind::DeepseekHarness {
            return dsh::files(&dsh::home(&self.home, env))
                .into_iter()
                .map(|(path, _)| path)
                .collect();
        }
        vec![kind.config_path(&self.home, env)]
    }

    /// 配置文件里现在的连接（体检时和终端里的环境变量比较；里面有 Key，不要交给界面）。
    pub(crate) fn current(&self, kind: ToolKind) -> Result<Connection, ConfigError> {
        self.connection(kind)
    }

    /// 应用自己看到的环境变量。
    pub fn env_var(&self, name: &str) -> Option<String> {
        (self.env)(name)
    }

    pub fn home(&self) -> &Path {
        &self.home
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
                let site_recommend = operator_recommend(site.config.as_ref(), kind);
                let (group, _) = recommend(
                    kind,
                    site.info.platform,
                    &groups,
                    user_group.as_deref(),
                    site_recommend,
                );
                // 站长推荐了模型就用它，否则沿用现在的（那个站点没有时自动换）。
                let model = site_recommend
                    .map(|recommend| recommend.model.clone())
                    .filter(|model| !model.is_empty())
                    .or_else(|| self.state.get(kind).and_then(|applied| applied.model));
                (group, model)
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
        if let Some(adapter) = multi(kind) {
            let docs = self.multi_docs(adapter)?;
            if adapter.busy(&docs) {
                return Err(ConfigError::Busy);
            }
            multi::backup(
                &docs,
                &self.backup_dir(kind),
                crate::clock::now_secs(),
                false,
            )?;
            let next = adapter.restore(&docs, &applied.snapshot)?;
            multi::write(&docs, &next)?;
            self.state.remove(kind)?;
            return Ok(());
        }
        if kind == ToolKind::DeepseekHarness {
            let snapshot: dsh::Snapshot =
                serde_json::from_value(applied.snapshot).unwrap_or_default();
            let home = self.dsh_home();
            let now = crate::clock::now_secs();
            for (file, name) in dsh::files(&home) {
                files::backup_named(&file, &name, &self.backup_dir(kind), now, false)?;
            }
            dsh::restore(&home, &snapshot)?;
            self.state.remove(kind)?;
            return Ok(());
        }
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
            ToolKind::GeminiCli => {
                let snapshot: gemini::Snapshot =
                    serde_json::from_value(applied.snapshot).unwrap_or_default();
                if let Some(auth) = &snapshot.auth {
                    self.restore_gemini_auth(auth)?;
                }
                gemini::restore(text.as_deref(), &snapshot)
            }
            ToolKind::GrokBuild => {
                let snapshot: grok::Snapshot =
                    serde_json::from_value(applied.snapshot).unwrap_or_default();
                grok::restore(text.as_deref(), &snapshot)
            }
            ToolKind::OpenCode => {
                let snapshot: opencode::Snapshot =
                    serde_json::from_value(applied.snapshot).unwrap_or_default();
                opencode::restore(text.as_deref(), &snapshot)
            }
            ToolKind::DeepseekHarness
            | ToolKind::Pi
            | ToolKind::OhMyPi
            | ToolKind::KimiCode
            | ToolKind::MinimaxCode
            | ToolKind::OpenClaw
            | ToolKind::Antigravity => unreachable!("handled above"),
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

/// 站长在 TransitHub 里给这个工具的推荐（用专属链接接入的站点才有）。
fn operator_recommend(
    config: Option<&SiteConfig>,
    kind: ToolKind,
) -> Option<&siteconfig::ToolRecommend> {
    Some(config?.recommend.tool(kind))
}

/// 给工具推荐分组：先用站长指定的分组（站点里还有这个分组时）；再按 sub2api 分组的平台
/// （Claude Code 用 anthropic，Codex 用 openai）或 new-api 分组名称和说明里的关键词认；都没认出来时，
/// 用用户自己的分组、`default` 或第一个分组。
fn recommend(
    kind: ToolKind,
    platform: Platform,
    groups: &[GroupSummary],
    user_group: Option<&str>,
    operator: Option<&siteconfig::ToolRecommend>,
) -> (Option<String>, bool) {
    if let Some(wanted) = operator
        .map(|recommend| recommend.group.as_str())
        .filter(|group| !group.is_empty())
        && let Some(group) = groups
            .iter()
            .find(|group| group.id == wanted || group.name == wanted)
    {
        return (Some(group.id.clone()), true);
    }
    // OpenCode 什么模型都能用：优先 Claude 分组，其次 GPT 分组。
    // DeepSeek Harness 优先 DeepSeek 分组，其次和 OpenCode 一样。
    // Pi、Oh My Pi、OpenClaw 也是什么模型都能用；Kimi Code、MiniMax Code 优先自家模型的分组。
    let wanted: &[ToolKind] = match kind {
        ToolKind::OpenCode | ToolKind::Pi | ToolKind::OhMyPi | ToolKind::OpenClaw => {
            &[ToolKind::ClaudeCode, ToolKind::Codex]
        }
        ToolKind::DeepseekHarness => &[
            ToolKind::DeepseekHarness,
            ToolKind::ClaudeCode,
            ToolKind::Codex,
        ],
        ToolKind::KimiCode => &[ToolKind::KimiCode, ToolKind::ClaudeCode, ToolKind::Codex],
        ToolKind::MinimaxCode => &[ToolKind::MinimaxCode, ToolKind::ClaudeCode, ToolKind::Codex],
        _ => std::slice::from_ref(&kind),
    };
    for candidate in wanted {
        if let Some(group) = groups
            .iter()
            .find(|group| fits_group(*candidate, platform, group))
        {
            return (Some(group.id.clone()), true);
        }
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
            let wanted: &[&str] = match kind {
                ToolKind::ClaudeCode => &["anthropic"],
                ToolKind::Codex => &["openai"],
                ToolKind::GeminiCli => &["gemini"],
                ToolKind::GrokBuild => &["grok"],
                ToolKind::OpenCode => &["anthropic", "openai"],
                // 只认自家模型的分组；Claude、GPT 分组在 recommend 里按顺序再找。
                ToolKind::DeepseekHarness => &["deepseek"],
                ToolKind::KimiCode => &["kimi"],
                ToolKind::MinimaxCode => &["minimax"],
                ToolKind::Antigravity => &["antigravity", "gemini"],
                ToolKind::Pi | ToolKind::OhMyPi | ToolKind::OpenClaw => &["anthropic", "openai"],
            };
            group
                .platform
                .as_deref()
                .is_some_and(|platform| wanted.contains(&platform))
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
                ToolKind::GeminiCli => text.contains("gemini") || text.contains("google"),
                ToolKind::GrokBuild => text.contains("grok") || words.any(|word| word == "xai"),
                ToolKind::OpenCode => {
                    fits_group(ToolKind::ClaudeCode, platform, group)
                        || fits_group(ToolKind::Codex, platform, group)
                }
                ToolKind::DeepseekHarness => text.contains("deepseek"),
                ToolKind::KimiCode => text.contains("kimi") || text.contains("moonshot"),
                ToolKind::MinimaxCode => text.contains("minimax"),
                ToolKind::Antigravity => text.contains("gemini") || text.contains("antigravity"),
                ToolKind::Pi | ToolKind::OhMyPi | ToolKind::OpenClaw => {
                    fits_group(ToolKind::ClaudeCode, platform, group)
                        || fits_group(ToolKind::Codex, platform, group)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
