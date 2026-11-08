//! 余额和 Key 提醒：打开应用时、之后每 30 分钟、改了提醒设置时，查一次工具在用的站点和当前站点的
//! 余额，以及工具用的 Key 和分组。新出现的问题发一条系统通知（同一个问题一天最多一次），应用里也显示。
//! 只读站点上的信息，不改任何东西。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::Finding;
use crate::accounts::Accounts;
use crate::configure::Configurator;
use crate::settings::{AppSettings, SettingsStore};
use crate::tools::ToolKind;

/// 隔多久查一次。
const INTERVAL: Duration = Duration::from_secs(30 * 60);
/// 打开应用后等一会儿再查（不和启动时的其他请求挤在一起）。
const FIRST_DELAY: Duration = Duration::from_secs(20);
/// 同一个问题隔这么久才再通知一次。
const RENOTIFY: Duration = Duration::from_secs(24 * 3600);
/// 一次最多发几条通知（其余的在应用里看）。
const MAX_NOTIFICATIONS: usize = 3;

/// 一条提醒：哪个站点（哪个工具），问题和修复办法。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// 同一个问题每次查都是同一个 ID（`balance:<站点>`、`key:<工具>`、`group:<工具>`）。
    pub id: String,
    pub site_id: String,
    pub site_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ToolKind>,
    #[serde(flatten)]
    pub finding: Finding,
}

/// 一个站点这次查的结果：None 表示没查成（连不上、登录过期、请求失败），沿用上次的提醒。
#[derive(Debug, Clone, PartialEq)]
pub struct SiteOutcome {
    pub site_id: String,
    pub alerts: Option<Vec<Alert>>,
}

/// 现在的提醒和通知过的记录（只在内存里）。
#[derive(Default)]
pub struct AlertCenter {
    current: Mutex<Vec<Alert>>,
    /// `ID:问题` → 上次通知的时间。问题变严重了（例如余额从不多变成用完）算新问题。
    notified: Mutex<HashMap<String, Instant>>,
    /// 同一时间只查一次。
    running: tauri::async_runtime::Mutex<()>,
}

fn notify_key(alert: &Alert) -> String {
    format!("{}:{}", alert.id, alert.finding.code)
}

/// 这次的提醒：查成了的站点用新结果，没查成的沿用上次的（免得网络一断提醒就没了、恢复后又通知一遍）。
/// 不再查的站点（删掉了、工具换走了）的提醒去掉。
fn merge(previous: &[Alert], outcomes: Vec<SiteOutcome>) -> Vec<Alert> {
    let mut merged = Vec::new();
    for outcome in outcomes {
        match outcome.alerts {
            Some(alerts) => merged.extend(alerts),
            None => merged.extend(
                previous
                    .iter()
                    .filter(|alert| alert.site_id == outcome.site_id)
                    .cloned(),
            ),
        }
    }
    merged.sort_by_key(|alert| std::cmp::Reverse(alert.finding.level));
    merged
}

impl AlertCenter {
    pub fn list(&self) -> Vec<Alert> {
        self.current
            .lock()
            .map(|current| current.clone())
            .unwrap_or_default()
    }

    /// 记下这次查到的提醒。返回：提醒有没有变、现在的提醒、该发通知的（新出现的、变严重的，
    /// 或者上次通知已经过了一天的）。
    fn update(&self, outcomes: Vec<SiteOutcome>, now: Instant) -> (bool, Vec<Alert>, Vec<Alert>) {
        let (changed, alerts) = match self.current.lock() {
            Ok(mut current) => {
                let alerts = merge(&current, outcomes);
                let changed = *current != alerts;
                current.clone_from(&alerts);
                (changed, alerts)
            }
            Err(_) => return (false, Vec::new(), Vec::new()),
        };
        let mut notify = Vec::new();
        if let Ok(mut notified) = self.notified.lock() {
            // 已经解决的问题忘掉：以后再出现时重新通知。
            notified.retain(|key, _| alerts.iter().any(|alert| &notify_key(alert) == key));
            for alert in &alerts {
                let key = notify_key(alert);
                let due = notified
                    .get(&key)
                    .is_none_or(|last| now.duration_since(*last) >= RENOTIFY);
                if due {
                    notified.insert(key, now);
                    notify.push(alert.clone());
                }
            }
        }
        (changed, alerts, notify)
    }
}

