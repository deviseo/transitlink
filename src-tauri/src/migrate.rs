//! 换电脑：把站点、设置和工具的选择导出成一个文件，在新电脑上导入。
//!
//! 文件里没有登录凭证、Key，也没有工具配置文件里的原值：新电脑上每个站点重新登录一次，再一键恢复
//! 工具配置（用回原来的分组和模型，Key 用站点上原来那个）。导入的文件当作不可信的输入：地址重新校验，
//! 站长配置重新清洗，工单服务的授权不带过来（新电脑上用工单时再确认一次）。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::accounts::Accounts;
use crate::configure::{Choice, Configurator};
use crate::settings::{AlertSettings, AppSettings, SettingsStore};
use crate::siteconfig::{self, SiteConfig};
use crate::sites::{self, SiteError, SiteInfo};
use crate::tools::ToolKind;

const FORMAT: &str = "transitlink-backup";
const VERSION: u32 = 1;
/// 备份文件最大多大（站长配置里有 logo 时会大一些）。
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_SITES: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum MigrateError {
    /// 不是 TransitLink 的备份文件，或者文件坏了。
    Invalid,
    /// 新版本 TransitLink 导出的，这个版本看不懂。
    TooNew,
    TooLarge,
    /// 备份文件存不下来（例如系统没允许访问「下载」文件夹）。
    Save(String),
    Io(String),
    Site(SiteError),
}

impl MigrateError {
    pub fn code(&self) -> &'static str {
        match self {
            MigrateError::Invalid => "backupInvalid",
            MigrateError::TooNew => "backupTooNew",
            MigrateError::TooLarge => "backupTooLarge",
            MigrateError::Save(_) => "backupWrite",
            MigrateError::Io(_) => "configWrite",
            MigrateError::Site(error) => error.code(),
        }
    }
}

impl From<SiteError> for MigrateError {
    fn from(error: SiteError) -> Self {
        MigrateError::Site(error)
    }
}

