//! 一键体检：查终端里的代理、相关的站点（连不连得上、延迟、登录、余额），以及每个用 TransitLink
//! 配置过的工具（配置还在不在、有没有被终端里的环境变量盖住或者换了位置、Key、分组、模型、延迟）。
//! 只读不改；修复由界面调用现有的命令（重新配置、重新登录）。结果里没有 Key，也没有环境变量的值：
//! 只有变量名、设置它的文件和行号；例外是代理地址（去掉了用户名和密码）和由目录变量算出来的配置文件位置
//! （主目录写成 `~`），它们是告诉用户去哪里改的。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use futures_util::future::join_all;
use serde::Serialize;

use super::{Finding, Fix, Level};
use crate::accounts::Accounts;
use crate::configure::probe::TestResult;
use crate::configure::{ConfigState, Configurator, Diagnosis};
use crate::settings::AppSettings;
use crate::sites::{GroupSummary, KeySummary, Platform, SiteError};
use crate::tools::{self, ToolKind, ToolStatus};

/// 站点超过这么久才响应算慢。
const SLOW_SITE_MS: u64 = 3000;
/// 拉模型列表超过这么久算慢（要经过站点转发，比站点自己的接口慢一些）。
const SLOW_TOOL_MS: u64 = 5000;

/// 工具发请求用的代理（按 curl 的顺序，小写优先）。
const PROXY_VARS: [&str; 4] = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"];
/// 不走代理的地址。
const NO_PROXY_VARS: [&str; 2] = ["no_proxy", "NO_PROXY"];

/// 决定工具配置文件位置的环境变量（见 `ToolKind::config_path` 和各工具的 `files`）。
/// 测试保证这里列全了。
const PATH_VARS: [&str; 16] = [
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "GEMINI_CLI_HOME",
    "GROK_HOME",
    "XDG_CONFIG_HOME",
    "DSH_HOME",
    "PI_CODING_AGENT_DIR",
    "OMP_PROFILE",
    "PI_PROFILE",
    "PI_CONFIG_DIR",
    "KIMI_CODE_HOME",
    "MINIMAX_DATA_DIR",
    "MAVIS_DATA_DIR",
    "OPENCLAW_CONFIG_PATH",
    "OPENCLAW_STATE_DIR",
    "OPENCLAW_HOME",
];

/// Claude Code 的 settings.json 里的 env 会盖过终端里的同名变量（2.1.319 核实），但这几个开关我们不写：
/// 终端里打开了，请求就发到云服务商，不发到站点。
const CLAUDE_SWITCHES: [&str; 7] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_GATEWAY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
];

/// Gemini CLI 读 `.env` 时跳过终端里已经有的变量：终端里的地址、Key 和模型说了算。
const GEMINI_VARS: [&str; 3] = ["GOOGLE_GEMINI_BASE_URL", "GEMINI_API_KEY", "GEMINI_MODEL"];