/// 查一遍，每个站点一个结果。关掉了提醒时没有结果（提醒全部清掉）。
pub async fn collect(
    accounts: &Accounts,
    configurator: &Configurator,
    settings: &AppSettings,
) -> Vec<SiteOutcome> {
    let options = &settings.alerts;
    if !options.enabled {
        return Vec::new();
    }
    let applied = configurator.applied_all();
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
    let now = i64::try_from(crate::clock::now_secs()).unwrap_or(i64::MAX);
    join_all(wanted.iter().map(|site_id| {
        let tools: Vec<_> = applied
            .iter()
            .filter(|(_, applied)| &applied.site_id == site_id)
            .cloned()
            .collect();
        async move {
            let outcome = |alerts: Option<Vec<Alert>>| SiteOutcome {
                site_id: site_id.clone(),
                alerts,
            };
            let Ok(record) = accounts.site(site_id) else {
                return outcome(Some(Vec::new()));
            };
            let site_name = record
                .config
                .as_ref()
                .map(|config| config.brand.name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| record.info.name.clone());
            let alert = |id: String, kind: Option<ToolKind>, finding: Finding| Alert {
                id,
                site_id: site_id.clone(),
                site_name: site_name.clone(),
                kind,
                finding,
            };
            // 登录过期、站点连不上：这次查不了，沿用上次的提醒（概览上另有登录提示）。
            let Ok(account) = accounts.account(site_id).await else {
                return outcome(None);
            };
            let keys = async {
                if tools.is_empty() {
                    Ok(Vec::new())
                } else {
                    accounts.keys(site_id).await
                }
            };
            let (keys, groups) = tokio::join!(keys, accounts.groups(site_id));
            let (Ok(keys), Ok(groups)) = (keys, groups) else {
                return outcome(None);
            };
            let mut alerts = Vec::new();
            // 订阅分组的 Key 不扣余额：工具都用订阅时，余额是 0 也不提醒。
            let tool_groups: Vec<Option<String>> = tools
                .iter()
                .map(|(_, applied)| {
                    keys.iter()
                        .find(|key| key.id == applied.key_id)
                        .and_then(|key| key.group.clone())
                })
                .collect();
            if super::balance_applies(&groups, &tool_groups)
                && let Some(finding) =
                    super::balance(site_id, account.balance_usd, options.balance_below)
            {
                alerts.push(alert(format!("balance:{site_id}"), None, finding));
            }
            if options.keys {
                for (kind, applied) in &tools {
                    if let Some(finding) = super::tool_key(
                        *kind,
                        site_id,
                        &applied.key_id,
                        &applied.key_name,
                        &keys,
                        now,
                        options.balance_below,
                    ) {
                        alerts.push(alert(format!("key:{}", kind.id()), Some(*kind), finding));
                    } else if let Some(key) = keys.iter().find(|key| key.id == applied.key_id)
                        && let Some(finding) = super::group_denied(*kind, site_id, key, &groups)
                    {
                        alerts.push(alert(format!("group:{}", kind.id()), Some(*kind), finding));
                    }
                }
            }
            outcome(Some(alerts))
        }
    }))
    .await
}

/// 查一遍、更新提醒、发通知。界面通过 `alerts-changed` 事件知道提醒变了。
pub async fn refresh(app: &AppHandle) -> Vec<Alert> {
    let center = app.state::<AlertCenter>();
    let _running = center.running.lock().await;
    let settings = app.state::<SettingsStore>().get();
    let outcomes = collect(
        &app.state::<Accounts>(),
        &app.state::<Configurator>(),
        &settings,
    )
    .await;
    let (changed, alerts, notify) = center.update(outcomes, Instant::now());
    if changed {
        let _ = app.emit("alerts-changed", &alerts);
    }
    for alert in notify.iter().take(MAX_NOTIFICATIONS) {
        let (title, body) = message(alert, &settings.language);
        send(app, &title, &body);
    }
    alerts
}

