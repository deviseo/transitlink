//! Codex：在 `config.toml` 里加一个 `transitlink` 服务商（地址和 Key），再把 `model_provider`
//! 指向它。不动 `auth.json`，ChatGPT 账号登录不受影响。
//!
//! 用 toml_edit 修改，原来的注释、格式和其他设置都保留。和 Claude Code 一样，配置前先清掉
//! 决定「请求发到哪、用哪个模型」的关键字段，原值记在快照里，还原时放回（关键字段清单参考了
//! cc-switch（MIT）对 Codex 的整理）。Codex 0.149 起，自定义服务商的 Key 写在
//! `experimental_bearer_token`；`wire_api` 只支持 `responses`。

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, value};

use super::ConfigError;

/// 我们的服务商 ID。切换站点只改这张表，ID 不变，Codex 的历史会话不受影响。
pub const PROVIDER_ID: &str = "transitlink";

/// 顶层的关键字段（`model_provider` 和 `model` 原地改写，其余清掉）。
const TOP_KEYS: [&str; 8] = [
    "model_provider",
    "model",
    "openai_base_url",
    "review_model",
    "model_catalog_json",
    "experimental_bearer_token",
    "base_url",
    "wire_api",
];

/// 嵌套在用户自己的表里的模型名：只清这几个键，表里的其他设置不动。
const NESTED_KEYS: [(&str, &str); 3] = [
    ("agents", "default_subagent_model"),
    ("memories", "extract_model"),
    ("memories", "consolidation_model"),
];

/// 第一次配置前关键字段的原值，都存成 TOML 文本（`键 = 值`），还原时原样解析回去。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    file_existed: bool,
    top: Vec<(String, String)>,
    nested: Vec<(String, String, String)>,
    /// 原来就有 `[model_providers.transitlink]` 时的内容。
    provider: Option<String>,
}

pub struct Target<'a> {
    pub site_name: &'a str,
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: &'a str,
}

/// 配置文件里现在的连接。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Current {
    pub provider: Option<String>,
    /// 只有 `model_provider = "transitlink"` 时才有（去掉了末尾的 `/v1`）。
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
}

fn parse(text: Option<&str>) -> Result<DocumentMut, ConfigError> {
    text.unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|_| ConfigError::Invalid)
}

/// 一个键值单独存成一段 TOML，例如 `model = "gpt-6"`。
fn item_text(key: &str, item: &Item) -> String {
    let mut doc = DocumentMut::new();
    doc.insert(key, item.clone());
    doc.to_string()
}

fn item_from(key: &str, text: &str) -> Option<Item> {
    text.parse::<DocumentMut>().ok()?.remove(key)
}

/// 选中的配置档（`profile = "..."`）自己也设了服务商或模型时，它会盖过顶层设置，所以也算关键字段。
fn profile_overrides(doc: &DocumentMut) -> bool {
    let Some(name) = doc.get("profile").and_then(Item::as_str) else {
        return false;
    };
    doc.get("profiles")
        .and_then(|profiles| profiles.get(name))
        .is_some_and(|profile| {
            profile.get("model_provider").is_some() || profile.get("model").is_some()
        })
}

/// 取出关键字段的原值；除 `model_provider`、`model` 外都从配置里删掉（这两个随后原地改写）。
fn take_key_fields(doc: &mut DocumentMut, file_existed: bool) -> Snapshot {
    let mut snapshot = Snapshot {
        file_existed,
        ..Snapshot::default()
    };
    let mut keys: Vec<&str> = TOP_KEYS.to_vec();
    if profile_overrides(doc) {
        keys.push("profile");
    }
    for key in keys {
        let Some(item) = doc.get(key).filter(|item| !item.is_none()) else {
            continue;
        };
        snapshot.top.push((key.to_string(), item_text(key, item)));
        if key != "model_provider" && key != "model" {
            doc.remove(key);
        }
    }
    for (table, key) in NESTED_KEYS {
        let Some(parent) = doc.get_mut(table).and_then(Item::as_table_like_mut) else {
            continue;
        };
        if let Some(item) = parent.remove(key) {
            snapshot
                .nested
                .push((table.to_string(), key.to_string(), item_text(key, &item)));
        }
    }
    if let Some(item) = doc
        .get("model_providers")
        .and_then(|providers| providers.get(PROVIDER_ID))
    {
        snapshot.provider = Some(item_text(PROVIDER_ID, item));
    }
    snapshot
}

/// `[model_providers]` 表；没有就建一个隐式表（只输出 `[model_providers.transitlink]`）。
/// 写成内联表的也转成普通表。
fn providers_table(doc: &mut DocumentMut) -> &mut Table {
    let entry = doc.entry("model_providers").or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    if !entry.is_table() {
        let item = std::mem::take(entry);
        *entry = match item.into_table() {
            Ok(table) => Item::Table(table),
            Err(_) => {
                let mut table = Table::new();
                table.set_implicit(true);
                Item::Table(table)
            }
        };
    }
    entry.as_table_mut().expect("model_providers is a table")
}

/// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过就沿用它，还原总是回到第一次之前。
pub fn apply(
    text: Option<&str>,
    target: &Target,
    previous: Option<&Snapshot>,
) -> Result<(String, Snapshot), ConfigError> {
    let mut doc = parse(text)?;
    let taken = take_key_fields(&mut doc, text.is_some());
    doc["model_provider"] = value(PROVIDER_ID);
    doc["model"] = value(target.model);

    let mut provider = Table::new();
    provider.insert("name", value(format!("TransitLink · {}", target.site_name)));
    provider.insert("base_url", value(format!("{}/v1", target.base_url)));
    provider.insert("wire_api", value("responses"));
    provider.insert("experimental_bearer_token", value(target.token));
    providers_table(&mut doc).insert(PROVIDER_ID, Item::Table(provider));

    Ok((doc.to_string(), previous.cloned().unwrap_or(taken)))
}

/// 只改模型。
pub fn set_model(text: Option<&str>, model: &str) -> Result<String, ConfigError> {
    let mut doc = parse(text)?;
    doc["model"] = value(model);
    Ok(doc.to_string())
}

/// 还原：撤掉我们的服务商和关键字段，放回快照里的原值。返回 None 表示文件本来不存在、
/// 现在也没有别的内容，可以删掉。
pub fn restore(text: Option<&str>, snapshot: &Snapshot) -> Result<Option<String>, ConfigError> {
    let mut doc = parse(text)?;
    if let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
    {
        providers.remove(PROVIDER_ID);
        if providers.is_empty() {
            doc.remove("model_providers");
        }
    }
    // 原来有值的键原地改回去（键上方的注释和位置都不变），原来没有的删掉。
    let mut keys: Vec<&str> = TOP_KEYS.to_vec();
    keys.extend(
        snapshot
            .top
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|key| !TOP_KEYS.contains(key)),
    );
    for key in keys {
        let saved = snapshot
            .top
            .iter()
            .find(|(saved, _)| saved == key)
            .and_then(|(_, text)| item_from(key, text));
        match saved {
            Some(item) if doc.contains_key(key) => doc[key] = item,
            Some(item) => {
                doc.insert(key, item);
            }
            None => {
                doc.remove(key);
            }
        }
    }
    for (table, key, text) in &snapshot.nested {
        if let Some(item) = item_from(key, text) {
            let parent = doc.entry(table).or_insert_with(|| {
                let mut table = Table::new();
                table.set_implicit(true);
                Item::Table(table)
            });
            if let Some(parent) = parent.as_table_like_mut() {
                parent.insert(key, item);
            }
        }
    }
    if let Some(item) = snapshot
        .provider
        .as_deref()
        .and_then(|text| item_from(PROVIDER_ID, text))
    {
        providers_table(&mut doc).insert(PROVIDER_ID, item);
    }
    let rendered = doc.to_string();
    if !snapshot.file_existed && rendered.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(rendered))
}

