//! OpenCode：在全局配置（`~/.config/opencode/opencode.jsonc`，没有就用 `opencode.json`）里加一个
//! `transitlink` 服务商（站点地址、Key、可选的模型），再把默认模型 `model` 设成 `transitlink/<模型>`。
//!
//! - Claude 模型走 Anthropic 接口（`@ai-sdk/anthropic`，请求 `<地址>/v1/messages`，Key 放 `x-api-key`）；
//!   其他模型走 OpenAI 兼容接口（`@ai-sdk/openai-compatible`，请求 `/v1/chat/completions`）。
//!   new-api 和 sub2api 都支持这两种。一个服务商只用一种接口，所以只列同一类的模型。
//! - 模型要写明上下文和输出上限，否则 OpenCode 默认要 32000 个输出 token，上限低的模型会拒绝。
//! - OpenCode 1 和 2 都认这种写法（v2 读到后在内存里转换）。配置文件允许注释，用 jsonc-parser
//!   只改这几个键，其余内容和注释不动。
//!
//! 2026-11-05 对照 OpenCode 1.19.5 和 2.2.5 源码核实。

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ConfigError;

/// 我们的服务商 ID（不和 OpenCode 内置的 `anthropic`、`openai` 等重名）。
pub const PROVIDER_ID: &str = "transitlink";
const SCHEMA: &str = "https://opencode.ai/config.json";
const ANTHROPIC: &str = "@ai-sdk/anthropic";
const OPENAI_COMPATIBLE: &str = "@ai-sdk/openai-compatible";

/// 第一次配置前这几个键的原值（JSON 文本），还原时放回。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    file_existed: bool,
    had_schema: bool,
    model: Option<String>,
    small_model: Option<String>,
    provider: Option<String>,
}

pub struct Target<'a> {
    pub site_name: &'a str,
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: &'a str,
    /// 站点上同一类（Claude 或其他）的模型，都写进服务商的模型列表。
    pub models: &'a [String],
    /// 起会话标题用的便宜模型（Claude 的 haiku、GPT 的 mini 等），没有时不设。
    pub small_model: Option<&'a str>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Current {
    /// `model` 里斜杠前面的服务商 ID。
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
    /// 我们的服务商走 Anthropic 接口（只能选 Claude 模型）。
    pub anthropic: bool,
}

/// 这个模型走 Anthropic 接口。
pub fn is_anthropic(model: &str) -> bool {
    model.to_ascii_lowercase().contains("claude")
}

/// 模型的上下文和输出上限（OpenCode 要求两个都写）。按模型名粗略判断，宁小勿大：
/// 输出上限超过模型本身的上限会被拒绝，上下文写小了只是早一点压缩对话。
pub(crate) fn limits(model: &str) -> (u64, u64) {
    let id = model.to_ascii_lowercase();
    if id.contains("claude") {
        (200_000, 32_000)
    } else if id.starts_with("gpt-5") || id.starts_with("gpt-6") || id.contains("codex") {
        (400_000, 32_000)
    } else if id.contains("gemini") {
        (1_000_000, 32_000)
    } else if id.contains("grok-4") {
        (256_000, 32_000)
    } else {
        (128_000, 8_192)
    }
}

fn model_entry(model: &str) -> CstInputValue {
    let (context, output) = limits(model);
    CstInputValue::Object(vec![
        ("name".into(), CstInputValue::String(model.to_string())),
        (
            "limit".into(),
            CstInputValue::Object(vec![
                ("context".into(), CstInputValue::Number(context.to_string())),
                ("output".into(), CstInputValue::Number(output.to_string())),
            ]),
        ),
    ])
}

fn parse(text: Option<&str>) -> Result<CstRootNode, ConfigError> {
    let root = CstRootNode::parse(text.unwrap_or_default(), &ParseOptions::default())
        .map_err(|_| ConfigError::Invalid)?;
    if root.value().is_some() && root.object_value().is_none() {
        return Err(ConfigError::Invalid);
    }
    Ok(root)
}

/// JSON 值转成可以写进 JSONC 的值（还原原值用）。
fn input_value(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(input_value).collect()),
        Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), input_value(value)))
                .collect(),
        ),
    }
}

/// 对象里某个键的值（JSON 文本），用来记原值。
fn saved(object: &CstObject, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(|prop| prop.value())
        .and_then(|value| value.to_serde_value())
        .map(|value| value.to_string())
}

fn string_at(object: &CstObject, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(|prop| prop.value())
        .and_then(|value| value.as_string_lit())
        .and_then(|value| value.decoded_value().ok())
}

/// 设置一个键：有就改，没有就加在最后（`first`：加在最前面）。
fn set(object: &CstObject, key: &str, value: CstInputValue, first: bool) {
    match object.get(key) {
        Some(prop) => prop.set_value(value),
        None if first => {
            object.insert(0, key, value);
        }
        None => {
            object.append(key, value);
        }
    }
}

