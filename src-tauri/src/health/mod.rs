//! 一键体检和余额、Key 提醒共用的判断：余额够不够、工具用的 Key 还能不能用、分组还有没有权限。
//! 给界面的只有文案代码和参数（站点名、Key 的名字、金额、日期、变量名），不含 Key 本身。

pub mod alerts;
pub mod checkup;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::sites::{GroupSummary, KEY_PAGE, KeyStatus, KeySummary};
use crate::tools::ToolKind;

/// 有多严重：Error 是已经用不了了，Warn 是快出问题了或者可能有问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Warn,
    Error,
}

/// 修复办法，也就是界面上的按钮。能直接修的（重新配置）由界面调用现有命令；其他的打开对应的页面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Fix {
    /// 重新登录站点。
    #[serde(rename_all = "camelCase")]
    Relogin { site_id: String },
    /// 用在这个站点上选过的分组和模型重新配置：Key 不能用了会换一个新的，模型没了会自动换一个。
    #[serde(rename_all = "camelCase")]
    Reapply { kind: ToolKind, site_id: String },
    /// 重新选分组（打开这个工具的一键配置）。
    #[serde(rename_all = "camelCase")]
    Setup { kind: ToolKind, site_id: String },
    /// 打开站点去充值。
    #[serde(rename_all = "camelCase")]
    Recharge { site_id: String },
    /// 去工具页安装。
    Install { kind: ToolKind },
}

/// 一个问题：文案是界面上的 `health.<code>`，参数填进文案里。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub level: Level,
    pub code: &'static str,
    pub params: BTreeMap<&'static str, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

impl Finding {
    pub fn new(level: Level, code: &'static str) -> Self {
        Self {
            level,
            code,
            params: BTreeMap::new(),
            fix: None,
        }
    }

    pub fn param(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.insert(name, value.into());
        self
    }

