//! Grok Build（xAI 的 Grok 命令行）：在 `~/.grok/config.toml` 里加一个 `[model.transitlink]`
//! （站点地址、Key、模型），再把 `[models]` 里的 `default`、`session_summary`、`image_description`
//! 指向它：会话标题和图片说明也用站点的模型，否则 Grok 会拿它内置的模型名请求站点而失败。
//! 有了自己的 Key，Grok 不再要求登录 xAI 账号。
//!
//! 用 toml_edit 修改，其他设置保留（Grok 自己改设置时会重写整个文件，注释不一定保得住）。
//! 2026-11-05 对照 Grok Build 1.0.45 源码和 docs.x.ai/build 核实：`base_url` 要带 `/v1`，
//! Grok 在后面接 `chat/completions`；Key 以 `Authorization: Bearer` 发送。

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, value};

use super::ConfigError;

/// 我们的模型表的名字。不会和 Grok 内置的模型名重名（重名会变成覆盖内置模型）。
pub const MODEL_KEY: &str = "transitlink";

/// `[models]` 里要指向我们模型的键。
const MODELS_KEYS: [&str; 3] = ["default", "session_summary", "image_description"];

/// 第一次配置前这些键的原值（TOML 文本），还原时放回。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    file_existed: bool,
    models: Vec<(String, String)>,
    /// 原来就有 `[model.transitlink]` 时的内容。
    table: Option<String>,
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
    /// `[models].default`。
    pub default: Option<String>,
    /// 只有 `default` 指向我们的表时才有（去掉了末尾的 `/v1`）。
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
}

fn parse(text: Option<&str>) -> Result<DocumentMut, ConfigError> {
    text.unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|_| ConfigError::Invalid)
}

fn item_text(key: &str, item: &Item) -> String {
    let mut doc = DocumentMut::new();
    doc.insert(key, item.clone());
    doc.to_string()
}

fn item_from(key: &str, text: &str) -> Option<Item> {
    text.parse::<DocumentMut>().ok()?.remove(key)
}

/// `name` 表；没有就建一个（`implicit`：只用来装子表、自己没有键时不输出表头）。写成内联表的转成普通表。
fn table<'a>(doc: &'a mut DocumentMut, name: &str, implicit: bool) -> &'a mut Table {
    let entry = doc.entry(name).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(implicit);
        Item::Table(table)
    });
    if !entry.is_table() {
        let item = std::mem::take(entry);
        *entry = Item::Table(item.into_table().unwrap_or_default());
    }
    entry.as_table_mut().expect("table")
}

fn take_snapshot(doc: &DocumentMut, file_existed: bool) -> Snapshot {
    let models = doc.get("models");
    Snapshot {
        file_existed,
        models: MODELS_KEYS
            .iter()
            .filter_map(|key| {
                models
                    .and_then(|models| models.get(key))
                    .map(|item| (key.to_string(), item_text(key, item)))
            })
            .collect(),
        table: doc
            .get("model")
            .and_then(|model| model.get(MODEL_KEY))
            .map(|item| item_text(MODEL_KEY, item)),
    }
}

/// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过就沿用它，还原总是回到第一次之前。
pub fn apply(
    text: Option<&str>,
    target: &Target,
    previous: Option<&Snapshot>,
) -> Result<(String, Snapshot), ConfigError> {
    let mut doc = parse(text)?;
    let taken = take_snapshot(&doc, text.is_some());
    let models = table(&mut doc, "models", false);
    for key in MODELS_KEYS {
        models.insert(key, value(MODEL_KEY));
    }
    let mut ours = Table::new();
    ours.insert("name", value(format!("TransitLink · {}", target.site_name)));
    ours.insert("model", value(target.model));
    ours.insert("base_url", value(format!("{}/v1", target.base_url)));
    ours.insert("api_key", value(target.token));
    ours.insert("api_backend", value("chat_completions"));
    table(&mut doc, "model", true).insert(MODEL_KEY, Item::Table(ours));
    Ok((doc.to_string(), previous.cloned().unwrap_or(taken)))
}

/// 只改模型。
pub fn set_model(text: Option<&str>, model: &str) -> Result<String, ConfigError> {
    let mut doc = parse(text)?;
    let ours = doc
        .get_mut("model")
        .and_then(|models| models.get_mut(MODEL_KEY))
        .and_then(Item::as_table_like_mut)
        .ok_or(ConfigError::NotConfigured)?;
    ours.insert("model", value(model));
    Ok(doc.to_string())
}

