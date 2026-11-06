//! Pi（`pi`，earendil-works 的编程助手）：在 `~/.pi/agent/models.json` 里加一个 `transitlink` 服务商，
//! 再在 `settings.json` 里把 `defaultProvider`、`defaultModel` 指向它。目录可以用 `PI_CODING_AGENT_DIR` 换。
//!
//! - `models.json` 允许 `//` 注释和末尾逗号，Pi 自己从不写它：只改我们这一项，注释不动。
//! - `settings.json` 是严格 JSON，Pi 自己也会改（改之前拿 `settings.json.lock` 目录锁）：只改这两个键，
//!   锁着的时候不写。
//! - 一个服务商可以混用几种接口：Claude 模型走 Anthropic 接口（地址不带 `/v1`，SDK 自己接 `/v1/messages`），
//!   名字里有 codex 的走 Responses 接口，其他走 Chat Completions（地址带 `/v1`）。中转站不一定认 `store`、
//!   `developer` 角色和 `max_completion_tokens`，都关掉（和 DeepSeek Harness 用的是同一个 pi-ai 库）。
//! - Key 里的 `$` 要写成 `$$`、开头的 `!` 要写成 `$!`，否则会被当成环境变量或命令。
//! - 自定义服务商的模型不继承 Pi 内置的模型资料，上下文和输出上限要自己写。
//!
//! 2026-11-06 对照 Pi 1.5.0（`@earendil-works/pi-coding-agent`）源码核实；写法参考了 cc-switch（MIT）。

use std::path::{Path, PathBuf};

use jsonc_parser::cst::CstInputValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::multi::{self, Doc, MultiFile, Target};
use super::{ConfigError, Connection, jsonc};

pub const PROVIDER_ID: &str = "transitlink";

/// Pi 的配置目录：`PI_CODING_AGENT_DIR`，没设时是 `~/.pi/agent`。
pub fn agent_dir(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    env("PI_CODING_AGENT_DIR")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".pi").join("agent"))
}

/// 第一次配置前这几项的原值（JSON 文本），还原时放回。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    models_existed: bool,
    provider: Option<String>,
    settings_existed: bool,
    default_provider: Option<String>,
    default_model: Option<String>,
}

/// Key 原样写进去：`$` 写成 `$$`，开头的 `!` 写成 `$!`。
fn literal(token: &str) -> String {
    let escaped = token.replace('$', "$$");
    match escaped.strip_prefix('!') {
        Some(rest) => format!("$!{rest}"),
        None => escaped,
    }
}

/// [`literal`] 反过来（读出 Key 用）。
fn unliteral(value: &str) -> String {
    let value = match value.strip_prefix("$!") {
        Some(rest) => format!("!{rest}"),
        None => value.to_string(),
    };
    value.replace("$$", "$")
}

fn model_entry(model: &str, root: &str) -> CstInputValue {
    let (context, output) = super::opencode::limits(model);
    let lower = model.to_ascii_lowercase();
    let mut entry = vec![("id".to_string(), CstInputValue::String(model.to_string()))];
    if lower.contains("claude") {
        entry.push((
            "api".into(),
            CstInputValue::String("anthropic-messages".into()),
        ));
        entry.push(("baseUrl".into(), CstInputValue::String(root.to_string())));
    } else if lower.contains("codex") {
        entry.push((
            "api".into(),
            CstInputValue::String("openai-responses".into()),
        ));
    }
    entry.push((
        "contextWindow".into(),
        CstInputValue::Number(context.to_string()),
    ));
    entry.push((
        "maxTokens".into(),
        CstInputValue::Number(output.to_string()),
    ));
    CstInputValue::Object(entry)
}

