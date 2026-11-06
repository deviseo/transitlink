//! Kimi Code（`kimi`）：在 `~/.kimi-code/config.toml`（`KIMI_CODE_HOME` 可以换目录）里加我们的服务商和模型，
//! 再把 `default_model` 指向它。
//!
//! - 一个服务商只用一种接口：Claude 模型用 `transitlink-anthropic`（type = "anthropic"，站点根地址），
//!   名字有 codex 的用 `transitlink-responses`（openai_responses），其他用 `transitlink`（openai，地址带 `/v1`）。
//! - 模型写成 `[models."transitlink/<模型>"]`（键里有 `/`，必须加引号，toml_edit 会自己加），
//!   `max_context_size` 必填。
//! - `base_url`、`api_key` 都要写：缺了 Kimi 会去读 OPENAI_BASE_URL 这类环境变量。
//! - Kimi 自己改这个文件时按行改、保留注释；我们用 toml_edit，也只动这几项。
//!
//! 2026-11-06 对照 Kimi Code 2.5.1（`@moonshot-ai/kimi-code`）源码核实；写法参考了 cc-switch（MIT）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, value};

use super::multi::{self, Doc, MultiFile, Target};
use super::{ConfigError, Connection};

const OPENAI: &str = "transitlink";
const ANTHROPIC: &str = "transitlink-anthropic";
const RESPONSES: &str = "transitlink-responses";
const OUR_PROVIDERS: [&str; 3] = [OPENAI, ANTHROPIC, RESPONSES];
const ALIAS_PREFIX: &str = "transitlink/";

/// Kimi 的数据目录：`KIMI_CODE_HOME`，没设时是 `~/.kimi-code`。
pub fn data_dir(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    env("KIMI_CODE_HOME")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kimi-code"))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    file_existed: bool,
    default_model: Option<String>,
    providers: Vec<(String, String)>,
    models: Vec<(String, String)>,
}

fn alias(model: &str) -> String {
    format!("{ALIAS_PREFIX}{model}")
}

/// 这个模型用哪个服务商、哪种接口、什么地址。
fn provider_for(model: &str, root: &str) -> (&'static str, &'static str, String) {
    let lower = model.to_ascii_lowercase();
    if lower.contains("claude") {
        (ANTHROPIC, "anthropic", root.to_string())
    } else if lower.contains("codex") {
        (RESPONSES, "openai_responses", format!("{root}/v1"))
    } else {
        (OPENAI, "openai", format!("{root}/v1"))
    }
}

fn parse(doc: &Doc) -> Result<DocumentMut, ConfigError> {
    doc.text().parse::<DocumentMut>().map_err(|_| doc.invalid())
}

fn item_text(key: &str, item: &Item) -> String {
    let mut doc = DocumentMut::new();
    doc.insert(key, item.clone());
    doc.to_string()
}

fn item_from(key: &str, text: &str) -> Option<Item> {
    text.parse::<DocumentMut>().ok()?.remove(key)
}

/// 顶层的 `name` 表（只装子表，自己不输出表头）；写成行内表的转成普通表。
fn table<'a>(doc: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    let entry = doc.entry(name).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    if !entry.is_table() {
        let item = std::mem::take(entry);
        let mut table = item.into_table().unwrap_or_default();
        table.set_implicit(true);
        *entry = Item::Table(table);
    }
    entry.as_table_mut().expect("table")
}

fn ours_in<'a>(
    doc: &'a DocumentMut,
    parent: &str,
    wanted: impl Fn(&str) -> bool + 'a,
) -> Vec<(String, &'a Item)> {
    doc.get(parent)
        .and_then(Item::as_table_like)
        .map(|table| {
            table
                .iter()
                .filter(|(key, _)| wanted(key))
                .map(|(key, item)| (key.to_string(), item))
                .collect()
        })
        .unwrap_or_default()
}

fn is_our_provider(id: &str) -> bool {
    OUR_PROVIDERS.contains(&id)
}

fn is_our_model(alias: &str) -> bool {
    alias.starts_with(ALIAS_PREFIX)
}

/// 删掉我们的服务商和模型；空了的 `providers`、`models` 也删掉。
fn remove_ours(doc: &mut DocumentMut) {
    for (parent, wanted) in [
        ("providers", is_our_provider as fn(&str) -> bool),
        ("models", is_our_model),
    ] {
        if let Some(table) = doc.get_mut(parent).and_then(Item::as_table_like_mut) {
            let keys: Vec<String> = table
                .iter()
                .map(|(key, _)| key.to_string())
                .filter(|key| wanted(key))
                .collect();
            for key in keys {
                table.remove(&key);
            }
            if table.is_empty() {
                doc.remove(parent);
            }
        }
    }
}