/// 后台定时查。
pub async fn watch(app: AppHandle) {
    tokio::time::sleep(FIRST_DELAY).await;
    loop {
        refresh(&app).await;
        tokio::time::sleep(INTERVAL).await;
    }
}

#[cfg(desktop)]
fn send(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg(not(desktop))]
fn send(_app: &AppHandle, _title: &str, _body: &str) {}

/// 通知的标题和正文（说人话，告诉用户打开 TransitLink 处理）。
fn message(alert: &Alert, language: &str) -> (String, String) {
    let zh = language.starts_with("zh");
    let param = |name: &str| alert.finding.params.get(name).cloned().unwrap_or_default();
    let site = &alert.site_name;
    let tool = alert.kind.map(ToolKind::display_name).unwrap_or_default();
    let key = param("key");
    let (title, body) = match (alert.finding.code, zh) {
        ("balanceEmpty", true) => (
            "余额用完了".to_string(),
            format!("{site} 的余额用完了，编程工具会停下来。打开 TransitLink 去充值。"),
        ),
        ("balanceEmpty", false) => (
            "Balance used up".to_string(),
            format!(
                "{site} has no balance left, so your coding tools will stop. Open TransitLink to top up."
            ),
        ),
        ("balanceLow", true) => (
            "余额不多了".to_string(),
            format!(
                "{site} 的余额还剩 ${}，低于你设的 ${}。",
                param("amount"),
                param("threshold")
            ),
        ),
        ("balanceLow", false) => (
            "Balance is running low".to_string(),
            format!(
                "{site} has ${} left, below your ${} alert.",
                param("amount"),
                param("threshold")
            ),
        ),
        ("keyExpiring", true) => (
            "Key 快到期了".to_string(),
            format!("{tool} 用的 Key「{key}」3 天内到期，到期后就用不了了。"),
        ),
        ("keyExpiring", false) => (
            "A key expires soon".to_string(),
            format!("The key \u{201c}{key}\u{201d} used by {tool} expires within 3 days."),
        ),
        ("keyLow", true) => (
            "Key 的额度不多了".to_string(),
            format!("{tool} 用的 Key「{key}」只剩 ${}。", param("amount")),
        ),
        ("keyLow", false) => (
            "A key is running low".to_string(),
            format!(
                "The key \u{201c}{key}\u{201d} used by {tool} has ${} left.",
                param("amount")
            ),
        ),
        ("groupDenied", true) => (
            "分组不能用了".to_string(),
            format!(
                "{tool} 用的分组「{}」你已经没有权限了。打开 TransitLink 换个分组。",
                param("group")
            ),
        ),
        ("groupDenied", false) => (
            "A group is no longer available".to_string(),
            format!(
                "You no longer have access to the group \u{201c}{}\u{201d} used by {tool}. Open TransitLink to pick another.",
                param("group")
            ),
        ),
        // 停用、过期、额度用完、找不到：Key 不能用了。
        (code, true) => {
            let reason = match code {
                "keyDisabled" => "已停用",
                "keyExpired" => "已过期",
                "keyExhausted" => "的额度用完了",
                _ => "在站点上找不到了",
            };
            (
                "Key 用不了了".to_string(),
                format!("{tool} 用的 Key「{key}」{reason}。打开 TransitLink 换一个。"),
            )
        }
        (code, false) => {
            let reason = match code {
                "keyDisabled" => "was disabled",
                "keyExpired" => "has expired",
                "keyExhausted" => "has used up its quota",
                _ => "is gone from the site",
            };
            (
                "A key stopped working".to_string(),
                format!(
                    "The key \u{201c}{key}\u{201d} used by {tool} {reason}. Open TransitLink to switch keys."
                ),
            )
        }
    };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{Fix, Level, balance};

    fn alert(id: &str, code: &'static str) -> Alert {
        Alert {
            id: id.into(),
            site_id: "s".into(),
            site_name: "好用中转".into(),
            kind: Some(ToolKind::Codex),
            finding: Finding::new(Level::Error, code).param("key", "TransitLink Codex"),
        }
    }

    fn checked(alerts: Vec<Alert>) -> Vec<SiteOutcome> {
        vec![SiteOutcome {
            site_id: "s".into(),
            alerts: Some(alerts),
        }]
    }

    fn unchecked() -> Vec<SiteOutcome> {
        vec![SiteOutcome {
            site_id: "s".into(),
            alerts: None,
        }]
    }

    #[test]
    fn notifies_new_alerts_once_a_day() {
        let center = AlertCenter::default();
        let start = Instant::now();
        let first = vec![alert("key:codex", "keyDisabled")];
        let (changed, _, notify) = center.update(checked(first.clone()), start);
        assert!(changed);
        assert_eq!(notify.len(), 1);

        let (changed, _, notify) =
            center.update(checked(first.clone()), start + Duration::from_secs(1800));
        assert!(!changed);
        assert!(notify.is_empty(), "same problem, same day");

        let (_, _, notify) = center.update(checked(first.clone()), start + RENOTIFY);
        assert_eq!(notify.len(), 1, "reminded again a day later");

        // 解决了再出现：马上通知。
        let (changed, _, _) = center.update(checked(Vec::new()), start + RENOTIFY);
        assert!(changed);
        let (_, _, notify) =
            center.update(checked(first), start + RENOTIFY + Duration::from_secs(60));
        assert_eq!(notify.len(), 1);
    }

    #[test]
    fn a_failed_check_keeps_the_alerts_without_notifying_again() {
        let center = AlertCenter::default();
        let start = Instant::now();
        let low = vec![alert("balance:s", "balanceLow")];
        center.update(checked(low.clone()), start);
        // 断网了：这次没查成，提醒还在，也不算解决。
        let (changed, kept, _) = center.update(unchecked(), start + Duration::from_secs(1800));
        assert!(!changed);
        assert_eq!(kept, low);
        // 网好了，还是那个问题：不再通知。
        let (_, _, notify) = center.update(checked(low), start + Duration::from_secs(3600));
        assert!(notify.is_empty());
        // 站点不再查了（删掉了）：提醒去掉。
        let (changed, gone, _) = center.update(Vec::new(), start + Duration::from_secs(4000));
        assert!(changed && gone.is_empty());
    }

    #[test]
    fn a_problem_getting_worse_is_notified_again() {
        let center = AlertCenter::default();
        let start = Instant::now();
        center.update(checked(vec![alert("balance:s", "balanceLow")]), start);
        let (_, _, notify) = center.update(
            checked(vec![alert("balance:s", "balanceEmpty")]),
            start + Duration::from_secs(1800),
        );
        assert_eq!(notify.len(), 1, "used up is news even within the day");
    }

    #[test]
    fn messages_are_plain_and_keyless() {
        let disabled = alert("key:codex", "keyDisabled");
        let (title, body) = message(&disabled, "zh-CN");
        assert_eq!(title, "Key 用不了了");
        assert_eq!(
            body,
            "Codex 用的 Key「TransitLink Codex」已停用。打开 TransitLink 换一个。"
        );
        let (title, _) = message(&disabled, "en-US");
        assert_eq!(title, "A key stopped working");

        let low = Alert {
            id: "balance:s".into(),
            site_id: "s".into(),
            site_name: "好用中转".into(),
            kind: None,
            finding: balance("s", 1.2, 2.0).unwrap(),
        };
        assert_eq!(
            message(&low, "zh-CN").1,
            "好用中转 的余额还剩 $1.20，低于你设的 $2.00。"
        );
        assert!(matches!(low.finding.fix, Some(Fix::Recharge { .. })));
        let json = serde_json::to_value(&low).unwrap();
        assert_eq!(json["code"], "balanceLow");
        assert_eq!(json["siteName"], "好用中转");
        assert_eq!(json["fix"]["action"], "recharge");
    }
}
