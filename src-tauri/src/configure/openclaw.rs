//! OpenClaw（`openclaw`）：在 `openclaw.json`（JSON5）的 `models.providers` 里加一个 `transitlink` 服务商，
//! 再把 `agents.defaults.model` 的主模型设成 `transitlink/<模型>`。
//!
//! - 位置：`OPENCLAW_CONFIG_PATH` > `$OPENCLAW_STATE_DIR/openclaw.json` >（`OPENCLAW_HOME` 或主目录）下的
//!   `.openclaw/openclaw.json`。配置档（`.openclaw-X/`）只在命令行带 `--profile X` 时用，光设
//!   `OPENCLAW_PROFILE` 环境变量不换目录。
//! - 配置有严格的格式检查，多一个不认识的键 Gateway 就起不来：只写它认识的字段，模型的 `id`、`name` 都要写。
//! - 一个服务商可以按模型换接口：Claude 模型走 anthropic-messages（站点根地址），名字有 codex 的走
//!   openai-responses，其他走 openai-completions（加 `compat.supportsUsageInStreaming`，否则统计不到用量）。
//! - 老配置把 `agents.defaults.models` 当允许列表：加上我们的模型；`agents.defaults.modelPolicy.allow`
//!   不为空时加上 `transitlink/*`。
//! - Gateway 要 `gateway.mode = "local"` 才会启动（引导流程写的）：没有时补上。
//! - `models`、`agents` 由 `$include` 引入的不改。OpenClaw 自己改这个文件时会去掉注释。
//!
//! 2026-11-06 对照 OpenClaw 2026.11.1 核实；写法参考了 cc-switch（MIT）。

use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::multi::{self, Doc, MultiFile, Target};
use super::{ConfigError, Connection, jsonc};

pub const PROVIDER_ID: &str = "transitlink";
const PREFIX: &str = "transitlink/";
const WILDCARD: &str = "transitlink/*";