/// 放回原值（原来没有就删掉这个键）。
fn put_back(object: &CstObject, key: &str, original: Option<&str>) {
    match original.and_then(|text| serde_json::from_str::<Value>(text).ok()) {
        Some(value) => set(object, key, input_value(&value), false),
        None => {
            if let Some(prop) = object.get(key) {
                prop.remove();
            }
        }
    }
}

fn provider_entry(target: &Target) -> CstInputValue {
    let npm = if is_anthropic(target.model) {
        ANTHROPIC
    } else {
        OPENAI_COMPATIBLE
    };
    let mut models: Vec<&str> = target.models.iter().map(String::as_str).collect();
    if !models.contains(&target.model) {
        models.insert(0, target.model);
    }
    CstInputValue::Object(vec![
        ("npm".into(), CstInputValue::String(npm.into())),
        (
            "name".into(),
            CstInputValue::String(format!("TransitLink · {}", target.site_name)),
        ),
        (
            "options".into(),
            CstInputValue::Object(vec![
                (
                    "baseURL".into(),
                    CstInputValue::String(format!("{}/v1", target.base_url)),
                ),
                ("apiKey".into(), CstInputValue::String(target.token.into())),
            ]),
        ),
        (
            "models".into(),
            CstInputValue::Object(
                models
                    .into_iter()
                    .map(|model| (model.to_string(), model_entry(model)))
                    .collect(),
            ),
        ),
    ])
}

/// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过就沿用它，还原总是回到第一次之前。
pub fn apply(
    text: Option<&str>,
    target: &Target,
    previous: Option<&Snapshot>,
) -> Result<(String, Snapshot), ConfigError> {
    let root = parse(text)?;
    let object = root.object_value_or_set();
    let taken = Snapshot {
        file_existed: text.is_some(),
        had_schema: object.get("$schema").is_some(),
        model: saved(&object, "model"),
        small_model: saved(&object, "small_model"),
        provider: object
            .object_value("provider")
            .and_then(|providers| saved(&providers, PROVIDER_ID)),
    };
    // 没有 $schema 时 OpenCode 1 会自己改文件加上，这里先加好。
    if object.get("$schema").is_none() {
        set(
            &object,
            "$schema",
            CstInputValue::String(SCHEMA.into()),
            true,
        );
    }
    set(
        &object,
        "model",
        CstInputValue::String(format!("{PROVIDER_ID}/{}", target.model)),
        false,
    );
    if let Some(small) = target.small_model {
        set(
            &object,
            "small_model",
            CstInputValue::String(format!("{PROVIDER_ID}/{small}")),
            false,
        );
    }
    let providers = object.object_value_or_set("provider");
    set(&providers, PROVIDER_ID, provider_entry(target), false);
    Ok((root.to_string(), previous.cloned().unwrap_or(taken)))
}

/// 换模型：设成默认模型，服务商的模型列表里没有就加上。
pub fn set_model(text: Option<&str>, model: &str) -> Result<String, ConfigError> {
    let root = parse(text)?;
    let object = root.object_value().ok_or(ConfigError::NotConfigured)?;
    let ours = object
        .object_value("provider")
        .and_then(|providers| providers.object_value(PROVIDER_ID))
        .ok_or(ConfigError::NotConfigured)?;
    let models = ours.object_value_or_set("models");
    if models.get(model).is_none() {
        models.append(model, model_entry(model));
    }
    set(
        &object,
        "model",
        CstInputValue::String(format!("{PROVIDER_ID}/{model}")),
        false,
    );
    Ok(root.to_string())
}

/// 还原：撤掉我们的服务商，`model`、`small_model`、`$schema` 放回原值。返回 None 表示文件本来
/// 不存在、现在也没有别的内容，可以删掉。
pub fn restore(text: Option<&str>, snapshot: &Snapshot) -> Result<Option<String>, ConfigError> {
    let root = parse(text)?;
    if let Some(object) = root.object_value() {
        if let Some(providers) = object.object_value("provider") {
            put_back(&providers, PROVIDER_ID, snapshot.provider.as_deref());
            if providers.properties().is_empty()
                && let Some(prop) = object.get("provider")
            {
                prop.remove();
            }
        }
        put_back(&object, "model", snapshot.model.as_deref());
        put_back(&object, "small_model", snapshot.small_model.as_deref());
        if !snapshot.had_schema
            && let Some(prop) = object.get("$schema")
        {
            prop.remove();
        }
    }
    let empty = root
        .object_value()
        .is_none_or(|object| object.properties().is_empty());
    if !snapshot.file_existed && empty {
        return Ok(None);
    }
    Ok(Some(root.to_string()))
}