    pub fn fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// 三天内到期的 Key 提前提醒。
const EXPIRING_SECS: i64 = 3 * 24 * 3600;

/// 金额都按美元显示两位小数。
pub fn usd(amount: f64) -> String {
    format!("{amount:.2}")
}

/// 余额对这个站点的工具有没有意义。sub2api 订阅分组里的 Key 用订阅的额度、不扣余额：
/// 工具用的 Key 都在订阅分组里时，余额是 0 也照样能用。用户有订阅分组、但这个站点上没有工具时也不提醒
/// （不知道他用的是哪种）。`tool_groups`：这个站点上各工具 Key 的分组名（查不到的是 None）。
pub fn balance_applies(groups: &[GroupSummary], tool_groups: &[Option<String>]) -> bool {
    if !groups.iter().any(|group| group.subscription) {
        return true;
    }
    let subscription = |name: &str| {
        groups
            .iter()
            .any(|group| group.subscription && (group.name == name || group.id == name))
    };
    tool_groups
        .iter()
        .any(|group| group.as_deref().is_none_or(|name| !subscription(name)))
}

/// 余额：用完了是 Error，低于提醒线是 Warn。
pub fn balance(site_id: &str, balance_usd: f64, below: f64) -> Option<Finding> {
    let recharge = Fix::Recharge {
        site_id: site_id.to_string(),
    };
    if balance_usd <= 0.0 {
        return Some(
            Finding::new(Level::Error, "balanceEmpty")
                .param("amount", usd(balance_usd))
                .fix(recharge),
        );
    }
    (balance_usd < below).then(|| {
        Finding::new(Level::Warn, "balanceLow")
            .param("amount", usd(balance_usd))
            .param("threshold", usd(below))
            .fix(recharge)
    })
}

/// 工具用的 Key：找不到、停用、过期、额度用完是 Error（重新配置会换一个新 Key）；
/// 三天内到期、剩下的额度低于提醒线是 Warn（到站点上处理，没有按钮）。
pub fn tool_key(
    kind: ToolKind,
    site_id: &str,
    key_id: &str,
    key_name: &str,
    keys: &[KeySummary],
    now: i64,
    below: f64,
) -> Option<Finding> {
    let reapply = Fix::Reapply {
        kind,
        site_id: site_id.to_string(),
    };
    let Some(key) = keys.iter().find(|key| key.id == key_id) else {
        // 列表只取了第一页：满了的话可能在后面，不能说它被删了。
        if keys.len() >= KEY_PAGE {
            return None;
        }
        return Some(
            Finding::new(Level::Error, "keyMissing")
                .param("key", key_name)
                .fix(reapply),
        );
    };
    let broken = match key.status {
        KeyStatus::Active => None,
        KeyStatus::Disabled => Some("keyDisabled"),
        KeyStatus::Expired => Some("keyExpired"),
        KeyStatus::Exhausted => Some("keyExhausted"),
    }
    .or_else(|| key.expires_at.filter(|at| *at <= now).map(|_| "keyExpired"))
    .or_else(|| {
        key.remaining_usd
            .filter(|remaining| *remaining <= 0.0)
            .map(|_| "keyExhausted")
    });
    if let Some(code) = broken {
        return Some(
            Finding::new(Level::Error, code)
                .param("key", &key.name)
                .fix(reapply),
        );
    }
    if let Some(at) = key.expires_at.filter(|at| at - now < EXPIRING_SECS) {
        return Some(
            Finding::new(Level::Warn, "keyExpiring")
                .param("key", &key.name)
                .param("at", at.to_string()),
        );
    }
    key.remaining_usd
        .filter(|remaining| *remaining < below)
        .map(|remaining| {
            Finding::new(Level::Warn, "keyLow")
                .param("key", &key.name)
                .param("amount", usd(remaining))
        })
}

/// Key 的分组不在用户能用的分组里了（站长调整了分组，或者用户的等级变了）：请求会被拒绝，换个分组。
/// 没有分组（用户自己的分组）或者读不到分组列表时不判断。
pub fn group_denied(
    kind: ToolKind,
    site_id: &str,
    key: &KeySummary,
    groups: &[GroupSummary],
) -> Option<Finding> {
    let group = key.group.as_deref().filter(|group| !group.is_empty())?;
    if groups.is_empty()
        || groups
            .iter()
            .any(|known| known.name == group || known.id == group)
    {
        return None;
    }
    Some(
        Finding::new(Level::Error, "groupDenied")
            .param("group", group)
            .fix(Fix::Setup {
                kind,
                site_id: site_id.to_string(),
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str, status: KeyStatus) -> KeySummary {
        KeySummary {
            id: id.into(),
            name: "TransitLink Claude Code".into(),
            key: "sk-secret-value".into(),
            masked: false,
            group: Some("cc".into()),
            enabled: status == KeyStatus::Active,
            status,
            remaining_usd: None,
            expires_at: None,
        }
    }

    #[test]
    fn balance_warns_below_the_line_and_errors_when_empty() {
        assert_eq!(balance("s", 5.0, 2.0), None);
        let low = balance("s", 1.5, 2.0).unwrap();
        assert_eq!((low.level, low.code), (Level::Warn, "balanceLow"));
        assert_eq!(low.params["amount"], "1.50");
        assert_eq!(low.params["threshold"], "2.00");
        let empty = balance("s", 0.0, 2.0).unwrap();
        assert_eq!((empty.level, empty.code), (Level::Error, "balanceEmpty"));
        assert_eq!(
            empty.fix,
            Some(Fix::Recharge {
                site_id: "s".into()
            })
        );
        assert_eq!(
            balance("s", 0.5, 0.0),
            None,
            "a zero line only warns when empty"
        );
    }

    #[test]
    fn tool_key_reports_broken_and_soon_broken_keys() {
        let now = 1_800_000_000;
        let check = |keys: &[KeySummary]| {
            tool_key(ToolKind::ClaudeCode, "s", "7", "old name", keys, now, 2.0)
        };
        assert_eq!(check(&[key("7", KeyStatus::Active)]), None);

        let missing = check(&[key("8", KeyStatus::Active)]).unwrap();
        assert_eq!((missing.level, missing.code), (Level::Error, "keyMissing"));
        assert_eq!(missing.params["key"], "old name");
        assert!(matches!(missing.fix, Some(Fix::Reapply { .. })));

        for (status, code) in [
            (KeyStatus::Disabled, "keyDisabled"),
            (KeyStatus::Expired, "keyExpired"),
            (KeyStatus::Exhausted, "keyExhausted"),
        ] {
            assert_eq!(check(&[key("7", status)]).unwrap().code, code);
        }

        let mut expired = key("7", KeyStatus::Active);
        expired.expires_at = Some(now - 1);
        assert_eq!(check(&[expired]).unwrap().code, "keyExpired");

        let mut soon = key("7", KeyStatus::Active);
        soon.expires_at = Some(now + 3600);
        let soon = check(&[soon]).unwrap();
        assert_eq!(
            (soon.level, soon.code, soon.fix),
            (Level::Warn, "keyExpiring", None)
        );

        let mut later = key("7", KeyStatus::Active);
        later.expires_at = Some(now + 30 * 24 * 3600);
        assert_eq!(check(&[later]), None);

        let mut low = key("7", KeyStatus::Active);
        low.remaining_usd = Some(0.8);
        let low = check(&[low]).unwrap();
        assert_eq!((low.level, low.code), (Level::Warn, "keyLow"));
        assert_eq!(low.params["amount"], "0.80");

        let mut used_up = key("7", KeyStatus::Active);
        used_up.remaining_usd = Some(0.0);
        assert_eq!(check(&[used_up]).unwrap().code, "keyExhausted");
    }

    #[test]
    fn findings_never_carry_the_key_itself() {
        let mut broken = key("7", KeyStatus::Disabled);
        broken.expires_at = Some(1);
        let finding = tool_key(ToolKind::Codex, "s", "7", "n", &[broken], 2, 2.0).unwrap();
        let json = serde_json::to_string(&finding).unwrap();
        assert!(!json.contains("sk-secret-value"), "{json}");
        assert!(json.contains("\"action\":\"reapply\""), "{json}");
        assert!(json.contains("\"siteId\":\"s\""), "{json}");
    }

    #[test]
    fn subscription_groups_make_the_balance_irrelevant() {
        let group = |name: &str, subscription: bool| GroupSummary {
            id: format!("id-{name}"),
            name: name.into(),
            description: String::new(),
            ratio: None,
            platform: Some("anthropic".into()),
            subscription,
        };
        let plain = [group("cc", false)];
        assert!(
            balance_applies(&plain, &[]),
            "no subscriptions: the balance matters"
        );
        let both = [group("cc", false), group("Claude Max", true)];
        assert!(!balance_applies(&both, &[Some("Claude Max".into())]));
        assert!(!balance_applies(&both, &[Some("id-Claude Max".into())]));
        assert!(balance_applies(
            &both,
            &[Some("Claude Max".into()), Some("cc".into())]
        ));
        assert!(
            balance_applies(&both, &[None]),
            "a key we couldn't find may spend the balance"
        );
        assert!(
            !balance_applies(&both, &[]),
            "a subscriber with no tools here: don't nag"
        );
    }

    #[test]
    fn a_full_key_page_does_not_prove_a_key_is_gone() {
        let keys: Vec<KeySummary> = (0..KEY_PAGE)
            .map(|index| key(&index.to_string(), KeyStatus::Active))
            .collect();
        assert_eq!(
            tool_key(ToolKind::Codex, "s", "missing", "n", &keys, 0, 2.0),
            None
        );
    }

    #[test]
    fn usable_keys_are_active_unexpired_and_funded() {
        let now = 1_800_000_000;
        let mut working = key("7", KeyStatus::Active);
        assert!(working.usable(now));
        working.expires_at = Some(now - 1);
        assert!(
            !working.usable(now),
            "sub2api keeps expired keys active in the list"
        );
        let mut empty = key("7", KeyStatus::Active);
        empty.remaining_usd = Some(0.0);
        assert!(!empty.usable(now));
        assert!(!key("7", KeyStatus::Disabled).usable(now));
    }

    #[test]
    fn group_denied_only_when_the_group_is_gone() {
        let group = |name: &str| GroupSummary {
            id: format!("id-{name}"),
            name: name.into(),
            description: String::new(),
            ratio: None,
            platform: None,
            subscription: false,
        };
        let key = key("7", KeyStatus::Active);
        assert_eq!(group_denied(ToolKind::ClaudeCode, "s", &key, &[]), None);
        assert_eq!(
            group_denied(ToolKind::ClaudeCode, "s", &key, &[group("cc")]),
            None
        );
        let denied = group_denied(ToolKind::ClaudeCode, "s", &key, &[group("vip")]).unwrap();
        assert_eq!(
            (denied.code, denied.params["group"].as_str()),
            ("groupDenied", "cc")
        );
        let mut own = key.clone();
        own.group = Some(String::new());
        assert_eq!(
            group_denied(ToolKind::ClaudeCode, "s", &own, &[group("vip")]),
            None
        );
    }
}