/// 还原：撤掉我们的模型表，`[models]` 里的键放回原值（原来没有的删掉）。
/// 返回 None 表示文件本来不存在、现在也没有别的内容，可以删掉。
pub fn restore(text: Option<&str>, snapshot: &Snapshot) -> Result<Option<String>, ConfigError> {
    let mut doc = parse(text)?;
    if let Some(models) = doc.get_mut("model").and_then(Item::as_table_like_mut) {
        models.remove(MODEL_KEY);
        if models.is_empty() {
            doc.remove("model");
        }
    }
    if let Some(item) = snapshot
        .table
        .as_deref()
        .and_then(|text| item_from(MODEL_KEY, text))
    {
        table(&mut doc, "model", true).insert(MODEL_KEY, item);
    }
    if let Some(models) = doc.get_mut("models").and_then(Item::as_table_like_mut) {
        for key in MODELS_KEYS {
            let saved = snapshot
                .models
                .iter()
                .find(|(saved, _)| saved == key)
                .and_then(|(_, text)| item_from(key, text));
            match saved {
                Some(item) => {
                    models.insert(key, item);
                }
                None => {
                    models.remove(key);
                }
            }
        }
        if models.is_empty() {
            doc.remove("models");
        }
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
    let default = doc
        .get("models")
        .and_then(|models| models.get("default"))
        .and_then(Item::as_str)
        .map(str::to_string);
    let ours = doc
        .get("model")
        .and_then(|models| models.get(MODEL_KEY))
        .filter(|_| default.as_deref() == Some(MODEL_KEY));
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
        token: field("api_key"),
        model: field("model"),
        default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Target = Target {
        site_name: "示例中转站",
        base_url: "https://relay.example.com",
        token: "sk-relay",
        model: "grok-4.7",
    };

    #[test]
    fn writes_a_fresh_config() {
        let (text, snapshot) = apply(None, &TARGET, None).unwrap();
        assert_eq!(
            text,
            r#"[models]
default = "transitlink"
session_summary = "transitlink"
image_description = "transitlink"

[model.transitlink]
name = "TransitLink · 示例中转站"
model = "grok-4.7"
base_url = "https://relay.example.com/v1"
api_key = "sk-relay"
api_backend = "chat_completions"
"#
        );
        assert!(!snapshot.file_existed);
        let current = read(Some(&text)).unwrap();
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.token.as_deref(), Some("sk-relay"));
        assert_eq!(current.model.as_deref(), Some("grok-4.7"));
        // 还原后文件本来不存在：删掉。
        assert_eq!(restore(Some(&text), &snapshot).unwrap(), None);
    }

    #[test]
    fn keeps_other_settings_and_restores_the_originals() {
        let original = r#"# my settings
[cli]
channel = "stable"

[models]
default = "grok-build"
web_search = "grok-4.7"

[model."my-local"]
base_url = "http://localhost:8080/v1"
"#;
        let (text, snapshot) = apply(Some(original), &TARGET, None).unwrap();
        assert!(text.contains("# my settings"));
        assert!(text.contains("channel = \"stable\""));
        assert!(text.contains("web_search = \"grok-4.7\""));
        assert!(text.contains("[model.\"my-local\"]"));
        assert_eq!(
            read(Some(&text)).unwrap().default.as_deref(),
            Some(MODEL_KEY)
        );

        let changed = set_model(Some(&text), "grok-code-fast-1").unwrap();
        assert_eq!(
            read(Some(&changed)).unwrap().model.as_deref(),
            Some("grok-code-fast-1")
        );

        let restored = restore(Some(&changed), &snapshot).unwrap().unwrap();
        assert!(restored.contains("default = \"grok-build\""));
        assert!(restored.contains("web_search = \"grok-4.7\""));
        assert!(!restored.contains("session_summary"));
        assert!(!restored.contains("transitlink"));
        assert!(restored.contains("[model.\"my-local\"]"));
        assert_eq!(read(Some(&restored)).unwrap().base_url, None);
    }

    #[test]
    fn another_default_model_is_not_ours() {
        let (text, _) = apply(None, &TARGET, None).unwrap();
        // 用户在 Grok 里 /model 换成了内置模型：请求不再发到站点。
        let switched = text.replace("default = \"transitlink\"", "default = \"grok-build\"");
        let current = read(Some(&switched)).unwrap();
        assert_eq!(current.default.as_deref(), Some("grok-build"));
        assert_eq!(current.base_url, None);
        assert_eq!(current.token, None);
    }

    #[test]
    fn invalid_files_are_reported() {
        assert_eq!(read(Some("[models\n")), Err(ConfigError::Invalid));
    }
}