/// 文件的样子。站点、工具一个一个解析：新版本多出来的工具或平台跳过，不让整个文件导不进来。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupFile {
    format: String,
    version: u32,
    #[serde(default)]
    exported_at: u64,
    #[serde(default)]
    app_version: String,
    #[serde(default)]
    sites: Vec<Value>,
    #[serde(default)]
    settings: Option<Value>,
    #[serde(default)]
    tools: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupSite {
    /// 旧电脑上的站点 ID：只用来对应工具和选择，导入时换成新的。
    id: String,
    info: SiteInfo,
    account: String,
    #[serde(default)]
    added_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<SiteConfig>,
    /// 在这个站点上给工具选过的分组和模型（工具 ID → 选择）。
    #[serde(default)]
    choices: BTreeMap<String, Choice>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupSettings {
    #[serde(default)]
    close_to_tray: Option<bool>,
    #[serde(default)]
    alerts: Option<AlertSettings>,
    #[serde(default)]
    current_site: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupTool {
    kind: String,
    site_id: String,
}

/// 解析、校验过的备份。
#[derive(Debug)]
pub struct Backup {
    exported_at: u64,
    sites: Vec<BackupSite>,
    settings: Option<BackupSettings>,
    tools: Vec<(ToolKind, String)>,
    /// 认不出来、跳过了的站点和工具。
    skipped: usize,
}

fn tool_from_id(id: &str) -> Option<ToolKind> {
    ToolKind::ALL.iter().copied().find(|kind| kind.id() == id)
}

fn clip(value: &str, limit: usize) -> String {
    value.trim().chars().take(limit).collect()
}

/// 把这台电脑上的站点、设置和工具的选择打包（不含凭证、Key、配置原值）。
pub fn export(
    accounts: &Accounts,
    configurator: &Configurator,
    settings: &AppSettings,
    app_version: &str,
) -> Result<String, MigrateError> {
    let sites = accounts
        .records()?
        .into_iter()
        .map(|record| {
            let choices = ToolKind::ALL
                .iter()
                .filter_map(|kind| {
                    configurator
                        .choice(&record.id, *kind)
                        .map(|choice| (kind.id().to_string(), choice))
                })
                .collect();
            let mut info = record.info;
            // 登录窗口的 User-Agent 是这台电脑的，新电脑登录时会换成它自己的。
            info.user_agent = None;
            serde_json::to_value(BackupSite {
                id: record.id,
                info,
                account: record.account,
                added_at: record.added_at,
                config_url: record.config_url,
                config: record.config,
                choices,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| MigrateError::Io(error.to_string()))?;
    let tools = configurator
        .applied_all()
        .into_iter()
        .map(|(kind, applied)| {
            serde_json::to_value(BackupTool {
                kind: kind.id().to_string(),
                site_id: applied.site_id,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| MigrateError::Io(error.to_string()))?;
    let settings = serde_json::to_value(BackupSettings {
        close_to_tray: Some(settings.close_to_tray),
        alerts: Some(settings.alerts.clone()),
        current_site: settings.current_site.clone(),
    })
    .ok();
    let file = BackupFile {
        format: FORMAT.to_string(),
        version: VERSION,
        exported_at: crate::clock::now_secs(),
        app_version: app_version.to_string(),
        sites,
        settings,
        tools,
    };
    serde_json::to_string_pretty(&file).map_err(|error| MigrateError::Io(error.to_string()))
}

/// 存到这个目录（一般是「下载」），文件名带日期，重名时加编号。Unix 上只有自己能读。
pub fn save(text: &str, dir: &Path, now: u64) -> Result<PathBuf, MigrateError> {
    let date = chrono::DateTime::from_timestamp(i64::try_from(now).unwrap_or(0), 0)
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "backup".to_string());
    std::fs::create_dir_all(dir).map_err(|error| MigrateError::Save(error.to_string()))?;
    for index in 1..1000 {
        let name = if index == 1 {
            format!("transitlink-backup-{date}.json")
        } else {
            format!("transitlink-backup-{date}-{index}.json")
        };
        let path = dir.join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&path) {
            Ok(mut file) => {
                std::io::Write::write_all(&mut file, text.as_bytes())
                    .map_err(|error| MigrateError::Save(error.to_string()))?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(MigrateError::Save(error.to_string())),
        }
    }
    Err(MigrateError::Save("too many backups today".into()))
}

/// 解析并校验备份文件。
pub fn parse(text: &str) -> Result<Backup, MigrateError> {
    if text.len() > MAX_BYTES {
        return Err(MigrateError::TooLarge);
    }
    let file: BackupFile = serde_json::from_str(text).map_err(|_| MigrateError::Invalid)?;
    if file.format != FORMAT {
        return Err(MigrateError::Invalid);
    }
    if file.version > VERSION {
        return Err(MigrateError::TooNew);
    }
    let mut skipped = 0;
    let mut sites: Vec<BackupSite> = Vec::new();
    for value in file.sites {
        match serde_json::from_value::<BackupSite>(value)
            .ok()
            .and_then(clean_site)
        {
            Some(site)
                if sites.len() < MAX_SITES
                    && !sites.iter().any(|known| {
                        known.info.base_url == site.info.base_url
                            && known.account.eq_ignore_ascii_case(&site.account)
                    }) =>
            {
                sites.push(site)
            }
            _ => skipped += 1,
        }
    }
    let mut tools: Vec<(ToolKind, String)> = Vec::new();
    for value in file.tools {
        match serde_json::from_value::<BackupTool>(value)
            .ok()
            .and_then(|tool| Some((tool_from_id(&tool.kind)?, tool.site_id)))
        {
            Some((kind, site_id))
                if kind.configurable()
                    && sites.iter().any(|site| site.id == site_id)
                    && !tools.iter().any(|(known, _)| *known == kind) =>
            {
                tools.push((kind, site_id))
            }
            _ => skipped += 1,
        }
    }
    let settings = file
        .settings
        .and_then(|value| serde_json::from_value::<BackupSettings>(value).ok())
        .map(|mut settings| {
            settings.alerts = settings.alerts.map(AlertSettings::cleaned);
            settings
        });
    Ok(Backup {
        exported_at: file.exported_at,
        sites,
        settings,
        tools,
        skipped,
    })
}

/// 校验一个站点：地址重新规范化，长度有限制，站长配置重新清洗、地址要和站点一致，选择只留认得的工具。
fn clean_site(mut site: BackupSite) -> Option<BackupSite> {
    site.info.base_url = sites::normalize_url(&site.info.base_url).ok()?;
    site.account = clip(&site.account, 200);
    if site.account.is_empty() || site.id.is_empty() || site.id.chars().count() > 100 {
        return None;
    }
    site.info.name = clip(&site.info.name, 100);
    site.info.user_agent = None;
    site.info.logo_url = site
        .info
        .logo_url
        .map(|logo| logo.trim().to_string())
        .filter(|logo| logo.starts_with("https://") || logo.starts_with("data:image/"));
    let config_url = site
        .config_url
        .as_deref()
        .and_then(|url| url::Url::parse(url).ok())
        .filter(siteconfig::allowed);
    let saved = site.config.take();
    let had_config = saved.is_some();
    let config = match (&config_url, saved) {
        (Some(url), Some(config)) => siteconfig::sanitize(config, url)
            .ok()
            .filter(|config| config.site.base_url == site.info.base_url),
        _ => None,
    };
    // 有配置但和站点对不上：链接也不要了。只有链接没有配置的，留着链接，之后会重新读。
    site.config_url = match (&config_url, &config) {
        (Some(url), Some(_)) => Some(url.to_string()),
        (Some(url), None) if !had_config => Some(url.to_string()),
        _ => None,
    };
    site.config = config;
    site.choices = std::mem::take(&mut site.choices)
        .into_iter()
        .filter(|(id, _)| tool_from_id(id).is_some_and(ToolKind::configurable))
        .map(|(id, choice)| {
            let clean = |value: Option<String>| {
                value
                    .map(|value| clip(&value, 200))
                    .filter(|value| !value.is_empty())
            };
            (
                id,
                Choice {
                    group_id: clean(choice.group_id),
                    model: clean(choice.model),
                },
            )
        })
        .collect();
    Some(site)
}

/// 导入前给用户看的：有哪些站点（这台电脑上已经有的会合并）、哪些工具要恢复。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub exported_at: u64,
    pub sites: Vec<PreviewSite>,
    pub tools: Vec<PreviewTool>,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSite {
    pub name: String,
    pub host: String,
    pub account: String,
    /// 这台电脑上已经有了（不会改它）。
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTool {
    pub kind: ToolKind,
    pub site_name: String,
    /// 这台电脑上已经用 TransitLink 配置过了（不会动它）。
    pub configured: bool,
}

fn site_name(site: &BackupSite) -> String {
    site.config
        .as_ref()
        .map(|config| config.brand.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| site.info.name.clone())
}

fn host(base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| base_url.to_string())
}

pub fn preview(accounts: &Accounts, configurator: &Configurator, backup: &Backup) -> Preview {
    let records = accounts.records().unwrap_or_default();
    let configured: Vec<ToolKind> = configurator
        .applied_all()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    Preview {
        exported_at: backup.exported_at,
        sites: backup
            .sites
            .iter()
            .map(|site| PreviewSite {
                name: site_name(site),
                host: host(&site.info.base_url),
                account: site.account.clone(),
                exists: records.iter().any(|record| {
                    record.info.base_url == site.info.base_url
                        && record.account.eq_ignore_ascii_case(&site.account)
                }),
            })
            .collect(),
        tools: backup
            .tools
            .iter()
            .filter_map(|(kind, site_id)| {
                let site = backup.sites.iter().find(|site| &site.id == site_id)?;
                Some(PreviewTool {
                    kind: *kind,
                    site_name: site_name(site),
                    configured: configured.contains(kind),
                })
            })
            .collect(),
        skipped: backup.skipped,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub added: usize,
    pub merged: usize,
    pub pending: Vec<PendingView>,
}

/// 导入：加站点（已有的不动）、记下在每个站点上的选择（已有的不覆盖）、设置、要恢复的工具。
pub fn import(
    accounts: &Accounts,
    configurator: &Configurator,
    settings: &SettingsStore,
    pending: &PendingStore,
    backup: Backup,
) -> Result<ImportOutcome, MigrateError> {
    let mut ids: HashMap<String, String> = HashMap::new();
    let (mut added, mut merged) = (0, 0);
    let mut choices: Vec<(String, ToolKind, Choice)> = Vec::new();
    for site in backup.sites {
        let (id, is_new) = accounts.import_site(
            site.info,
            &site.account,
            if site.added_at == 0 {
                crate::clock::now_secs()
            } else {
                site.added_at
            },
            site.config_url,
            site.config,
        )?;
        if is_new {
            added += 1;
        } else {
            merged += 1;
        }
        choices.extend(
            site.choices
                .into_iter()
                .filter_map(|(tool, choice)| Some((id.clone(), tool_from_id(&tool)?, choice))),
        );
        ids.insert(site.id, id);
    }
    configurator
        .remember_missing(&choices)
        .map_err(|_| MigrateError::Io("tools.json".into()))?;
    // 设置只在这台电脑上还是默认的时候才用备份里的（不覆盖用户在这台电脑上改过的）。
    if let Some(from) = backup.settings {
        let defaults = AppSettings::default();
        settings
            .update(|local| {
                if let Some(close_to_tray) = from.close_to_tray
                    && local.close_to_tray == defaults.close_to_tray
                {
                    local.close_to_tray = close_to_tray;
                }
                if let Some(alerts) = from.alerts.clone()
                    && local.alerts == defaults.alerts
                {
                    local.alerts = alerts;
                }
                if local.current_site.is_none() {
                    local.current_site = from
                        .current_site
                        .as_ref()
                        .and_then(|old| ids.get(old).cloned());
                }
            })
            .map_err(|error| MigrateError::Io(error.to_string()))?;
    }
    // 这台电脑上已经配置过的工具不动。
    let configured: Vec<ToolKind> = configurator
        .applied_all()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    let restore: Vec<(ToolKind, String)> = backup
        .tools
        .into_iter()
        .filter(|(kind, _)| !configured.contains(kind))
        .filter_map(|(kind, old)| Some((kind, ids.get(&old)?.clone())))
        .collect();
    pending.add(&restore)?;
    Ok(ImportOutcome {
        added,
        merged,
        pending: pending_views(accounts, configurator, pending),
    })
}

/// 等着恢复的工具：在新电脑上登录站点后，一键用原来的分组和模型配置好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingTool {
    kind: ToolKind,
    site_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    pub kind: ToolKind,
    pub site_id: String,
    pub site_name: String,
}

#[derive(Default, Serialize, Deserialize)]
struct PendingFile {
    version: u32,
    #[serde(default)]
    tools: Vec<PendingTool>,
}

/// `~/.transitlink/migration.json`：从备份导入、还没恢复的工具。
pub struct PendingStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl PendingStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> PendingFile {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, file: &PendingFile) -> Result<(), MigrateError> {
        if file.tools.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(MigrateError::Io(error.to_string()))
                }
                _ => Ok(()),
            };
        }
        let text = serde_json::to_string_pretty(file)
            .map_err(|error| MigrateError::Io(error.to_string()))?;
        crate::configure::files::write(&self.path, &text)
            .map_err(|error| MigrateError::Io(error.to_string()))
    }

    fn change(&self, edit: impl FnOnce(&mut Vec<PendingTool>)) -> Result<(), MigrateError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut file = self.read();
        let before = file.tools.clone();
        edit(&mut file.tools);
        if file.tools == before {
            return Ok(());
        }
        file.version = 1;
        self.write(&file)
    }

    /// 加上要恢复的工具（同一个工具只留最新的）。
    fn add(&self, tools: &[(ToolKind, String)]) -> Result<(), MigrateError> {
        self.change(|list| {
            for (kind, site_id) in tools {
                list.retain(|tool| tool.kind != *kind);
                list.push(PendingTool {
                    kind: *kind,
                    site_id: site_id.clone(),
                });
            }
        })
    }

    fn list(&self) -> Vec<PendingTool> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.read().tools
    }

    /// 这几个工具恢复好了（或者不用恢复了）。
    pub fn remove(&self, kinds: &[ToolKind]) -> Result<(), MigrateError> {
        self.change(|list| list.retain(|tool| !kinds.contains(&tool.kind)))
    }

    pub fn clear(&self) -> Result<(), MigrateError> {
        self.change(Vec::clear)
    }

    /// 这个站点上等着恢复的工具。
    pub fn kinds_for(&self, site_id: &str) -> Vec<ToolKind> {
        self.list()
            .into_iter()
            .filter(|tool| tool.site_id == site_id)
            .map(|tool| tool.kind)
            .collect()
    }
}

/// 还等着恢复的工具。站点删掉了、工具已经在这台电脑上配置过了的，顺手去掉。
pub fn pending_views(
    accounts: &Accounts,
    configurator: &Configurator,
    pending: &PendingStore,
) -> Vec<PendingView> {
    let configured: Vec<ToolKind> = configurator
        .applied_all()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    let mut stale = Vec::new();
    let mut views = Vec::new();
    for tool in pending.list() {
        match accounts.site(&tool.site_id) {
            Ok(record) if !configured.contains(&tool.kind) => views.push(PendingView {
                kind: tool.kind,
                site_name: record
                    .config
                    .as_ref()
                    .map(|config| config.brand.name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(record.info.name),
                site_id: tool.site_id,
            }),
            _ => stale.push(tool.kind),
        }
    }
    if !stale.is_empty() {
        let _ = pending.remove(&stale);
    }
    views
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;
    use crate::sites::http::Http;
    use crate::sites::{Credential, Platform};
    use crate::store::SiteStore;

    /// 测试用的临时目录，用完删掉。
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("transitlink-migrate-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Env {
        _dir: Scratch,
        accounts: Accounts,
        configurator: Configurator,
        settings: SettingsStore,
        pending: PendingStore,
    }

    fn env(name: &str) -> Env {
        let dir = Scratch::new(name);
        let data = dir.0.join(".transitlink");
        Env {
            accounts: Accounts::new(
                Http::new(),
                SiteStore::new(data.join("sites.json")),
                Box::new(MemoryStore::default()),
            ),
            configurator: Configurator::new(
                Http::new(),
                dir.0.clone(),
                data.clone(),
                Box::new(|_| None),
            ),
            settings: SettingsStore::load(data.join("settings.json")),
            pending: PendingStore::new(data.join("migration.json")),
            _dir: dir,
        }
    }

    fn info(base_url: &str) -> SiteInfo {
        SiteInfo {
            platform: Platform::Newapi,
            base_url: base_url.into(),
            name: "好用中转".into(),
            logo_url: Some("javascript:alert(1)".into()),
            captcha_required: false,
            password_login_disabled: false,
            email_only: false,
            quota_per_unit: None,
            password_encryption: false,
            user_agent: Some("Mozilla/5.0 old laptop".into()),
        }
    }

    fn credential() -> Credential {
        Credential::NewapiToken {
            token: "secret-access-token".into(),
        }
    }

    #[test]
    fn export_has_no_credentials_and_imports_on_a_new_computer() {
        let old = env("old");
        let site = old
            .accounts
            .save(
                info("https://relay.example.com"),
                "alice@example.com",
                &credential(),
            )
            .unwrap();
        old.configurator
            .remember(
                &site.id,
                ToolKind::Codex,
                Choice {
                    group_id: Some("codex".into()),
                    model: Some("gpt-6.1-sol".into()),
                },
            )
            .unwrap();
        old.settings
            .update(|settings| {
                settings.current_site = Some(site.id.clone());
                settings.alerts.balance_below = 5.0;
            })
            .unwrap();
        let text = export(
            &old.accounts,
            &old.configurator,
            &old.settings.get(),
            "0.7.0",
        )
        .unwrap();
        assert!(!text.contains("secret-access-token"), "{text}");
        assert!(!text.contains("old laptop"), "{text}");
        assert!(text.contains("gpt-6.1-sol"));

        let new = env("new");
        let backup = parse(&text).unwrap();
        let preview = preview(&new.accounts, &new.configurator, &backup);
        assert_eq!(preview.sites.len(), 1);
        assert!(!preview.sites[0].exists);
        assert_eq!(preview.sites[0].host, "relay.example.com");
        // 旧电脑上没有配置好的工具，只有选择。
        assert!(preview.tools.is_empty());

        let outcome = import(
            &new.accounts,
            &new.configurator,
            &new.settings,
            &new.pending,
            backup,
        )
        .unwrap();
        assert_eq!((outcome.added, outcome.merged), (1, 0));
        let sites = new.accounts.records().unwrap();
        assert_eq!(sites.len(), 1);
        let imported = &sites[0];
        assert_ne!(imported.id, site.id, "a fresh ID on the new computer");
        assert_eq!(imported.info.logo_url, None, "unsafe logo dropped");
        assert_eq!(imported.ticket_hub, None);
        // 没有凭证：用的时候提示重新登录。
        assert!(matches!(
            new.accounts.load(&imported.id),
            Err(SiteError::SessionExpired)
        ));
        assert_eq!(
            new.configurator.choice(&imported.id, ToolKind::Codex),
            Some(Choice {
                group_id: Some("codex".into()),
                model: Some("gpt-6.1-sol".into()),
            })
        );
        let settings = new.settings.get();
        assert_eq!(settings.current_site.as_deref(), Some(imported.id.as_str()));
        assert_eq!(settings.alerts.balance_below, 5.0);

        // 再导入一次：合并，不会多出一个站点。
        let again = import(
            &new.accounts,
            &new.configurator,
            &new.settings,
            &new.pending,
            parse(&text).unwrap(),
        )
        .unwrap();
        assert_eq!((again.added, again.merged), (1 - 1, 1));
        assert_eq!(new.accounts.records().unwrap().len(), 1);
    }

    #[test]
    fn configured_tools_wait_for_a_one_click_restore() {
        let new = env("restore");
        let text = serde_json::json!({
            "format": FORMAT,
            "version": 1,
            "sites": [{
                "id": "old-1",
                "info": serde_json::to_value(info("relay.example.com")).unwrap(),
                "account": "alice",
                "choices": { "claudeCode": { "groupId": "cc", "model": null }, "someFutureTool": {} }
            }, {
                "id": "old-2",
                "info": { "platform": "someFuturePlatform", "baseUrl": "https://x.example.com" },
                "account": "bob"
            }],
            "tools": [
                { "kind": "claudeCode", "siteId": "old-1" },
                { "kind": "antigravity", "siteId": "old-1" },
                { "kind": "someFutureTool", "siteId": "old-1" },
                { "kind": "codex", "siteId": "nowhere" }
            ]
        })
        .to_string();
        let backup = parse(&text).unwrap();
        assert_eq!(backup.sites.len(), 1);
        assert_eq!(backup.sites[0].info.base_url, "https://relay.example.com");
        assert_eq!(
            backup.skipped, 4,
            "unknown platform, tools and sites are skipped"
        );
        let outcome = import(
            &new.accounts,
            &new.configurator,
            &new.settings,
            &new.pending,
            backup,
        )
        .unwrap();
        assert_eq!(outcome.pending.len(), 1);
        let pending = &outcome.pending[0];
        assert_eq!(pending.kind, ToolKind::ClaudeCode);
        assert_eq!(
            new.pending.kinds_for(&pending.site_id),
            vec![ToolKind::ClaudeCode]
        );

        // 站点删掉了：不用恢复了。
        new.accounts.remove(&pending.site_id).unwrap();
        assert!(pending_views(&new.accounts, &new.configurator, &new.pending).is_empty());
        assert!(new.pending.list().is_empty());
    }

    #[test]
    fn settings_changed_on_this_computer_are_kept() {
        let new = env("keep-settings");
        new.settings
            .update(|settings| {
                settings.alerts.enabled = true;
                settings.alerts.balance_below = 9.0;
            })
            .unwrap();
        let text = serde_json::json!({
            "format": FORMAT,
            "version": 1,
            "settings": { "closeToTray": false, "alerts": { "enabled": false, "balanceBelow": 1, "keys": false } }
        })
        .to_string();
        import(
            &new.accounts,
            &new.configurator,
            &new.settings,
            &new.pending,
            parse(&text).unwrap(),
        )
        .unwrap();
        let settings = new.settings.get();
        assert_eq!(settings.alerts.balance_below, 9.0, "changed here, so kept");
        assert!(settings.alerts.enabled);
        assert!(
            !settings.close_to_tray,
            "still the default here, so taken from the backup"
        );
    }

    #[test]
    fn rejects_files_that_are_not_backups() {
        assert_eq!(parse("not json").unwrap_err(), MigrateError::Invalid);
        assert_eq!(
            parse(r#"{"format":"something-else","version":1}"#).unwrap_err(),
            MigrateError::Invalid
        );
        assert_eq!(
            parse(r#"{"format":"transitlink-backup","version":99}"#).unwrap_err(),
            MigrateError::TooNew
        );
        let huge = format!(
            r#"{{"format":"transitlink-backup","version":1,"x":"{}"}}"#,
            "a".repeat(MAX_BYTES)
        );
        assert_eq!(parse(&huge).unwrap_err(), MigrateError::TooLarge);
    }

    #[test]
    fn mismatched_operator_config_is_dropped() {
        let mut site = BackupSite {
            id: "old".into(),
            info: info("https://relay.example.com"),
            account: "alice".into(),
            added_at: 1,
            config_url: Some("https://hub.example.com/api/public/client-config/abc".into()),
            config: Some(SiteConfig {
                site: crate::siteconfig::ConfigSite {
                    name: "别人的站".into(),
                    base_url: "https://other.example.com".into(),
                    platform: "newapi".into(),
                },
                ..SiteConfig::default()
            }),
            choices: BTreeMap::new(),
        };
        let cleaned = clean_site(site.clone()).unwrap();
        assert_eq!((cleaned.config, cleaned.config_url), (None, None));

        site.config.as_mut().unwrap().site.base_url = "https://relay.example.com/".into();
        let cleaned = clean_site(site.clone()).unwrap();
        assert!(cleaned.config.is_some());
        assert!(cleaned.config_url.is_some());

        site.config_url = Some("http://evil.example.com/config".into());
        let cleaned = clean_site(site).unwrap();
        assert_eq!(
            (cleaned.config, cleaned.config_url),
            (None, None),
            "http links are not allowed"
        );
    }

    #[test]
    fn saves_with_a_dated_name_without_overwriting() {
        let dir = Scratch::new("save");
        let first = save("{}", &dir.0, 1_762_600_000).unwrap();
        let second = save("{}", &dir.0, 1_762_600_000).unwrap();
        assert_eq!(
            first.file_name().unwrap().to_str().unwrap(),
            "transitlink-backup-2025-11-08.json"
        );
        assert_eq!(
            second.file_name().unwrap().to_str().unwrap(),
            "transitlink-backup-2025-11-08-2.json"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&first).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