/// 读出现在的连接（文件格式有误时返回 Invalid）。
pub fn read(text: Option<&str>) -> Result<Current, ConfigError> {
    let root = parse(text)?;
    let Some(object) = root.object_value() else {
        return Ok(Current::default());
    };
    let model = string_at(&object, "model");
    let (provider, model) = match model.as_deref().and_then(|model| model.split_once('/')) {
        Some((provider, name)) => (Some(provider.to_string()), Some(name.to_string())),
        None => (None, model),
    };
    let ours = object
        .object_value("provider")
        .and_then(|providers| providers.object_value(PROVIDER_ID))
        .filter(|_| provider.as_deref() == Some(PROVIDER_ID));
    let options = ours.as_ref().and_then(|ours| ours.object_value("options"));
    Ok(Current {
        base_url: options
            .as_ref()
            .and_then(|options| string_at(options, "baseURL"))
            .map(|url| {
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            }),
        token: options
            .as_ref()
            .and_then(|options| string_at(options, "apiKey")),
        anthropic: ours
            .as_ref()
            .and_then(|ours| string_at(ours, "npm"))
            .is_some_and(|npm| npm == ANTHROPIC),
        model: model.filter(|_| ours.is_some()),
        provider,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target<'a>(model: &'a str, models: &'a [String], small: Option<&'a str>) -> Target<'a> {
        Target {
            site_name: "示例中转站",
            base_url: "https://relay.example.com",
            token: "sk-relay",
            model,
            models,
            small_model: small,
        }
    }

    #[test]
    fn writes_a_fresh_config_for_claude_models() {
        let models = vec![
            "claude-sonnet-5-5".to_string(),
            "claude-haiku-5-5".to_string(),
        ];
        let (text, snapshot) = apply(
            None,
            &target("claude-sonnet-5-5", &models, Some("claude-haiku-5-5")),
            None,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["$schema"], SCHEMA);
        assert_eq!(value["model"], "transitlink/claude-sonnet-5-5");
        assert_eq!(value["small_model"], "transitlink/claude-haiku-5-5");
        let provider = &value["provider"]["transitlink"];
        assert_eq!(provider["npm"], ANTHROPIC);
        assert_eq!(
            provider["options"]["baseURL"],
            "https://relay.example.com/v1"
        );
        assert_eq!(provider["options"]["apiKey"], "sk-relay");
        assert_eq!(
            provider["models"]["claude-haiku-5-5"]["limit"]["output"],
            32000
        );
        assert_eq!(
            read(Some(&text)).unwrap(),
            Current {
                provider: Some(PROVIDER_ID.into()),
                base_url: Some("https://relay.example.com".into()),
                token: Some("sk-relay".into()),
                model: Some("claude-sonnet-5-5".into()),
                anthropic: true,
            }
        );
        assert_eq!(restore(Some(&text), &snapshot).unwrap(), None);
    }

    #[test]
    fn keeps_comments_and_other_providers_and_restores_the_originals() {
        let original = r#"{
  "$schema": "https://opencode.ai/config.json",
  // 我自己的服务商
  "provider": {
    "mine": { "npm": "@ai-sdk/openai-compatible", "options": { "baseURL": "http://localhost:8080/v1" }, "models": { "local": {} } }
  },
  "model": "mine/local",
  "autoupdate": false,
}
"#;
        let models = vec!["gpt-6.1-sol".to_string()];
        let (text, snapshot) =
            apply(Some(original), &target("gpt-6.1-sol", &models, None), None).unwrap();
        assert!(text.contains("// 我自己的服务商"));
        assert!(text.contains("\"autoupdate\": false"));
        assert!(text.contains("\"mine\""));
        let current = read(Some(&text)).unwrap();
        assert_eq!(current.model.as_deref(), Some("gpt-6.1-sol"));
        assert!(!current.anthropic);
        assert!(text.contains(OPENAI_COMPATIBLE));

        let changed = set_model(Some(&text), "gpt-6-astra").unwrap();
        assert_eq!(
            read(Some(&changed)).unwrap().model.as_deref(),
            Some("gpt-6-astra")
        );
        assert!(changed.contains("\"gpt-6-astra\": {"));

        assert_eq!(
            restore(Some(&changed), &snapshot).unwrap().as_deref(),
            Some(original)
        );
    }

    #[test]
    fn another_default_model_is_not_ours() {
        let models = vec!["claude-sonnet-5-5".to_string()];
        let (text, _) = apply(None, &target("claude-sonnet-5-5", &models, None), None).unwrap();
        let switched = text.replace(
            "transitlink/claude-sonnet-5-5",
            "anthropic/claude-sonnet-5-5",
        );
        let current = read(Some(&switched)).unwrap();
        assert_eq!(current.provider.as_deref(), Some("anthropic"));
        assert_eq!(current.base_url, None);
    }

    #[test]
    fn small_unknown_models_get_small_limits() {
        assert_eq!(limits("deepseek-v4"), (128_000, 8_192));
        assert_eq!(limits("claude-opus-5-5"), (200_000, 32_000));
    }

    #[test]
    fn invalid_files_are_reported() {
        assert_eq!(read(Some("[1]")), Err(ConfigError::Invalid));
        assert_eq!(read(Some("{ \"model\": ")), Err(ConfigError::Invalid));
    }
}