fn non_empty(env: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    env(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// OpenClaw 的配置文件。
pub fn config_path(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(path) = non_empty(env, "OPENCLAW_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    if let Some(dir) = non_empty(env, "OPENCLAW_STATE_DIR") {
        return PathBuf::from(dir).join("openclaw.json");
    }
    non_empty(env, "OPENCLAW_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf())
        .join(".openclaw")
        .join("openclaw.json")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    file_existed: bool,
    provider: Option<String>,
    /// `agents.defaults.model` 原来的值（JSON 文本）。
    model: Option<String>,
    /// 我们加进允许列表的模型。
    allowed: Vec<String>,
    policy_added: bool,
    gateway_added: bool,
}

fn model_entry(model: &str, root: &str) -> CstInputValue {
    let (context, output) = super::opencode::limits(model);
    let lower = model.to_ascii_lowercase();
    let string = |value: &str| CstInputValue::String(value.to_string());
    let mut entry = vec![
        ("id".to_string(), string(model)),
        ("name".to_string(), string(model)),
    ];
    if lower.contains("claude") {
        entry.push(("api".into(), string("anthropic-messages")));
        entry.push(("baseUrl".into(), string(root)));
    } else {
        if lower.contains("codex") {
            entry.push(("api".into(), string("openai-responses")));
        }
        entry.push((
            "compat".into(),
            CstInputValue::Object(vec![(
                "supportsUsageInStreaming".into(),
                CstInputValue::Bool(true),
            )]),
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
    CstInputValue::Object(vec![
        (
            "baseUrl".into(),
            CstInputValue::String(format!("{}/v1", target.base_url)),
        ),
        (
            "api".into(),
            CstInputValue::String("openai-completions".into()),
        ),
        ("apiKey".into(), CstInputValue::String(target.token.into())),
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

fn parse(doc: &Doc) -> Result<CstRootNode, ConfigError> {
    let root = jsonc::parse(doc.text.as_deref()).map_err(|error| doc.at(error))?;
    // `models`、`agents` 由 `$include` 引入的：我们改了也不一定生效。
    if let Some(object) = root.object_value() {
        let included = object.get("$include").is_some()
            || ["models", "agents"].iter().any(|name| {
                object
                    .object_value(name)
                    .is_some_and(|child| child.get("$include").is_some())
            });
        if included {
            return Err(doc.unsupported());
        }
    }
    Ok(root)
}

/// 现在的主模型（`agents.defaults.model` 可以是字符串，也可以是 `{ primary }`）。
fn primary(object: &CstObject) -> Option<String> {
    let defaults = object.object_value("agents")?.object_value("defaults")?;
    match jsonc::value_at(&defaults, "model")? {
        Value::String(model) => Some(model),
        Value::Object(map) => map
            .get("primary")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// 主模型设成 `reference`（原来是 `{ primary, fallbacks }` 的只改 primary）。
fn set_primary(object: &CstObject, reference: &str) {
    let defaults = object
        .object_value_or_set("agents")
        .object_value_or_set("defaults");
    match defaults.object_value("model") {
        Some(model) => jsonc::set(&model, "primary", CstInputValue::String(reference.into())),
        None => jsonc::set(&defaults, "model", CstInputValue::String(reference.into())),
    }
}

/// 允许列表里加上这个模型，返回是不是我们加的。
fn allow(object: &CstObject, reference: &str) -> bool {
    let defaults = object
        .object_value_or_set("agents")
        .object_value_or_set("defaults");
    let Some(allowed) = defaults.object_value("models") else {
        return false;
    };
    if allowed.get(reference).is_some() {
        return false;
    }
    allowed.append(reference, CstInputValue::Object(Vec::new()));
    true
}

pub struct Adapter;

impl MultiFile for Adapter {
    fn files(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, String)> {
        vec![(config_path(home, env), "openclaw.json".into())]
    }

    fn apply(
        &self,
        docs: &[Doc],
        target: &Target,
        previous: Option<&Value>,
    ) -> Result<(Vec<Option<String>>, Value), ConfigError> {
        let file = &docs[0];
        let root = parse(file)?;
        let object = root.object_value_or_set();
        let mut snapshot = multi::snapshot::<Snapshot>(previous).unwrap_or_else(|| Snapshot {
            file_existed: file.text.is_some(),
            provider: object
                .object_value("models")
                .and_then(|models| models.object_value("providers"))
                .and_then(|providers| jsonc::saved(&providers, PROVIDER_ID)),
            model: object
                .object_value("agents")
                .and_then(|agents| agents.object_value("defaults"))
                .and_then(|defaults| jsonc::saved(&defaults, "model")),
            ..Snapshot::default()
        });
        let providers = object
            .object_value_or_set("models")
            .object_value_or_set("providers");
        jsonc::set(&providers, PROVIDER_ID, provider_entry(target));
        let reference = format!("{PREFIX}{}", target.model);
        set_primary(&object, &reference);
        if allow(&object, &reference) {
            snapshot.allowed.push(reference);
        }
        if let Some(policy) = object
            .object_value("agents")
            .and_then(|agents| agents.object_value("defaults"))
            .and_then(|defaults| defaults.object_value("modelPolicy"))
            .and_then(|policy| policy.array_value("allow"))
            && !policy.elements().is_empty()
            && !policy
                .elements()
                .iter()
                .any(|element| element.to_serde_value() == Some(Value::String(WILDCARD.into())))
        {
            policy.append(CstInputValue::String(WILDCARD.into()));
            snapshot.policy_added = true;
        }
        let gateway = object.object_value_or_set("gateway");
        if gateway.get("mode").is_none() {
            gateway.append("mode", CstInputValue::String("local".into()));
            snapshot.gateway_added = true;
        }
        Ok((vec![Some(root.to_string())], multi::to_value(&snapshot)?))
    }

    fn set_model(&self, docs: &[Doc], model: &str) -> Result<Vec<Option<String>>, ConfigError> {
        let file = &docs[0];
        let root = parse(file)?;
        let object = root.object_value().ok_or(ConfigError::NotConfigured)?;
        let ours = object
            .object_value("models")
            .and_then(|models| models.object_value("providers"))
            .and_then(|providers| providers.object_value(PROVIDER_ID))
            .ok_or(ConfigError::NotConfigured)?;
        let base = jsonc::string_at(&ours, "baseUrl").unwrap_or_default();
        let site = base
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        let list = ours.array_value_or_set("models");
        let known = list
            .elements()
            .iter()
            .filter_map(|element| element.to_serde_value())
            .any(|value| value.get("id").and_then(Value::as_str) == Some(model));
        if !known {
            list.ensure_multiline();
            list.append(model_entry(model, &site));
        }
        let reference = format!("{PREFIX}{model}");
        set_primary(&object, &reference);
        allow(&object, &reference);
        Ok(vec![Some(root.to_string())])
    }

    fn restore(&self, docs: &[Doc], snapshot: &Value) -> Result<Vec<Option<String>>, ConfigError> {
        let snapshot = multi::snapshot::<Snapshot>(Some(snapshot)).unwrap_or_default();
        let file = &docs[0];
        let Some(text) = &file.text else {
            return Ok(vec![None]);
        };
        let root = parse(&Doc {
            path: file.path.clone(),
            backup: file.backup.clone(),
            text: Some(text.clone()),
        })?;
        if let Some(object) = root.object_value() {
            if let Some(models) = object.object_value("models") {
                if let Some(providers) = models.object_value("providers") {
                    jsonc::put_back(&providers, PROVIDER_ID, snapshot.provider.as_deref());
                }
                jsonc::remove_if_empty(&models, "providers");
                jsonc::remove_if_empty(&object, "models");
            }
            if let Some(agents) = object.object_value("agents") {
                if let Some(defaults) = agents.object_value("defaults") {
                    // 原来是 `{ primary, fallbacks }` 的只放回 primary，其余的写法不动。
                    let saved = snapshot
                        .model
                        .as_deref()
                        .and_then(|text| serde_json::from_str::<Value>(text).ok());
                    match (defaults.object_value("model"), saved) {
                        (Some(model), Some(Value::Object(saved))) => match saved.get("primary") {
                            Some(primary) => {
                                jsonc::set(&model, "primary", jsonc::input_value(primary))
                            }
                            None => {
                                if let Some(prop) = model.get("primary") {
                                    prop.remove();
                                }
                            }
                        },
                        _ => jsonc::put_back(&defaults, "model", snapshot.model.as_deref()),
                    }
                    if let Some(allowed) = defaults.object_value("models") {
                        // 我们加的，以及换模型时加的（都以 transitlink/ 开头、值是空对象）。
                        let ours: Vec<String> = allowed
                            .properties()
                            .iter()
                            .filter_map(|prop| {
                                prop.name().and_then(|name| name.decoded_value().ok())
                            })
                            .filter(|name| name.starts_with(PREFIX))
                            .collect();
                        for name in ours {
                            let added = snapshot.allowed.contains(&name)
                                || jsonc::value_at(&allowed, &name)
                                    == Some(Value::Object(Default::default()));
                            if added && let Some(prop) = allowed.get(&name) {
                                prop.remove();
                            }
                        }
                    }
                    if snapshot.policy_added
                        && let Some(policy) = defaults
                            .object_value("modelPolicy")
                            .and_then(|policy| policy.array_value("allow"))
                    {
                        for element in policy.elements() {
                            if element.to_serde_value() == Some(Value::String(WILDCARD.into())) {
                                element.remove();
                            }
                        }
                    }
                    jsonc::remove_if_empty(&agents, "defaults");
                }
                jsonc::remove_if_empty(&object, "agents");
            }
            // `gateway.mode` 留着：之后装的 Gateway 服务要靠它才能启动。只有文件是我们建的、而且只剩
            // 这一项时才去掉（文件会整个删掉）。
            if snapshot.gateway_added
                && !snapshot.file_existed
                && object.properties().len() == 1
                && jsonc::value_at(&object, "gateway")
                    == Some(serde_json::json!({ "mode": "local" }))
                && let Some(prop) = object.get("gateway")
            {
                prop.remove();
            }
        }
        Ok(vec![
            (snapshot.file_existed || !jsonc::is_empty(&root)).then(|| root.to_string()),
        ])
    }

    fn read(&self, docs: &[Doc]) -> Result<Connection, ConfigError> {
        let root = parse(&docs[0])?;
        let Some(object) = root.object_value() else {
            return Ok(Connection::default());
        };
        let reference = primary(&object);
        let model = reference
            .as_deref()
            .and_then(|value| value.strip_prefix(PREFIX))
            .map(str::to_string);
        let ours = model.is_some();
        let entry = object
            .object_value("models")
            .and_then(|models| models.object_value("providers"))
            .and_then(|providers| providers.object_value(PROVIDER_ID))
            .filter(|_| ours);
        Ok(Connection {
            ours,
            other: reference.is_some() && !ours,
            base_url: entry
                .as_ref()
                .and_then(|entry| jsonc::string_at(entry, "baseUrl"))
                .map(|url| {
                    url.trim_end_matches('/')
                        .trim_end_matches("/v1")
                        .to_string()
                }),
            token: entry
                .as_ref()
                .and_then(|entry| jsonc::string_at(entry, "apiKey")),
            model,
            provider: reference,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(text: Option<&str>) -> Vec<Doc> {
        vec![Doc {
            path: PathBuf::from("/home/a/.openclaw/openclaw.json"),
            backup: "openclaw.json".into(),
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
    fn writes_the_provider_primary_allowlist_and_restores() {
        let before = "{\n  // 我的设置\n  gateway: { mode: \"local\", port: 18789 },\n  agents: {\n    defaults: {\n      model: { primary: \"anthropic/claude-opus-4-6\", fallbacks: [\"openai/gpt-5.4\"] },\n      models: { \"anthropic/claude-opus-4-6\": {} },\n      modelPolicy: { allow: [\"anthropic/*\"] },\n    },\n  },\n}\n";
        let start = docs(Some(before));
        assert!(Adapter.read(&start).unwrap().other);
        let list = vec!["claude-sonnet-5-5".to_string(), "gpt-6.1-sol".to_string()];
        let (next, snapshot) = Adapter
            .apply(&start, &target("claude-sonnet-5-5", &list), None)
            .unwrap();
        let text = next[0].as_deref().unwrap();
        assert!(text.contains("// 我的设置"));
        assert!(text.contains("\"transitlink/claude-sonnet-5-5\""));
        assert!(text.contains("\"transitlink/*\""));
        assert!(text.contains("fallbacks"), "fallbacks stay");

        let applied = docs(Some(text));
        let current = Adapter.read(&applied).unwrap();
        assert!(current.ours);
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.model.as_deref(), Some("claude-sonnet-5-5"));

        let switched = docs(Adapter.set_model(&applied, "claude-haiku-5-5").unwrap()[0].as_deref());
        assert_eq!(
            Adapter.read(&switched).unwrap().model.as_deref(),
            Some("claude-haiku-5-5")
        );

        // 内容回到原样（jsonc-parser 改过的对象可能换成多行写法，OpenClaw 自己保存时也会重排）。
        let restored = Adapter.restore(&switched, &snapshot).unwrap();
        let restored = restored[0].as_deref().unwrap();
        assert!(restored.contains("// 我的设置"));
        assert!(
            restored.contains("fallbacks: [\"openai/gpt-5.4\"]"),
            "untouched parts keep their layout"
        );
        let value = |text: &str| {
            CstRootNode::parse(text, &jsonc_parser::ParseOptions::default())
                .unwrap()
                .value()
                .unwrap()
                .to_serde_value()
        };
        assert_eq!(value(restored), value(before));
    }

    #[test]
    fn a_fresh_config_gets_a_local_gateway_and_is_removed_again() {
        let list = vec!["gpt-6.1-sol".to_string()];
        let (next, snapshot) = Adapter
            .apply(&docs(None), &target("gpt-6.1-sol", &list), None)
            .unwrap();
        let value: Value = serde_json::from_str(next[0].as_deref().unwrap()).unwrap();
        assert_eq!(value["gateway"]["mode"], "local");
        assert_eq!(
            value["agents"]["defaults"]["model"],
            "transitlink/gpt-6.1-sol"
        );
        let entry = &value["models"]["providers"]["transitlink"]["models"][0];
        assert_eq!(entry["name"], "gpt-6.1-sol");
        assert_eq!(entry["compat"]["supportsUsageInStreaming"], true);
        assert_eq!(
            Adapter
                .restore(&docs(next[0].as_deref()), &snapshot)
                .unwrap(),
            vec![None]
        );
    }

    #[test]
    fn included_sections_are_not_changed() {
        let list = vec!["gpt-6.1-sol".to_string()];
        assert!(matches!(
            Adapter.apply(
                &docs(Some("{ models: { $include: \"./models.json5\" } }")),
                &target("gpt-6.1-sol", &list),
                None
            ),
            Err(ConfigError::Unsupported(_))
        ));
    }

    #[test]
    fn profiles_and_overrides_pick_the_file() {
        let home = Path::new("/home/a");
        let none = |_: &str| None;
        assert_eq!(
            config_path(home, &none),
            PathBuf::from("/home/a/.openclaw/openclaw.json")
        );
        // OpenClaw 只认命令行的 --profile：光设环境变量还是默认位置。
        let profile = |name: &str| (name == "OPENCLAW_PROFILE").then(|| "work".to_string());
        assert_eq!(
            config_path(home, &profile),
            PathBuf::from("/home/a/.openclaw/openclaw.json")
        );
        let state = |name: &str| (name == "OPENCLAW_STATE_DIR").then(|| "/srv/claw".to_string());
        assert_eq!(
            config_path(home, &state),
            PathBuf::from("/srv/claw/openclaw.json")
        );
    }

    #[test]
    fn restoring_keeps_the_gateway_mode_a_service_may_need() {
        let before = "{\n  // 我的设置\n  agents: { defaults: { model: \"anthropic/claude-opus-4-6\" } },\n}\n";
        let list = vec!["gpt-6.1-sol".to_string()];
        let (next, snapshot) = Adapter
            .apply(&docs(Some(before)), &target("gpt-6.1-sol", &list), None)
            .unwrap();
        let restored = Adapter
            .restore(&docs(next[0].as_deref()), &snapshot)
            .unwrap();
        let restored = restored[0].as_deref().unwrap();
        assert!(restored.contains("// 我的设置"));
        let value = CstRootNode::parse(restored, &jsonc_parser::ParseOptions::default())
            .unwrap()
            .value()
            .unwrap()
            .to_serde_value()
            .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "agents": { "defaults": { "model": "anthropic/claude-opus-4-6" } },
                "gateway": { "mode": "local" },
            })
        );
    }
}
