//! Claude Code：在 `settings.json` 的 `env` 里写站点地址和 Key；在 `.claude.json` 里标记
//! 已完成首次引导，免得第一次启动就要求登录 Anthropic 账号。
//!
//! 「关键字段」决定请求发到哪、用什么鉴权、用哪个模型。配置时先把别家留下的关键字段清掉，
//! 否则 `ANTHROPIC_API_KEY`、`apiKeyHelper`、`model` 这类旧值会盖过我们写的。第一次配置前
//! 这些字段的原值记在快照里，还原时只放回它们，其余设置一概不动。
//! 关键字段清单参考了 cc-switch（MIT）对 Claude Code 2.1 的整理。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::ConfigError;

/// `env` 里整个前缀都属于连接和鉴权的。
const ENV_PREFIXES: [&str; 3] = ["ANTHROPIC_", "AWS_", "VERTEX_REGION_"];

/// `env` 里按名字列出的关键字段。`CLAUDE_CODE_USE_*` 不能按前缀匹配，同一前缀下还有
/// 和服务商无关的功能开关。
const ENV_KEYS: [&str; 15] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_GATEWAY",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL_FORCE",
    "CLOUD_ML_REGION",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_SCOPES",
    "CLAUDE_CODE_API_KEY_HELPER_TTL_MS",
];

/// 顶层的关键字段。
const TOP_KEYS: [&str; 13] = [
    "apiKeyHelper",
    "apiBaseUrl",
    "primaryModel",
    "smallFastModel",
    "apiKey",
    "model",
    "fallbackModel",
    "modelOverrides",
    "modelPicker",
    "advisorModel",
    "awsAuthRefresh",
    "awsCredentialExport",
    "gcpAuthRefresh",
];

fn is_key_env(key: &str) -> bool {
    ENV_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
        || ENV_KEYS.contains(&key)
        || (key.starts_with("CLAUDE_CODE_SKIP_") && key.ends_with("_AUTH"))
}

/// 第一次配置前关键字段的原值。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    file_existed: bool,
    had_env: bool,
    env: Map<String, Value>,
    top: Map<String, Value>,
}

pub struct Target<'a> {
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: Option<&'a str>,
}

/// 配置文件里现在的连接。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Current {
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
}

fn parse(text: Option<&str>) -> Result<Map<String, Value>, ConfigError> {
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        return Ok(Map::new());
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(root)) => Ok(root),
        _ => Err(ConfigError::Invalid),
    }
}

/// 和 Claude Code 自己写的格式一致：两个空格缩进。
fn render(root: Map<String, Value>) -> String {
    let mut text = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
    text.push('\n');
    text
}

/// 把关键字段从配置里拿掉，返回它们原来的值。
fn take_key_fields(root: &mut Map<String, Value>, file_existed: bool) -> Snapshot {
    let mut snapshot = Snapshot {
        file_existed,
        had_env: root.contains_key("env"),
        ..Snapshot::default()
    };
    if let Some(Value::Object(env)) = root.get_mut("env") {
        let keys: Vec<String> = env.keys().filter(|key| is_key_env(key)).cloned().collect();
        for key in keys {
            if let Some(value) = env.shift_remove(&key) {
                snapshot.env.insert(key, value);
            }
        }
    }
    for key in TOP_KEYS {
        if let Some(value) = root.shift_remove(key) {
            snapshot.top.insert(key.to_string(), value);
        }
    }
    snapshot
}

fn env_mut(root: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let env = root.entry("env").or_insert_with(|| json!({}));
    if !env.is_object() {
        *env = json!({});
    }
    env.as_object_mut().expect("env is an object")
}

/// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过就沿用它，
/// 这样还原总是回到第一次配置之前。返回新的文件内容和要保存的快照。
pub fn apply(
    text: Option<&str>,
    target: &Target,
    previous: Option<&Snapshot>,
) -> Result<(String, Snapshot), ConfigError> {
    let mut root = parse(text)?;
    let taken = take_key_fields(&mut root, text.is_some());
    let env = env_mut(&mut root);
    env.insert("ANTHROPIC_BASE_URL".into(), json!(target.base_url));
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(target.token));
    if let Some(model) = target.model {
        env.insert("ANTHROPIC_MODEL".into(), json!(model));
    }
    Ok((render(root), previous.cloned().unwrap_or(taken)))
}

/// 只改模型；`None` 表示用 Claude Code 自己的默认模型。
pub fn set_model(text: Option<&str>, model: Option<&str>) -> Result<String, ConfigError> {
    let mut root = parse(text)?;
    let env = env_mut(&mut root);
    match model {
        Some(model) => {
            env.insert("ANTHROPIC_MODEL".into(), json!(model));
        }
        None => {
            env.shift_remove("ANTHROPIC_MODEL");
        }
    }
    Ok(render(root))
}

/// 还原：撤掉关键字段，放回快照里的原值。返回 None 表示文件本来不存在、现在也没有别的内容，
/// 可以删掉。
pub fn restore(text: Option<&str>, snapshot: &Snapshot) -> Result<Option<String>, ConfigError> {
    let mut root = parse(text)?;
    take_key_fields(&mut root, true);
    if !snapshot.env.is_empty() {
        let env = env_mut(&mut root);
        for (key, value) in &snapshot.env {
            env.insert(key.clone(), value.clone());
        }
    }
    if !snapshot.had_env
        && root
            .get("env")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
    {
        root.shift_remove("env");
    }
    for (key, value) in &snapshot.top {
        root.insert(key.clone(), value.clone());
    }
    if !snapshot.file_existed && root.is_empty() {
        return Ok(None);
    }
    Ok(Some(render(root)))
}