fn provider_table(kind: &str, base_url: &str, token: &str) -> Item {
    let mut table = Table::new();
    table.insert("type", value(kind));
    table.insert("base_url", value(base_url));
    table.insert("api_key", value(token));
    Item::Table(table)
}

fn model_table(model: &str, provider: &str) -> Item {
    let (context, output) = super::opencode::limits(model);
    let mut table = Table::new();
    table.insert("provider", value(provider));
    table.insert("model", value(model));
    table.insert("max_context_size", value(context as i64));
    table.insert("max_output_size", value(output as i64));
    Item::Table(table)
}

/// 加上一个模型（还没有它要用的服务商时建一个）。
fn add_model(doc: &mut DocumentMut, model: &str, root: &str, token: &str) {
    let (id, kind, base) = provider_for(model, root);
    let providers = table(doc, "providers");
    if !providers.contains_key(id) {
        providers.insert(id, provider_table(kind, &base, token));
    }
    let models = table(doc, "models");
    let key = alias(model);
    if !models.contains_key(&key) {
        models.insert(&key, model_table(model, id));
    }
}

pub struct Adapter;

impl MultiFile for Adapter {
    fn files(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, String)> {
        vec![(
            data_dir(home, env).join("config.toml"),
            "config.toml".into(),
        )]
    }

    fn apply(
        &self,
        docs: &[Doc],
        target: &Target,
        previous: Option<&Value>,
    ) -> Result<(Vec<Option<String>>, Value), ConfigError> {
        let file = &docs[0];
        let mut doc = parse(file)?;
        let snapshot = multi::snapshot::<Snapshot>(previous).unwrap_or_else(|| Snapshot {
            file_existed: file.text.is_some(),
            default_model: doc
                .get("default_model")
                .map(|item| item_text("default_model", item)),
            providers: ours_in(&doc, "providers", is_our_provider)
                .into_iter()
                .map(|(key, item)| (key.clone(), item_text(&key, item)))
                .collect(),
            models: ours_in(&doc, "models", is_our_model)
                .into_iter()
                .map(|(key, item)| (key.clone(), item_text(&key, item)))
                .collect(),
        });
        remove_ours(&mut doc);
        let mut models: Vec<&str> = target.models.iter().map(String::as_str).collect();
        if !models.contains(&target.model) {
            models.insert(0, target.model);
        }
        for model in models {
            add_model(&mut doc, model, target.base_url, target.token);
        }
        // 用下标赋值：`insert` 会丢掉这个键前面的注释。
        doc["default_model"] = value(alias(target.model));
        Ok((vec![Some(doc.to_string())], multi::to_value(&snapshot)?))
    }