/// Kimi Code：环境变量 `KIMI_MODEL_NAME` 盖过配置里的模型。
const KIMI_VARS: [&str; 1] = ["KIMI_MODEL_NAME"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkup {
    pub checked_at: u64,
    /// 读到了终端里的环境变量（读不到时没查代理和环境变量）。
    pub shell_checked: bool,
    /// 终端里设置的代理（工具发请求走它）。
    pub proxy: Option<ProxyCheck>,
    pub sites: Vec<SiteCheck>,
    pub tools: Vec<ToolCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyCheck {
    /// 设置代理的变量名。
    pub name: String,
    /// 代理地址（去掉了用户名和密码）。
    pub url: String,
    pub reachable: bool,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteCheck {
    pub site_id: String,
    pub name: String,
    pub host: String,
    pub latency_ms: Option<u64>,
    pub balance_usd: Option<f64>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCheck {
    pub kind: ToolKind,
    pub installed: bool,
    pub version: Option<String>,
    pub site_id: String,
    pub site_name: String,
    pub key_name: String,
    pub group: Option<String>,
    pub model: Option<String>,
    pub latency_ms: Option<u64>,
    /// 站点给这个 Key 的模型数、其中适合这个工具的（测试通过时）。
    pub models: Option<usize>,
    pub family: Option<usize>,
    pub findings: Vec<Finding>,
}

/// 要从终端里读的环境变量。
pub fn shell_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = PROXY_VARS
        .iter()
        .chain(&NO_PROXY_VARS)
        .chain(&PATH_VARS)
        .chain(&CLAUDE_SWITCHES)
        .chain(&GEMINI_VARS)
        .chain(&KIMI_VARS)
        .copied()
        .collect();
    names.dedup();
    names
}

/// 站点那边查到的东西：给界面的结果，加上查工具时要用的平台、Key 和分组列表。
struct SiteFacts {
    check: SiteCheck,
    platform: Platform,
    keys: Option<Vec<KeySummary>>,
    groups: Vec<GroupSummary>,
}

pub async fn run(
    accounts: &Accounts,
    configurator: &Configurator,
    statuses: Vec<ToolStatus>,
    shell: Option<HashMap<String, String>>,
    settings: &AppSettings,
) -> Checkup {
    let home = configurator.home().to_path_buf();
    let applied = configurator.applied_all();
    let overview = configurator.overview(statuses);

    // 查哪些站点：工具在用的，加上当前站点；都没有时查所有站点。
    let known: Vec<String> = accounts
        .sites()
        .unwrap_or_default()
        .into_iter()
        .map(|site| site.id)
        .collect();
    let mut wanted: Vec<String> = Vec::new();
    for id in applied
        .iter()
        .map(|(_, applied)| applied.site_id.clone())
        .chain(settings.current_site.clone())
    {
        if known.contains(&id) && !wanted.contains(&id) {
            wanted.push(id);
        }
    }
    if wanted.is_empty() {
        wanted = known;
    }

    let below = settings.alerts.balance_below;
    let proxy = shell.as_ref().and_then(proxy_setting);
    let configured: Vec<ToolKind> = applied
        .iter()
        .map(|(kind, _)| *kind)
        .filter(|kind| {
            overview
                .iter()
                .any(|tool| tool.status.kind == *kind && tool.state == ConfigState::Configured)
        })
        .collect();
    let (sites, diagnoses, proxy) = tokio::join!(
        join_all(wanted.iter().map(|id| check_site(accounts, id))),
        join_all(configured.iter().map(|kind| configurator.diagnose(*kind))),
        check_proxy(proxy, &home),
    );
    let mut sites: HashMap<String, SiteFacts> = sites
        .into_iter()
        .flatten()
        .map(|facts| (facts.check.site_id.clone(), facts))
        .collect();
    // 余额：订阅分组的 Key 不扣余额，工具都用订阅时余额是 0 也不算问题。
    for facts in sites.values_mut() {
        let Some(balance) = facts.check.balance_usd else {
            continue;
        };
        let tool_groups: Vec<Option<String>> = applied
            .iter()
            .filter(|(_, applied)| applied.site_id == facts.check.site_id)
            .map(|(_, applied)| {
                facts
                    .keys
                    .as_ref()
                    .and_then(|keys| keys.iter().find(|key| key.id == applied.key_id))
                    .and_then(|key| key.group.clone())
            })
            .collect();
        if super::balance_applies(&facts.groups, &tool_groups) {
            facts
                .check
                .findings
                .extend(super::balance(&facts.check.site_id, balance, below));
            facts
                .check
                .findings
                .sort_by_key(|finding| std::cmp::Reverse(finding.level));
        }
    }
    // 代理连不上，但站点都不走代理（NO_PROXY）：不影响连站点。
    let proxy = proxy.map(|mut proxy| {
        let bypass = shell.as_ref().and_then(|shell| {
            NO_PROXY_VARS
                .iter()
                .find_map(|name| shell.get(*name).filter(|value| !value.trim().is_empty()))
        });
        if !proxy.reachable
            && let Some(bypass) = bypass
            && !sites.is_empty()
            && sites
                .values()
                .all(|facts| bypassed(bypass, &facts.check.host))
        {
            for finding in &mut proxy.findings {
                if finding.code == "proxyDown" {
                    finding.level = Level::Warn;
                    finding.code = "proxyDownBypassed";
                }
            }
        }
        proxy
    });
    let diagnoses: Vec<(ToolKind, Diagnosis)> = configured
        .into_iter()
        .zip(diagnoses)
        .filter_map(|(kind, diagnosis)| Some((kind, diagnosis.ok()?)))
        .collect();

    let now = i64::try_from(crate::clock::now_secs()).unwrap_or(i64::MAX);
    let tools = applied
        .into_iter()
        .map(|(kind, applied)| {
            let tool = overview.iter().find(|tool| tool.status.kind == kind);
            let state = tool.map_or(ConfigState::Invalid, |tool| tool.state);
            let installed = tool.is_some_and(|tool| tool.status.installed);
            let diagnosis = diagnoses
                .iter()
                .find(|(tested, _)| *tested == kind)
                .map(|(_, diagnosis)| diagnosis);
            let site = sites.get(&applied.site_id);
            let mut findings = Vec::new();
            if !installed {
                findings.push(Finding::new(Level::Warn, "notInstalled").fix(Fix::Install { kind }));
            }
            let reapply = Fix::Reapply {
                kind,
                site_id: applied.site_id.clone(),
            };
            match state {
                // 可能是用户自己改的（例如换了别的服务）：只提醒，点了才改回来。
                ConfigState::Changed => {
                    findings.push(Finding::new(Level::Warn, "configChanged").fix(reapply.clone()))
                }
                ConfigState::Invalid => findings.push(
                    Finding::new(Level::Error, "configInvalid").param(
                        "path",
                        tool.map(|tool| display_path(&home, Path::new(&tool.status.config_path)))
                            .unwrap_or_default(),
                    ),
                ),
                _ => {}
            }
            if let Some(shell) = &shell {
                findings.extend(moved(configurator, kind, shell, &home));
                findings.extend(overridden(configurator, kind, shell, &home));
            }
            let site_known =
                sites.contains_key(&applied.site_id) || accounts.site(&applied.site_id).is_ok();
            if !site_known {
                findings.push(Finding::new(Level::Warn, "siteRemoved"));
            }
            // Key 和分组：站点那边登录着才查得到。
            let mut key_broken = false;
            if let Some(keys) = site.and_then(|site| site.keys.as_deref()) {
                if let Some(finding) = super::tool_key(
                    kind,
                    &applied.site_id,
                    &applied.key_id,
                    &applied.key_name,
                    keys,
                    now,
                    below,
                ) {
                    key_broken = finding.level == Level::Error;
                    findings.push(finding);
                }
                if let Some(key) = keys.iter().find(|key| key.id == applied.key_id)
                    && let Some(finding) = super::group_denied(
                        kind,
                        &applied.site_id,
                        key,
                        site.map(|site| site.groups.as_slice()).unwrap_or_default(),
                    )
                {
                    findings.push(finding);
                }
            }
            if let Some(diagnosis) = diagnosis {
                let balance_empty = site.is_some_and(|site| {
                    site.check
                        .findings
                        .iter()
                        .any(|finding| finding.code == "balanceEmpty")
                });
                let platform = site.map(|site| site.platform).or_else(|| {
                    accounts
                        .site(&applied.site_id)
                        .ok()
                        .map(|record| record.info.platform)
                });
                findings.extend(test_findings(
                    kind,
                    &applied.site_id,
                    diagnosis,
                    TestContext {
                        key_broken,
                        balance_empty,
                        platform,
                    },
                ));
            }
            // 站点删掉了：重新配置、换分组都用不了（要先在一键配置里换个站点）。
            if !site_known {
                for finding in &mut findings {
                    if matches!(finding.fix, Some(Fix::Reapply { .. } | Fix::Setup { .. })) {
                        finding.fix = None;
                    }
                }
            }
            findings.sort_by_key(|finding| std::cmp::Reverse(finding.level));
            ToolCheck {
                kind,
                installed,
                version: tool.and_then(|tool| tool.status.version.clone()),
                site_id: applied.site_id.clone(),
                site_name: site
                    .map(|site| site.check.name.clone())
                    .unwrap_or(applied.site_name),
                key_name: applied.key_name,
                group: applied.group,
                model: diagnosis
                    .and_then(|diagnosis| diagnosis.model.clone())
                    .or(applied.model),
                latency_ms: diagnosis.map(|diagnosis| diagnosis.elapsed_ms),
                models: diagnosis.and_then(|diagnosis| match diagnosis.test {
                    TestResult::Ok { models, .. } | TestResult::NoFamilyModels { models } => {
                        Some(models)
                    }
                    _ => None,
                }),
                family: diagnosis.and_then(|diagnosis| match diagnosis.test {
                    TestResult::Ok { family, .. } => Some(family),
                    _ => None,
                }),
                findings,
            }
        })
        .collect();

    // 站点按查的顺序排（工具在用的在前）。
    let mut sites: Vec<SiteFacts> = sites.into_values().collect();
    sites.sort_by_key(|facts| wanted.iter().position(|id| *id == facts.check.site_id));
    Checkup {
        checked_at: crate::clock::now_secs(),
        shell_checked: shell.is_some(),
        proxy,
        sites: sites.into_iter().map(|facts| facts.check).collect(),
        tools,
    }
}

/// 一个站点：连不连得上、多快、登录还在不在、余额；登录着的话顺便拿 Key 和分组（查工具用）。
async fn check_site(accounts: &Accounts, id: &str) -> Option<SiteFacts> {
    let record = accounts.site(id).ok()?;
    let host = url::Url::parse(&record.info.base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| record.info.base_url.clone());
    let name = record
        .config
        .as_ref()
        .map(|config| config.brand.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| record.info.name.clone());
    let platform = record.info.platform;
    let mut check = SiteCheck {
        site_id: id.to_string(),
        name,
        host,
        latency_ms: None,
        balance_usd: None,
        findings: Vec::new(),
    };
    match accounts.ping(id).await {
        Ok(ms) => {
            check.latency_ms = Some(ms);
            if ms > SLOW_SITE_MS {
                check
                    .findings
                    .push(Finding::new(Level::Warn, "siteSlow").param("ms", ms.to_string()));
            }
        }
        Err(error) => {
            check.findings.push(with_detail(
                Finding::new(Level::Error, "siteUnreachable"),
                &error,
            ));
            // 连不上就不用再试登录和余额了（每个请求都要等超时）。
            if error == SiteError::Unreachable {
                return Some(SiteFacts {
                    check,
                    platform,
                    keys: None,
                    groups: Vec::new(),
                });
            }
        }
    }
    let mut keys = None;
    let mut groups = Vec::new();
    match accounts.account(id).await {
        Ok(account) => {
            // 余额要不要提醒，等知道工具用的是不是订阅分组再说（见 run）。
            check.balance_usd = Some(account.balance_usd);
            let (listed, usable) = tokio::join!(accounts.keys(id), accounts.groups(id));
            keys = listed.ok();
            groups = usable.unwrap_or_default();
        }
        Err(SiteError::SessionExpired) => check.findings.push(
            Finding::new(Level::Error, "sessionExpired").fix(Fix::Relogin {
                site_id: id.to_string(),
            }),
        ),
        Err(error) => check
            .findings
            .push(with_detail(Finding::new(Level::Warn, "siteError"), &error)),
    }
    check
        .findings
        .sort_by_key(|finding| std::cmp::Reverse(finding.level));
    Some(SiteFacts {
        check,
        platform,
        keys,
        groups,
    })
}

/// 站点自己的说明放 `message`；没有的话放错误代码 `code`（界面换成文案），连不上本身就不用再说一遍。
fn with_detail(finding: Finding, error: &SiteError) -> Finding {
    match error.message() {
        Some(message) => finding.param("message", short(&message)),
        None if *error != SiteError::Unreachable => finding.param("code", error.code()),
        None => finding,
    }
}

/// `NO_PROXY` 里有没有这个主机（`*`、完整主机名，或者以 `.example.com`、`example.com` 结尾）。
fn bypassed(no_proxy: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    no_proxy.split(',').map(str::trim).any(|entry| {
        let entry = entry
            .trim_start_matches("*.")
            .trim_start_matches('.')
            .to_ascii_lowercase();
        entry == "*"
            || (!entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}"))))
    })
}

/// 判断测试结果时要考虑的：Key 已经查出问题（不再重复报）、余额用完了（站点拒绝 Key 是因为没钱）、
/// 站点平台（sub2api 没配模型映射的分组给的是默认模型列表，不一定全）。
struct TestContext {
    key_broken: bool,
    balance_empty: bool,
    platform: Option<Platform>,
}

/// 用工具配置里的 Key 拉模型列表的结果。Key 已经查出停用之类的问题、或者余额用完了时，不再重复报
/// 「Key 被拒绝」「连不上」（额度用完的 Key，sub2api 回 429）。
fn test_findings(
    kind: ToolKind,
    site_id: &str,
    diagnosis: &Diagnosis,
    context: TestContext,
) -> Vec<Finding> {
    let reapply = Fix::Reapply {
        kind,
        site_id: site_id.to_string(),
    };
    let mut findings = Vec::new();
    match &diagnosis.test {
        TestResult::BadKey { message } if !context.key_broken && !context.balance_empty => {
            let finding = Finding::new(Level::Error, "keyRejected").fix(reapply);
            findings.push(match message {
                Some(message) => finding.param("message", short(message)),
                None => finding,
            });
        }
        TestResult::BadKey { .. } => {}
        TestResult::Failed { .. } if context.key_broken => {}
        TestResult::Failed { message } => {
            let finding = Finding::new(Level::Error, "testFailed");
            findings.push(match message {
                Some(message) => finding.param("message", short(message)),
                None => finding,
            });
        }
        TestResult::NoFamilyModels { .. } => {
            findings.push(Finding::new(Level::Error, "noModels").fix(Fix::Setup {
                kind,
                site_id: site_id.to_string(),
            }))
        }
        TestResult::Ok { .. } => {
            if let Some(model) = missing_model(kind, diagnosis.model.as_deref(), &diagnosis.ids) {
                // new-api 的列表就是这个 Key 能用的模型；sub2api 可能只是默认列表，只提醒。
                let level = if context.platform == Some(Platform::Newapi) {
                    Level::Error
                } else {
                    Level::Warn
                };
                findings.push(
                    Finding::new(level, "modelMissing")
                        .param("model", model)
                        .fix(reapply),
                );
            }
            if diagnosis.elapsed_ms > SLOW_TOOL_MS {
                findings.push(
                    Finding::new(Level::Warn, "toolSlow")
                        .param("ms", diagnosis.elapsed_ms.to_string()),
                );
            }
        }
    }
    findings
}

/// 配置里的模型站点上已经没有了（下架或者改名了）。Claude Code 的 `opus`、`sonnet` 这类别名和
/// `[1m]` 后缀由它自己处理，不算。
fn missing_model(kind: ToolKind, model: Option<&str>, ids: &[String]) -> Option<String> {
    let model = model?.trim();
    if model.is_empty() || ids.is_empty() {
        return None;
    }
    let id = if kind == ToolKind::ClaudeCode {
        let id = model.split('[').next().unwrap_or(model);
        if !id.contains('-') {
            return None;
        }
        id
    } else {
        model
    };
    (!ids.iter().any(|known| known == id)).then(|| model.to_string())
}

/// 站点返回的说明可能很长，截一下。
fn short(message: &str) -> String {
    let message = message.trim();
    if message.chars().count() <= 200 {
        message.to_string()
    } else {
        format!("{}…", message.chars().take(200).collect::<String>())
    }
}

/// 终端里的环境变量让工具去别的地方读配置（例如 `CODEX_HOME`）：TransitLink 写的那份工具看不到。
/// macOS 上从程序坞打开的 TransitLink 拿不到终端里设置的变量，所以两边可能不一样。
fn moved(
    configurator: &Configurator,
    kind: ToolKind,
    shell: &HashMap<String, String>,
    home: &Path,
) -> Option<Finding> {
    let asked = RefCell::new(Vec::<String>::new());
    let lookup: &dyn Fn(&str) -> Option<String> = &|name: &str| {
        asked.borrow_mut().push(name.to_string());
        if PATH_VARS.contains(&name) {
            shell.get(name).cloned()
        } else {
            configurator.env_var(name)
        }
    };
    let theirs = configurator.config_files(kind, Some(lookup));
    let ours = configurator.config_files(kind, None);
    if theirs == ours {
        return None;
    }
    let mut seen = HashSet::new();
    let name = asked.into_inner().into_iter().find(|name| {
        seen.insert(name.clone())
            && shell.get(name).map(String::as_str) != configurator.env_var(name).as_deref()
    })?;
    Some(
        Finding::new(Level::Error, "configMoved")
            .param("name", &name)
            .param("path", display_path(home, theirs.first()?))
            .param("ours", display_path(home, ours.first()?))
            .param("where", tools::rc_locations(home, &name).join(", ")),
    )
}

/// 终端里设置了会盖过 TransitLink 配置的环境变量。
fn overridden(
    configurator: &Configurator,
    kind: ToolKind,
    shell: &HashMap<String, String>,
    home: &Path,
) -> Vec<Finding> {
    let set = |name: &str| {
        shell
            .get(name)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    let finding = |level: Level, code: &'static str, name: &str| {
        Finding::new(level, code)
            .param("name", name)
            .param("where", tools::rc_locations(home, name).join(", "))
    };
    match kind {
        ToolKind::ClaudeCode => CLAUDE_SWITCHES
            .iter()
            .filter(|name| {
                set(name).is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "on"
                    )
                })
            })
            .map(|name| finding(Level::Error, "envProvider", name))
            .collect(),
        ToolKind::GeminiCli => {
            let current = configurator.current(kind).unwrap_or_default();
            let ours = [current.base_url, current.token, current.model];
            GEMINI_VARS
                .iter()
                .zip(ours)
                .filter(|(name, ours)| {
                    // 设成空的地址或 Key 也算：Gemini CLI 照样跳过 .env 里的值。
                    let value = if **name == "GEMINI_MODEL" {
                        set(name)
                    } else {
                        shell.get(**name).map(|value| value.trim())
                    };
                    value.is_some_and(|value| {
                        Some(value.trim_end_matches('/'))
                            != ours.as_deref().map(|ours| ours.trim_end_matches('/'))
                    })
                })
                .map(|(name, _)| {
                    if *name == "GEMINI_MODEL" {
                        finding(Level::Warn, "envModel", name)
                    } else {
                        finding(Level::Error, "envOverride", name)
                    }
                })
                .collect()
        }
        ToolKind::KimiCode => {
            let ours = configurator.current(kind).unwrap_or_default().model;
            KIMI_VARS
                .iter()
                .filter(|name| {
                    set(name).is_some_and(|value| {
                        ours.as_deref().is_none_or(|ours| {
                            value != ours && value != format!("transitlink/{ours}")
                        })
                    })
                })
                .map(|name| finding(Level::Warn, "envModel", name))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// 终端里设置的代理：第一个非空的变量。
fn proxy_setting(shell: &HashMap<String, String>) -> Option<(String, String)> {
    PROXY_VARS.iter().find_map(|name| {
        let value = shell.get(*name)?.trim();
        (!value.is_empty()).then(|| (name.to_string(), value.to_string()))
    })
}

/// 解析代理地址（可以不写协议），返回去掉用户名密码的地址、主机和端口。
fn parse_proxy(value: &str) -> Option<(String, String, u16)> {
    let text = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let mut url = url::Url::parse(&text).ok()?;
    let host = url.host_str()?.trim_matches(['[', ']']).to_string();
    let port = url
        .port_or_known_default()
        .or_else(|| url.scheme().starts_with("socks").then_some(1080))?;
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Some((url.as_str().trim_end_matches('/').to_string(), host, port))
}

/// 代理连不连得上（只连 TCP，不经过它发请求）。
async fn check_proxy(setting: Option<(String, String)>, home: &Path) -> Option<ProxyCheck> {
    let (name, value) = setting?;
    let Some((url, host, port)) = parse_proxy(&value) else {
        return Some(ProxyCheck {
            findings: vec![
                Finding::new(Level::Error, "proxyInvalid")
                    .param("name", &name)
                    .param("where", tools::rc_locations(home, &name).join(", ")),
            ],
            name,
            url: String::new(),
            reachable: false,
        });
    };
    let reachable = tauri::async_runtime::spawn_blocking(move || {
        use std::net::{TcpStream, ToSocketAddrs};
        (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .into_iter()
            .flatten()
            .any(|address| TcpStream::connect_timeout(&address, Duration::from_secs(3)).is_ok())
    })
    .await
    .unwrap_or(false);
    let findings = if reachable {
        Vec::new()
    } else {
        vec![
            Finding::new(Level::Error, "proxyDown")
                .param("name", &name)
                .param("url", &url)
                .param("where", tools::rc_locations(home, &name).join(", ")),
        ]
    };
    Some(ProxyCheck {
        name,
        url,
        reachable,
        findings,
    })
}

/// 报告里的路径：主目录写成 `~`。
fn display_path(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;
    use crate::sites::http::Http;
    use crate::sites::{Credential, Platform, SiteInfo};
    use crate::store::SiteStore;
    use serde_json::{Value, json};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({ "code": 0, "message": "success", "data": data }))
    }

    fn models(ids: &[&str]) -> ResponseTemplate {
        let data: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
        ResponseTemplate::new(200).set_body_json(json!({ "object": "list", "data": data }))
    }

    async fn mount(server: &MockServer, balance: f64, key_status: &str, listed: &[&str]) {
        mount_with(server, balance, key_status, listed, "standard").await;
    }

    async fn mount_with(
        server: &MockServer,
        balance: f64,
        key_status: &str,
        listed: &[&str],
        group_type: &str,
    ) {
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/settings/public"))
            .respond_with(ok(
                json!({ "site_name": "Relay", "registration_enabled": true }),
            ))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/auth/me"))
            .respond_with(ok(json!({ "email": "a@example.com", "balance": balance })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/groups/available"))
            .respond_with(ok(json!([
                { "id": 2, "name": "Codex", "platform": "openai", "rate_multiplier": 1,
                  "subscription_type": group_type }
            ])))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/keys"))
            .respond_with(ok(json!({ "items": [
                { "id": 7, "name": "TransitLink Codex", "key": "sk-codex-secret", "group_id": 2,
                  "group": { "id": 2, "name": "Codex" }, "status": key_status }
            ] })))
            .mount(server)
            .await;
        // sub2api：停用的 Key 回 401，额度用完的回 429。
        let answer = match key_status {
            "active" => models(listed),
            "quota_exhausted" => ResponseTemplate::new(429)
                .set_body_json(json!({ "error": { "message": "API key 额度已用完" } })),
            _ => ResponseTemplate::new(401)
                .set_body_json(json!({ "error": { "message": "API key is disabled" } })),
        };
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer sk-codex-secret"))
            .respond_with(answer)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn checks_sites_keys_models_and_the_terminal() {
        let dir = std::env::temp_dir().join(format!("tl-checkup-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let server = MockServer::start().await;
        mount(&server, 5.0, "active", &["gpt-5.5", "gpt-6.1-sol"]).await;
        let accounts = Accounts::new(
            Http::new(),
            SiteStore::new(dir.join("data/sites.json")),
            Box::new(MemoryStore::default()),
        );
        let info = SiteInfo {
            platform: Platform::Sub2api,
            base_url: server.uri(),
            name: "Relay".into(),
            logo_url: None,
            captcha_required: false,
            password_login_disabled: false,
            email_only: true,
            quota_per_unit: None,
            password_encryption: false,
            user_agent: None,
        };
        let credential = Credential::Sub2api {
            access_token: "at".into(),
            refresh_token: "rt".into(),
        };
        let site = accounts.save(info, "a@example.com", &credential).unwrap();
        let configurator = Configurator::new(
            Http::new(),
            home.clone(),
            dir.join("data"),
            Box::new(|_| None),
        );
        configurator
            .apply(
                &accounts,
                ToolKind::Codex,
                &site.id,
                Some("2"),
                Some("gpt-5.5"),
            )
            .await
            .unwrap();
        let status = || {
            let path = ToolKind::Codex.config_path(&home, &|_| None);
            vec![ToolStatus {
                kind: ToolKind::Codex,
                installed: true,
                binary_path: None,
                version: Some("0.166.0".into()),
                config_exists: true,
                config_path: path.display().to_string(),
                install_command: String::new(),
                install_npm: false,
                configurable: true,
            }]
        };
        let settings = AppSettings::default();

        // 一切正常。
        let healthy = run(
            &accounts,
            &configurator,
            status(),
            Some(HashMap::new()),
            &settings,
        )
        .await;
        assert_eq!(healthy.sites.len(), 1);
        assert_eq!(healthy.sites[0].balance_usd, Some(5.0));
        assert!(healthy.sites[0].latency_ms.is_some());
        assert!(
            healthy.sites[0].findings.is_empty(),
            "{:?}",
            healthy.sites[0].findings
        );
        let tool = &healthy.tools[0];
        assert_eq!(tool.model.as_deref(), Some("gpt-5.5"));
        assert_eq!((tool.models, tool.family), (Some(2), Some(2)));
        assert!(tool.findings.is_empty(), "{:?}", tool.findings);
        assert!(healthy.proxy.is_none());

        // 余额不多了、Key 被停用、终端里的 CODEX_HOME 指到别处、代理连不上。
        mount(&server, 1.2, "disabled", &[]).await;
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let shell = HashMap::from([
            ("CODEX_HOME".to_string(), "/work/codex".to_string()),
            (
                "HTTPS_PROXY".to_string(),
                format!("http://u:p@127.0.0.1:{closed}"),
            ),
        ]);
        let sick = run(&accounts, &configurator, status(), Some(shell), &settings).await;
        let codes = |findings: &[Finding]| {
            findings
                .iter()
                .map(|finding| finding.code)
                .collect::<Vec<_>>()
        };
        assert_eq!(codes(&sick.sites[0].findings), ["balanceLow"]);
        let tool = &sick.tools[0];
        assert_eq!(codes(&tool.findings), ["configMoved", "keyDisabled"]);
        assert_eq!(
            tool.findings[1].fix,
            Some(Fix::Reapply {
                kind: ToolKind::Codex,
                site_id: site.id.clone()
            })
        );
        let proxy = sick.proxy.as_ref().unwrap();
        assert!(!proxy.reachable);
        assert_eq!(proxy.url, format!("http://127.0.0.1:{closed}"));
        let json = serde_json::to_string(&sick).unwrap();
        assert!(!json.contains("sk-codex-secret"), "{json}");
        assert!(!json.contains("u:p@"), "{json}");

        // 模型不在列表里：sub2api 的列表可能只是默认列表，只提醒。
        mount(&server, 5.0, "active", &["gpt-6.1-sol"]).await;
        let stale = run(&accounts, &configurator, status(), None, &settings).await;
        assert!(!stale.shell_checked);
        assert_eq!(codes(&stale.tools[0].findings), ["modelMissing"]);
        assert_eq!(stale.tools[0].findings[0].level, Level::Warn);
        assert_eq!(stale.tools[0].findings[0].params["model"], "gpt-5.5");

        // 额度用完的 Key：站点回 429，只报「额度用完」，不再报「连不上」。
        mount(&server, 5.0, "quota_exhausted", &[]).await;
        let exhausted = run(&accounts, &configurator, status(), None, &settings).await;
        assert_eq!(codes(&exhausted.tools[0].findings), ["keyExhausted"]);

        // 订阅分组：余额是 0 也照样能用，不报余额。
        mount_with(&server, 0.0, "active", &["gpt-5.5"], "subscription").await;
        let subscribed = run(&accounts, &configurator, status(), None, &settings).await;
        assert!(
            subscribed.sites[0].findings.is_empty(),
            "{:?}",
            subscribed.sites[0].findings
        );
        mount_with(&server, 0.0, "active", &["gpt-5.5"], "standard").await;
        let broke = run(&accounts, &configurator, status(), None, &settings).await;
        assert_eq!(codes(&broke.sites[0].findings), ["balanceEmpty"]);

        // 站点从 TransitLink 删掉了：不再查站点，但工具的配置还在、照样测试（模型还是没有）。
        mount(&server, 5.0, "active", &["gpt-6.1-sol"]).await;
        accounts.remove(&site.id).unwrap();
        let removed = run(&accounts, &configurator, status(), None, &settings).await;
        assert!(removed.sites.is_empty());
        assert_eq!(
            codes(&removed.tools[0].findings),
            ["siteRemoved", "modelMissing"]
        );
        assert!(
            removed.tools[0]
                .findings
                .iter()
                .all(|finding| finding.fix.is_none()),
            "can't set up again on a removed site"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_vars_cover_every_variable_the_tools_read() {
        let home = std::env::temp_dir().join("tl-checkup-vars");
        let configurator = Configurator::new(
            Http::new(),
            home.clone(),
            home.join(".transitlink"),
            Box::new(|_| None),
        );
        for &kind in ToolKind::ALL.iter() {
            let asked = RefCell::new(Vec::<String>::new());
            let lookup: &dyn Fn(&str) -> Option<String> = &|name: &str| {
                asked.borrow_mut().push(name.to_string());
                // 每个变量都设上，走到所有分支（例如 OMP 的配置档）。
                Some("/data/x".to_string())
            };
            configurator.config_files(kind, Some(lookup));
            for name in asked.into_inner() {
                assert!(PATH_VARS.contains(&name.as_str()), "{kind:?} reads {name}");
            }
        }
    }

    #[test]
    fn moved_config_names_the_variable() {
        let home = std::env::temp_dir().join("tl-checkup-moved");
        let configurator = Configurator::new(
            Http::new(),
            home.clone(),
            home.join(".transitlink"),
            Box::new(|_| None),
        );
        let shell = HashMap::from([("CODEX_HOME".to_string(), "/work/codex".to_string())]);
        let finding = moved(&configurator, ToolKind::Codex, &shell, &home).unwrap();
        assert_eq!(finding.code, "configMoved");
        assert_eq!(finding.params["name"], "CODEX_HOME");
        assert_eq!(finding.params["path"], "/work/codex/config.toml");
        assert_eq!(finding.params["ours"], "~/.codex/config.toml");
        assert_eq!(
            moved(&configurator, ToolKind::ClaudeCode, &shell, &home),
            None
        );
        assert_eq!(
            moved(&configurator, ToolKind::Codex, &HashMap::new(), &home),
            None
        );
    }

    #[test]
    fn claude_provider_switches_count_only_when_on() {
        let home = std::env::temp_dir().join("tl-checkup-switch");
        let configurator = Configurator::new(
            Http::new(),
            home.clone(),
            home.join(".transitlink"),
            Box::new(|_| None),
        );
        let shell = HashMap::from([
            ("CLAUDE_CODE_USE_BEDROCK".to_string(), "1".to_string()),
            ("CLAUDE_CODE_USE_VERTEX".to_string(), "0".to_string()),
            (
                "ANTHROPIC_BASE_URL".to_string(),
                "https://elsewhere".to_string(),
            ),
        ]);
        let found = overridden(&configurator, ToolKind::ClaudeCode, &shell, &home);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].code, found[0].params["name"].as_str()),
            ("envProvider", "CLAUDE_CODE_USE_BEDROCK")
        );
        let kimi = HashMap::from([("KIMI_MODEL_NAME".to_string(), "kimi-k3".to_string())]);
        assert_eq!(
            overridden(&configurator, ToolKind::KimiCode, &kimi, &home)[0].code,
            "envModel"
        );
    }

    #[test]
    fn missing_model_ignores_aliases_and_suffixes() {
        let ids = vec!["claude-sonnet-5-5".to_string(), "gpt-6.1-sol".to_string()];
        let missing = |kind, model| missing_model(kind, Some(model), &ids);
        assert_eq!(missing(ToolKind::ClaudeCode, "claude-sonnet-5-5[1m]"), None);
        assert_eq!(missing(ToolKind::ClaudeCode, "opus"), None);
        assert_eq!(
            missing(ToolKind::ClaudeCode, "claude-opus-4-1").as_deref(),
            Some("claude-opus-4-1")
        );
        assert_eq!(missing(ToolKind::Codex, "gpt-6.1-sol"), None);
        assert_eq!(
            missing(ToolKind::Codex, "gpt-5.5").as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(missing_model(ToolKind::Codex, Some("gpt-5.5"), &[]), None);
    }

    #[test]
    fn no_proxy_entries_match_hosts_and_subdomains() {
        assert!(bypassed("localhost, .example.com", "relay.example.com"));
        assert!(bypassed("example.com", "example.com"));
        assert!(bypassed("*.example.com", "api.example.com"));
        assert!(bypassed("*", "anything.dev"));
        assert!(!bypassed("example.com", "notexample.com"));
        assert!(!bypassed("", "relay.example.com"));
    }

    #[test]
    fn proxy_addresses_lose_their_credentials() {
        assert_eq!(
            parse_proxy("http://user:pa55@127.0.0.1:7890"),
            Some(("http://127.0.0.1:7890".into(), "127.0.0.1".into(), 7890))
        );
        assert_eq!(
            parse_proxy("127.0.0.1:7891"),
            Some(("http://127.0.0.1:7891".into(), "127.0.0.1".into(), 7891))
        );
        assert_eq!(
            parse_proxy("socks5://localhost"),
            Some(("socks5://localhost".into(), "localhost".into(), 1080))
        );
        let shell = HashMap::from([
            ("HTTPS_PROXY".to_string(), "http://b:2".to_string()),
            ("https_proxy".to_string(), " ".to_string()),
            ("ALL_PROXY".to_string(), "http://c:3".to_string()),
        ]);
        assert_eq!(
            proxy_setting(&shell),
            Some(("HTTPS_PROXY".into(), "http://b:2".into()))
        );
    }

    #[tokio::test]
    async fn a_closed_port_is_reported_as_down() {
        // 先占一个端口再放掉，确保没人在听。
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let home = std::env::temp_dir();
        let down = check_proxy(
            Some(("HTTPS_PROXY".into(), format!("http://127.0.0.1:{port}"))),
            &home,
        )
        .await
        .unwrap();
        assert!(!down.reachable);
        assert_eq!(down.findings[0].code, "proxyDown");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let open = listener.local_addr().unwrap().port();
        let up = check_proxy(
            Some(("HTTPS_PROXY".into(), format!("127.0.0.1:{open}"))),
            &home,
        )
        .await
        .unwrap();
        assert!(up.reachable && up.findings.is_empty());
    }
}