/// 读出现在的连接（文件格式有误时返回 Invalid）。
pub fn read(text: Option<&str>) -> Result<Current, ConfigError> {
    let root = parse(text)?;
    let env = root.get("env").and_then(Value::as_object);
    let get = |key: &str| {
        env.and_then(|env| env.get(key))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    Ok(Current {
        base_url: get("ANTHROPIC_BASE_URL"),
        token: get("ANTHROPIC_AUTH_TOKEN").or_else(|| get("ANTHROPIC_API_KEY")),
        model: get("ANTHROPIC_MODEL").or_else(|| {
            root.get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
    })
}

/// `.claude.json`：还没标记过完成首次引导就补上。返回 None 表示不用改。
pub fn mark_onboarded(text: Option<&str>) -> Result<Option<String>, ConfigError> {
    let mut root = parse(text)?;
    if root.get("hasCompletedOnboarding").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    root.insert("hasCompletedOnboarding".into(), Value::Bool(true));
    Ok(Some(render(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Target = Target {
        base_url: "https://relay.example.com",
        token: "sk-relay",
        model: None,
    };

    fn value(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn writes_a_fresh_config() {
        let (text, snapshot) = apply(None, &TARGET, None).unwrap();
        assert_eq!(
            value(&text),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://relay.example.com", "ANTHROPIC_AUTH_TOKEN": "sk-relay" } })
        );
        assert!(text.ends_with("}\n"));
        assert!(!snapshot.file_existed);
        // 文件本来不存在：还原就删掉。
        assert_eq!(restore(Some(&text), &snapshot).unwrap(), None);
    }

    #[test]
    fn replaces_another_providers_key_fields_and_keeps_everything_else() {
        let before = r#"{
  "permissions": { "allow": ["Bash(ls)"] },
  "env": {
    "DEBUG": "1",
    "ANTHROPIC_API_KEY": "sk-old",
    "ANTHROPIC_BASE_URL": "https://old.example.com",
    "CLAUDE_CODE_USE_BEDROCK": "1",
    "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1"
  },
  "apiKeyHelper": "~/bin/key.sh",
  "model": "opus",
  "theme": "dark"
}"#;
        let target = Target {
            model: Some("claude-sonnet-5-5"),
            ..TARGET
        };
        let (text, snapshot) = apply(Some(before), &target, None).unwrap();
        let after = value(&text);
        assert_eq!(
            after,
            json!({
                "permissions": { "allow": ["Bash(ls)"] },
                "env": {
                    "DEBUG": "1",
                    "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1",
                    "ANTHROPIC_BASE_URL": "https://relay.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-relay",
                    "ANTHROPIC_MODEL": "claude-sonnet-5-5"
                },
                "theme": "dark"
            })
        );
        // 原来的顺序不变。
        let keys: Vec<&String> = after.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["permissions", "env", "theme"]);
        assert_eq!(
            read(Some(&text)).unwrap(),
            Current {
                base_url: Some("https://relay.example.com".into()),
                token: Some("sk-relay".into()),
                model: Some("claude-sonnet-5-5".into()),
            }
        );

        // 换站点再配一次：快照还是第一次之前的。
        let other = Target {
            base_url: "https://other.example.com",
            token: "sk-other",
            model: None,
        };
        let (text, again) = apply(Some(&text), &other, Some(&snapshot)).unwrap();
        assert_eq!(again, snapshot);
        assert_eq!(value(&text)["env"].get("ANTHROPIC_MODEL"), None);

        // 用户在这期间加的设置，还原后还在。
        let mut edited = value(&text);
        edited["env"]["NEW_SETTING"] = json!("x");
        let restored = restore(Some(&edited.to_string()), &snapshot)
            .unwrap()
            .unwrap();
        let mut expected = value(before);
        expected["env"]["NEW_SETTING"] = json!("x");
        assert_eq!(value(&restored), expected);
    }

    #[test]
    fn changes_only_the_model() {
        let (text, _) = apply(None, &TARGET, None).unwrap();
        let text = set_model(Some(&text), Some("claude-opus-5-5")).unwrap();
        assert_eq!(
            read(Some(&text)).unwrap().model.as_deref(),
            Some("claude-opus-5-5")
        );
        let text = set_model(Some(&text), None).unwrap();
        assert_eq!(read(Some(&text)).unwrap().model, None);
        assert_eq!(
            read(Some(&text)).unwrap().token.as_deref(),
            Some("sk-relay")
        );
    }

    #[test]
    fn refuses_to_touch_invalid_files() {
        assert_eq!(
            apply(Some("{ not json"), &TARGET, None).unwrap_err(),
            ConfigError::Invalid
        );
        assert_eq!(
            apply(Some("[1, 2]"), &TARGET, None).unwrap_err(),
            ConfigError::Invalid
        );
        assert_eq!(read(Some("{")).unwrap_err(), ConfigError::Invalid);
        // 空文件当作没有配置。
        assert!(apply(Some("  \n"), &TARGET, None).is_ok());
    }

    #[test]
    fn marks_onboarding_once() {
        let text = mark_onboarded(Some(r#"{"numStartups": 3}"#))
            .unwrap()
            .unwrap();
        assert_eq!(
            value(&text),
            json!({ "numStartups": 3, "hasCompletedOnboarding": true })
        );
        assert_eq!(mark_onboarded(Some(&text)).unwrap(), None);
        assert!(mark_onboarded(None).unwrap().is_some());
    }
}