fn provider_entry(target: &Target) -> CstInputValue {
    let mut models: Vec<&str> = target.models.iter().map(String::as_str).collect();
    if !models.contains(&target.model) {
        models.insert(0, target.model);
    }
    let string = |value: &str| CstInputValue::String(value.to_string());
    CstInputValue::Object(vec![
        (
            "name".into(),
            CstInputValue::String(format!("TransitLink · {}", target.site_name)),
        ),
        (
            "baseUrl".into(),
            CstInputValue::String(format!("{}/v1", target.base_url)),
        ),
        ("api".into(), string("openai-completions")),
        (
            "apiKey".into(),
            CstInputValue::String(literal(target.token)),
        ),
        (
            "compat".into(),
            CstInputValue::Object(vec![
                ("supportsStore".into(), CstInputValue::Bool(false)),
                ("supportsDeveloperRole".into(), CstInputValue::Bool(false)),
                ("maxTokensField".into(), string("max_tokens")),
            ]),
        ),
        (
            "models".into(),
            CstInputValue::Array(
                models
                    .into_iter()
                    .map(|model| model_entry(model, target.base_url))
                    .collect(),
            ),
        ),
    ])
}

/// 站点地址（去掉 `/v1`）。
fn root_of(base_url: &str) -> String {
    base_url
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

pub struct Adapter;

impl MultiFile for Adapter {
    fn files(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, String)> {
        let dir = agent_dir(home, env);
        vec![
            (dir.join("models.json"), "models.json".into()),
            (dir.join("settings.json"), "settings.json".into()),
        ]
    }

    fn busy(&self, docs: &[Doc]) -> bool {
        let lock = PathBuf::from(format!("{}.lock", docs[1].path.display()));
        lock.exists()
    }

    fn apply(
        &self,
        docs: &[Doc],
        target: &Target,
        previous: Option<&Value>,
    ) -> Result<(Vec<Option<String>>, Value), ConfigError> {
        let (models, settings) = (&docs[0], &docs[1]);
        let models_root = jsonc::parse(models.text.as_deref()).map_err(|error| models.at(error))?;
        let settings_root =
            jsonc::parse(settings.text.as_deref()).map_err(|error| settings.at(error))?;
        let models_object = models_root.object_value_or_set();
        let settings_object = settings_root.object_value_or_set();
        let snapshot = multi::snapshot::<Snapshot>(previous).unwrap_or_else(|| Snapshot {
            models_existed: models.text.is_some(),
            provider: models_object
                .object_value("providers")
                .and_then(|providers| jsonc::saved(&providers, PROVIDER_ID)),
            settings_existed: settings.text.is_some(),
            default_provider: jsonc::saved(&settings_object, "defaultProvider"),
            default_model: jsonc::saved(&settings_object, "defaultModel"),
        });
        let providers = models_object.object_value_or_set("providers");
        jsonc::set(&providers, PROVIDER_ID, provider_entry(target));
        jsonc::set(
            &settings_object,
            "defaultProvider",
            CstInputValue::String(PROVIDER_ID.into()),
        );
        jsonc::set(
            &settings_object,
            "defaultModel",
            CstInputValue::String(target.model.into()),
        );
        Ok((
            vec![
                Some(models_root.to_string()),
                Some(settings_root.to_string()),
            ],
            multi::to_value(&snapshot)?,
        ))
    }

    fn set_model(&self, docs: &[Doc], model: &str) -> Result<Vec<Option<String>>, ConfigError> {
        let (models, settings) = (&docs[0], &docs[1]);
        let models_root = jsonc::parse(models.text.as_deref()).map_err(|error| models.at(error))?;
        let ours = models_root
            .object_value()
            .and_then(|object| object.object_value("providers"))
            .and_then(|providers| providers.object_value(PROVIDER_ID))
            .ok_or(ConfigError::NotConfigured)?;
        let root = root_of(&jsonc::string_at(&ours, "baseUrl").unwrap_or_default());
        let list = ours.array_value_or_set("models");
        let known = list
            .elements()
            .iter()
            .filter_map(|element| element.to_serde_value())
            .any(|value| value.get("id").and_then(Value::as_str) == Some(model));
        if !known {
            list.ensure_multiline();
            list.append(model_entry(model, &root));
        }
        let settings_root =
            jsonc::parse(settings.text.as_deref()).map_err(|error| settings.at(error))?;
        let settings_object = settings_root.object_value_or_set();
        jsonc::set(
            &settings_object,
            "defaultProvider",
            CstInputValue::String(PROVIDER_ID.into()),
        );
        jsonc::set(
            &settings_object,
            "defaultModel",
            CstInputValue::String(model.into()),
        );
        Ok(vec![
            Some(models_root.to_string()),
            Some(settings_root.to_string()),
        ])
    }

    fn restore(&self, docs: &[Doc], snapshot: &Value) -> Result<Vec<Option<String>>, ConfigError> {
        let snapshot = multi::snapshot::<Snapshot>(Some(snapshot)).unwrap_or_default();
        let (models, settings) = (&docs[0], &docs[1]);
        let models_next = match &models.text {
            None => None,
            Some(text) => {
                let root = jsonc::parse(Some(text)).map_err(|error| models.at(error))?;
                if let Some(object) = root.object_value() {
                    if let Some(providers) = object.object_value("providers") {
                        jsonc::put_back(&providers, PROVIDER_ID, snapshot.provider.as_deref());
                    }
                    jsonc::remove_if_empty(&object, "providers");
                }
                (snapshot.models_existed || !jsonc::is_empty(&root)).then(|| root.to_string())
            }
        };
        let settings_next = match &settings.text {
            None => None,
            Some(text) => {
                let root = jsonc::parse(Some(text)).map_err(|error| settings.at(error))?;
                if let Some(object) = root.object_value() {
                    jsonc::put_back(
                        &object,
                        "defaultProvider",
                        snapshot.default_provider.as_deref(),
                    );
                    jsonc::put_back(&object, "defaultModel", snapshot.default_model.as_deref());
                }
                (snapshot.settings_existed || !jsonc::is_empty(&root)).then(|| root.to_string())
            }
        };
        Ok(vec![models_next, settings_next])
    }

    fn read(&self, docs: &[Doc]) -> Result<Connection, ConfigError> {
        let (models, settings) = (&docs[0], &docs[1]);
        let settings_root =
            jsonc::parse(settings.text.as_deref()).map_err(|error| settings.at(error))?;
        let models_root = jsonc::parse(models.text.as_deref()).map_err(|error| models.at(error))?;
        let settings_object = settings_root.object_value();
        let provider = settings_object
            .as_ref()
            .and_then(|object| jsonc::string_at(object, "defaultProvider"));
        let model = settings_object
            .as_ref()
            .and_then(|object| jsonc::string_at(object, "defaultModel"));
        let ours = provider.as_deref() == Some(PROVIDER_ID);
        let entry = models_root
            .object_value()
            .and_then(|object| object.object_value("providers"))
            .and_then(|providers| providers.object_value(PROVIDER_ID))
            .filter(|_| ours);
        Ok(Connection {
            ours,
            other: provider.is_some() && !ours,
            base_url: entry
                .as_ref()
                .and_then(|entry| jsonc::string_at(entry, "baseUrl"))
                .map(|url| root_of(&url)),
            token: entry
                .as_ref()
                .and_then(|entry| jsonc::string_at(entry, "apiKey"))
                .map(|key| unliteral(&key)),
            model: model.filter(|_| ours),
            provider,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(models: Option<&str>, settings: Option<&str>) -> Vec<Doc> {
        vec![
            Doc {
                path: PathBuf::from("/home/a/.pi/agent/models.json"),
                backup: "models.json".into(),
                text: models.map(str::to_string),
            },
            Doc {
                path: PathBuf::from("/home/a/.pi/agent/settings.json"),
                backup: "settings.json".into(),
                text: settings.map(str::to_string),
            },
        ]
    }

    fn target<'a>(model: &'a str, models: &'a [String]) -> Target<'a> {
        Target {
            site_name: "示例中转站",
            base_url: "https://relay.example.com",
            token: "sk-relay$1",
            model,
            models,
        }
    }

    fn with(docs: &[Doc], next: Vec<Option<String>>) -> Vec<Doc> {
        docs.iter()
            .zip(next)
            .map(|(doc, text)| Doc {
                path: doc.path.clone(),
                backup: doc.backup.clone(),
                text,
            })
            .collect()
    }

    #[test]
    fn writes_a_provider_that_mixes_apis_and_restores_to_nothing() {
        let models = vec![
            "claude-sonnet-5-5".to_string(),
            "gpt-6.1-sol".to_string(),
            "gpt-5.3-codex".to_string(),
        ];
        let start = docs(None, None);
        let (next, snapshot) = Adapter
            .apply(&start, &target("claude-sonnet-5-5", &models), None)
            .unwrap();
        let written: Value = serde_json::from_str(next[0].as_deref().unwrap()).unwrap();
        let ours = &written["providers"]["transitlink"];
        assert_eq!(ours["baseUrl"], "https://relay.example.com/v1");
        assert_eq!(ours["apiKey"], "sk-relay$$1", "a `$` is escaped");
        assert_eq!(ours["compat"]["maxTokensField"], "max_tokens");
        assert_eq!(ours["models"][0]["api"], "anthropic-messages");
        assert_eq!(ours["models"][0]["baseUrl"], "https://relay.example.com");
        assert_eq!(
            ours["models"][1].get("api"),
            None,
            "uses the provider's chat completions"
        );
        assert_eq!(ours["models"][2]["api"], "openai-responses");
        let settings: Value = serde_json::from_str(next[1].as_deref().unwrap()).unwrap();
        assert_eq!(settings["defaultProvider"], PROVIDER_ID);
        assert_eq!(settings["defaultModel"], "claude-sonnet-5-5");

        let applied = with(&start, next);
        let current = Adapter.read(&applied).unwrap();
        assert!(current.ours);
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.token.as_deref(), Some("sk-relay$1"));
        assert_eq!(current.model.as_deref(), Some("claude-sonnet-5-5"));

        let changed = with(
            &applied,
            Adapter.set_model(&applied, "claude-haiku-5-5").unwrap(),
        );
        assert_eq!(
            Adapter.read(&changed).unwrap().model.as_deref(),
            Some("claude-haiku-5-5")
        );
        assert!(changed[0].text().contains("\"claude-haiku-5-5\""));

        assert_eq!(
            Adapter.restore(&changed, &snapshot).unwrap(),
            vec![None, None]
        );
    }

    #[test]
    fn keeps_comments_and_other_providers_and_restores_the_originals() {
        let models_before = "{\n  // 我自己的模型\n  \"providers\": {\n    \"ollama\": { \"baseUrl\": \"http://localhost:11434/v1\", \"api\": \"openai-completions\", \"apiKey\": \"ollama\", \"models\": [{ \"id\": \"qwen\" }], },\n  },\n}\n";
        let settings_before = "{\n  \"defaultProvider\": \"ollama\",\n  \"defaultModel\": \"qwen\",\n  \"theme\": \"dark\"\n}\n";
        let start = docs(Some(models_before), Some(settings_before));
        assert!(!Adapter.read(&start).unwrap().ours);
        assert!(Adapter.read(&start).unwrap().other);
        let models = vec!["gpt-6.1-sol".to_string()];
        let (next, snapshot) = Adapter
            .apply(&start, &target("gpt-6.1-sol", &models), None)
            .unwrap();
        assert!(next[0].as_deref().unwrap().contains("// 我自己的模型"));
        assert!(next[0].as_deref().unwrap().contains("\"ollama\""));
        assert!(next[1].as_deref().unwrap().contains("\"theme\": \"dark\""));
        let applied = with(&start, next);
        let restored = Adapter.restore(&applied, &snapshot).unwrap();
        assert_eq!(restored[0].as_deref(), Some(models_before));
        assert_eq!(restored[1].as_deref(), Some(settings_before));
    }

    #[test]
    fn keys_are_written_literally() {
        assert_eq!(literal("!abc$d"), "$!abc$$d");
        assert_eq!(unliteral(&literal("!abc$d")), "!abc$d");
        assert_eq!(unliteral("sk-plain"), "sk-plain");
    }

    #[test]
    fn invalid_files_name_the_file() {
        let start = docs(Some("[1]"), None);
        let models = vec!["gpt-6.1-sol".to_string()];
        match Adapter.apply(&start, &target("gpt-6.1-sol", &models), None) {
            Err(ConfigError::InvalidFile(path)) => assert!(path.ends_with("models.json")),
            other => panic!("unexpected {other:?}"),
        }
    }
}