/// 读出现在的连接（文件格式有误时返回 Invalid）。
pub fn read(text: Option<&str>) -> Result<Current, ConfigError> {
    let doc = parse(text)?;
    let provider = doc
        .get("model_provider")
        .and_then(Item::as_str)
        .map(str::to_string);
    let ours = doc
        .get("model_providers")
        .and_then(|providers| providers.get(PROVIDER_ID))
        .filter(|_| provider.as_deref() == Some(PROVIDER_ID));
    let field = |key: &str| {
        ours.and_then(|table| table.get(key))
            .and_then(Item::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    Ok(Current {
        base_url: field("base_url").map(|url| {
            url.trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_string()
        }),
        token: field("experimental_bearer_token"),
        model: doc.get("model").and_then(Item::as_str).map(str::to_string),
        provider,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Target = Target {
        site_name: "示例中转站",
        base_url: "https://relay.example.com",
        token: "sk-relay",
        model: "gpt-6.1-sol",
    };

    #[test]
    fn writes_a_fresh_config() {
        let (text, snapshot) = apply(None, &TARGET, None).unwrap();
        assert_eq!(
            text,
            r#"model_provider = "transitlink"
model = "gpt-6.1-sol"

[model_providers.transitlink]
name = "TransitLink · 示例中转站"
base_url = "https://relay.example.com/v1"
wire_api = "responses"
experimental_bearer_token = "sk-relay"
"#
        );
        assert_eq!(
            read(Some(&text)).unwrap(),
            Current {
                provider: Some("transitlink".into()),
                base_url: Some("https://relay.example.com".into()),
                token: Some("sk-relay".into()),
                model: Some("gpt-6.1-sol".into()),
            }
        );
        assert_eq!(restore(Some(&text), &snapshot).unwrap(), None);
    }

    const EXISTING: &str = r#"# 我的 Codex 配置
model = "o3" # 以前用的
model_provider = "other"
approval_policy = "on-request"
openai_base_url = "https://proxy.example.com/v1"
profile = "work"

[model_providers.other]
name = "Other"
base_url = "https://other.example.com/v1"

[profiles.work]
model = "gpt-5"
sandbox_mode = "workspace-write"

[agents]
default_subagent_model = "o3-mini"
max_threads = 4

[mcp_servers.docs]
command = "docs-mcp"
"#;

    #[test]
    fn switches_providers_and_keeps_the_rest_of_the_file() {
        let (text, snapshot) = apply(Some(EXISTING), &TARGET, None).unwrap();
        // 注释、其他服务商、配置档、MCP 都还在。
        for kept in [
            "# 我的 Codex 配置",
            "approval_policy = \"on-request\"",
            "[model_providers.other]",
            "[profiles.work]",
            "max_threads = 4",
            "[mcp_servers.docs]",
        ] {
            assert!(text.contains(kept), "{kept} should be kept:\n{text}");
        }
        // 会盖过我们设置的字段都清掉了。
        for removed in [
            "openai_base_url",
            "profile = \"work\"",
            "default_subagent_model",
        ] {
            assert!(
                !text.contains(removed),
                "{removed} should be removed:\n{text}"
            );
        }
        assert!(text.starts_with("# 我的 Codex 配置\nmodel = \"gpt-6.1-sol\""));
        let current = read(Some(&text)).unwrap();
        assert_eq!(current.provider.as_deref(), Some("transitlink"));
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );

        // 还原后关键字段回到原值，其余内容不变。
        let restored = restore(Some(&text), &snapshot).unwrap().unwrap();
        let original: toml_edit::DocumentMut = EXISTING.parse().unwrap();
        let restored_doc: toml_edit::DocumentMut = restored.parse().unwrap();
        for key in [
            "model",
            "model_provider",
            "openai_base_url",
            "profile",
            "approval_policy",
        ] {
            assert_eq!(
                restored_doc[key].as_str(),
                original[key].as_str(),
                "{key} is restored"
            );
        }
        assert_eq!(
            restored_doc["agents"]["default_subagent_model"].as_str(),
            Some("o3-mini")
        );
        assert!(restored_doc["model_providers"].get(PROVIDER_ID).is_none());
        assert!(restored_doc["model_providers"].get("other").is_some());
        assert!(restored.contains("# 以前用的"), "inline comments survive");
        assert!(
            restored.starts_with("# 我的 Codex 配置\nmodel = \"o3\""),
            "keys go back in place with the comment above them:\n{restored}"
        );
    }

    #[test]
    fn reapplying_keeps_the_first_snapshot() {
        let (text, snapshot) = apply(Some(EXISTING), &TARGET, None).unwrap();
        let other = Target {
            site_name: "Other",
            base_url: "https://b.example.com",
            token: "sk-b",
            model: "gpt-5.5",
        };
        let (text, again) = apply(Some(&text), &other, Some(&snapshot)).unwrap();
        assert_eq!(again, snapshot);
        assert_eq!(text.matches("[model_providers.transitlink]").count(), 1);
        assert_eq!(read(Some(&text)).unwrap().token.as_deref(), Some("sk-b"));
        let text = set_model(Some(&text), "gpt-6-astra").unwrap();
        assert_eq!(
            read(Some(&text)).unwrap().model.as_deref(),
            Some("gpt-6-astra")
        );
    }

    #[test]
    fn keeps_profiles_that_do_not_pick_a_provider() {
        let config = "profile = \"fast\"\n\n[profiles.fast]\napproval_policy = \"never\"\n";
        let (text, _) = apply(Some(config), &TARGET, None).unwrap();
        assert!(text.contains("profile = \"fast\""));
    }

    #[test]
    fn converts_inline_provider_tables() {
        let config = "model_providers = { other = { name = \"Other\", base_url = \"https://o.example.com/v1\" } }\n";
        let (text, snapshot) = apply(Some(config), &TARGET, None).unwrap();
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        assert_eq!(
            doc["model_providers"]["other"]["name"].as_str(),
            Some("Other")
        );
        assert_eq!(
            doc["model_providers"][PROVIDER_ID]["wire_api"].as_str(),
            Some("responses")
        );
        let restored: toml_edit::DocumentMut = restore(Some(&text), &snapshot)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert!(restored["model_providers"].get(PROVIDER_ID).is_none());
        assert_eq!(
            restored["model_providers"]["other"]["name"].as_str(),
            Some("Other")
        );
    }

    #[test]
    fn refuses_to_touch_invalid_files() {
        assert_eq!(
            apply(Some("model = \n"), &TARGET, None).unwrap_err(),
            ConfigError::Invalid
        );
        assert_eq!(read(Some("[unclosed")).unwrap_err(), ConfigError::Invalid);
    }
}