    fn set_model(&self, docs: &[Doc], model: &str) -> Result<Vec<Option<String>>, ConfigError> {
        let file = &docs[0];
        let mut doc = parse(file)?;
        let (root, token) = {
            let ours = ours_in(&doc, "providers", is_our_provider);
            let (_, item) = ours.first().ok_or(ConfigError::NotConfigured)?;
            let field = |name: &str| {
                item.get(name)
                    .and_then(Item::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            (
                field("base_url")
                    .trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string(),
                field("api_key"),
            )
        };
        add_model(&mut doc, model, &root, &token);
        doc["default_model"] = value(alias(model));
        Ok(vec![Some(doc.to_string())])
    }

    fn restore(&self, docs: &[Doc], snapshot: &Value) -> Result<Vec<Option<String>>, ConfigError> {
        let snapshot = multi::snapshot::<Snapshot>(Some(snapshot)).unwrap_or_default();
        let file = &docs[0];
        if file.text.is_none() {
            return Ok(vec![None]);
        }
        let mut doc = parse(file)?;
        remove_ours(&mut doc);
        for (parent, saved) in [
            ("providers", &snapshot.providers),
            ("models", &snapshot.models),
        ] {
            for (key, text) in saved {
                if let Some(item) = item_from(key, text) {
                    table(&mut doc, parent).insert(key, item);
                }
            }
        }
        match snapshot
            .default_model
            .as_deref()
            .and_then(|text| item_from("default_model", text))
        {
            Some(item) => {
                doc["default_model"] = item;
            }
            None => {
                doc.remove("default_model");
            }
        }
        let rendered = doc.to_string();
        Ok(vec![
            (snapshot.file_existed || !rendered.trim().is_empty()).then_some(rendered),
        ])
    }

    fn read(&self, docs: &[Doc]) -> Result<Connection, ConfigError> {
        let doc = parse(&docs[0])?;
        let default = doc
            .get("default_model")
            .and_then(Item::as_str)
            .map(str::to_string);
        let model_entry = default
            .as_deref()
            .and_then(|alias| doc.get("models").and_then(|models| models.get(alias)));
        let provider = model_entry
            .and_then(|entry| entry.get("provider"))
            .and_then(Item::as_str)
            .map(str::to_string);
        let ours = default.as_deref().is_some_and(is_our_model)
            && provider.as_deref().is_some_and(is_our_provider);
        let provider_entry = provider
            .as_deref()
            .filter(|_| ours)
            .and_then(|id| doc.get("providers").and_then(|providers| providers.get(id)));
        let field = |name: &str| {
            provider_entry
                .and_then(|entry| entry.get(name))
                .and_then(Item::as_str)
                .map(str::to_string)
        };
        Ok(Connection {
            ours,
            other: default.is_some() && !ours,
            base_url: field("base_url").map(|url| {
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            }),
            token: field("api_key"),
            model: model_entry
                .filter(|_| ours)
                .and_then(|entry| entry.get("model"))
                .and_then(Item::as_str)
                .map(str::to_string),
            provider: provider.or(default),
        })
    }

    fn models(&self, kind: crate::tools::ToolKind, _chosen: &str, ids: &[String]) -> Vec<String> {
        // 一个服务商一种接口，但可以有几个服务商：同时列上 Claude、Codex 和其他对话模型。
        ids.iter()
            .filter(|id| super::probe::fits(kind, id))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(text: Option<&str>) -> Vec<Doc> {
        vec![Doc {
            path: PathBuf::from("/home/a/.kimi-code/config.toml"),
            backup: "config.toml".into(),
            text: text.map(str::to_string),
        }]
    }

    fn target<'a>(model: &'a str, models: &'a [String]) -> Target<'a> {
        Target {
            site_name: "示例中转站",
            base_url: "https://relay.example.com",
            token: "sk-relay",
            model,
            models,
        }
    }

    #[test]
    fn writes_one_provider_per_api_and_restores() {
        let before = "# Kimi Code 配置\ndefault_model = \"kimi-code/kimi-for-coding\"\n\n[providers.\"kimi-code\"]\ntype = \"kimi\"\n\n[models.\"kimi-code/kimi-for-coding\"]\nprovider = \"kimi-code\"\nmodel = \"kimi-for-coding\"\nmax_context_size = 262144\n";
        let start = docs(Some(before));
        assert!(Adapter.read(&start).unwrap().other);
        let list = vec!["kimi-k2.6".to_string(), "claude-sonnet-5-5".to_string()];
        let (next, snapshot) = Adapter
            .apply(&start, &target("kimi-k2.6", &list), None)
            .unwrap();
        let text = next[0].as_deref().unwrap();
        assert!(text.starts_with("# Kimi Code 配置\ndefault_model = \"transitlink/kimi-k2.6\""));
        assert!(text.contains("[providers.transitlink]\ntype = \"openai\"\nbase_url = \"https://relay.example.com/v1\""));
        assert!(text.contains("[providers.transitlink-anthropic]\ntype = \"anthropic\"\nbase_url = \"https://relay.example.com\""));
        assert!(text.contains(
            "[models.\"transitlink/claude-sonnet-5-5\"]\nprovider = \"transitlink-anthropic\""
        ));
        assert!(
            text.contains("[providers.\"kimi-code\"]"),
            "the user's own provider stays"
        );

        let applied = docs(Some(text));
        let current = Adapter.read(&applied).unwrap();
        assert!(current.ours);
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.token.as_deref(), Some("sk-relay"));
        assert_eq!(current.model.as_deref(), Some("kimi-k2.6"));

        let switched = docs(Adapter.set_model(&applied, "gpt-5.3-codex").unwrap()[0].as_deref());
        let current = Adapter.read(&switched).unwrap();
        assert_eq!(current.model.as_deref(), Some("gpt-5.3-codex"));
        assert!(
            switched[0]
                .text()
                .contains("[providers.transitlink-responses]\ntype = \"openai_responses\"")
        );

        let restored = Adapter.restore(&switched, &snapshot).unwrap();
        let restored = restored[0].as_deref().unwrap();
        assert!(restored.contains("default_model = \"kimi-code/kimi-for-coding\""));
        assert!(!restored.contains("transitlink"));
        assert!(restored.contains("# Kimi Code 配置"));
    }

    #[test]
    fn a_new_file_is_removed_again() {
        let start = docs(None);
        let list = vec!["claude-sonnet-5-5".to_string()];
        let (next, snapshot) = Adapter
            .apply(&start, &target("claude-sonnet-5-5", &list), None)
            .unwrap();
        assert_eq!(
            Adapter
                .restore(&docs(next[0].as_deref()), &snapshot)
                .unwrap(),
            vec![None]
        );
        assert!(matches!(
            Adapter.read(&docs(Some("a = ["))),
            Err(ConfigError::InvalidFile(_))
        ));
    }
}
