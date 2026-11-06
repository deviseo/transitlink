//! MiniMax Code（`mcode`）：在 `~/.minimax/config.yaml`（`MINIMAX_DATA_DIR`、`MAVIS_DATA_DIR` 可以换目录）的
//! `custom_provider` 里加一个 `transitlink`，再把 `defaultModel` 设成 `custom_provider:transitlink/<模型>`。
//!
//! - 一个服务商只用一种接口：选了 Claude 模型用 anthropic-messages（站点根地址），否则 openai-completions
//!   （地址带 `/v1`；中转站不一定认 developer 角色、store 和 max_completion_tokens，都关掉）。
//! - 一定写在 `custom_provider` 下：写在 `provider` 下会触发 MCode 的旧格式迁移，整个文件被重写。
//! - 换默认模型时删掉 `defaultModelVariant`、`defaultModelThinking`、`defaultModelContextWindow`
//!   （MCode 自己换模型时也这样），还原时放回。
//! - MCode（和 MiniMax 桌面版）改这个文件时拿 `<文件>.lock` 目录锁，10 秒以上算过期：锁着时不写。
//! - 老版本的数据在 `~/.mavis`，MCode 启动时搬到 `~/.minimax`。还没搬时先建了 `~/.minimax`，MCode 会把
//!   `~/.mavis` 改名备份（原来的登录、会话都不用了）：这时请用户先运行一次。
//!
//! 2026-11-06 对照 MiniMax Code 0.7.6（`@minimax-ai/code`）核实；写法参考了 cc-switch（MIT）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml::{Mapping, Value as Yaml};

use super::multi::{self, Doc, MultiFile, Target};
use super::{ConfigError, Connection, yaml_map};

pub const PROVIDER_ID: &str = "transitlink";
const PROVIDER: [&str; 2] = ["custom_provider", PROVIDER_ID];
const DEFAULT: &str = "defaultModel";
const VARIANTS: [&str; 3] = [
    "defaultModelVariant",
    "defaultModelThinking",
    "defaultModelContextWindow",
];
const PREFIX: &str = "custom_provider:transitlink/";
const DATA_DIR_ENV: [&str; 2] = ["MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"];

/// MCode 的数据目录。
pub fn data_dir(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    DATA_DIR_ENV
        .iter()
        .filter_map(|name| env(name))
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".minimax"))
}

/// 目录里有没有东西（和 MCode 一样：只有一个空的 `workspace` 也算空）。读不了时为 None。
fn has_data(dir: &Path) -> Option<bool> {
    let names: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect();
    Some(match names.as_slice() {
        [] => false,
        [only] if only == "workspace" => std::fs::read_dir(dir.join("workspace"))
            .ok()?
            .next()
            .is_some(),
        _ => true,
    })
}

/// MCode 还没把老目录搬过来：新目录不存在或是空的，老目录里有东西（已经是指向新目录的链接的不算）。
fn legacy_pending(primary: &Path, legacy: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (
        std::fs::canonicalize(legacy),
        std::fs::canonicalize(primary),
    ) && a == b
    {
        return false;
    }
    if has_data(legacy) != Some(true) {
        return false;
    }
    match std::fs::symlink_metadata(primary) {
        Err(_) => true,
        Ok(metadata) if metadata.is_dir() => has_data(primary) == Some(false),
        Ok(_) => false,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    file_existed: bool,
    provider: Option<String>,
    default_model: Option<String>,
    variants: Vec<(String, Option<String>)>,
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

fn claude(model: &str) -> bool {
    model.to_ascii_lowercase().contains("claude")
}

fn model_value(model: &str, anthropic: bool) -> Yaml {
    let (context, output) = super::opencode::limits(model);
    let mut limit = Mapping::new();
    limit.insert(key("context"), Yaml::Number(context.into()));
    limit.insert(key("output"), Yaml::Number(output.into()));
    let mut entry = Mapping::new();
    entry.insert(key("name"), key(model));
    entry.insert(key("limit"), Yaml::Mapping(limit));
    if !anthropic {
        let mut compat = Mapping::new();
        compat.insert(key("supportsDeveloperRole"), Yaml::Bool(false));
        compat.insert(key("supportsStore"), Yaml::Bool(false));
        compat.insert(key("maxTokensField"), key("max_tokens"));
        entry.insert(key("compat"), Yaml::Mapping(compat));
    }
    Yaml::Mapping(entry)
}

fn provider_value(target: &Target) -> Yaml {
    let anthropic = claude(target.model);
    let mut models: Vec<&str> = target.models.iter().map(String::as_str).collect();
    if !models.contains(&target.model) {
        models.insert(0, target.model);
    }
    let mut options = Mapping::new();
    let base = if anthropic {
        target.base_url.to_string()
    } else {
        format!("{}/v1", target.base_url)
    };
    options.insert(key("baseURL"), key(&base));
    options.insert(key("apiKey"), key(target.token));
    options.insert(key("authMode"), key("api-key"));
    let mut list = Mapping::new();
    for model in models {
        list.insert(key(model), model_value(model, anthropic));
    }
    let mut provider = Mapping::new();
    provider.insert(
        key("name"),
        key(&format!("TransitLink · {}", target.site_name)),
    );
    provider.insert(key("kind"), key("custom"));
    provider.insert(key("enabled"), Yaml::Bool(true));
    provider.insert(
        key("api"),
        key(if anthropic {
            "anthropic-messages"
        } else {
            "openai-completions"
        }),
    );
    provider.insert(key("options"), Yaml::Mapping(options));
    provider.insert(key("models"), Yaml::Mapping(list));
    Yaml::Mapping(provider)
}

fn parse(doc: &Doc) -> Result<Yaml, ConfigError> {
    yaml_map::parse(doc.text()).map_err(|error| doc.at(error))
}

fn set(doc: &Doc, text: &str, path: &[&str], value: Option<&Yaml>) -> Result<String, ConfigError> {
    yaml_map::set(text, path, value).map_err(|error| doc.at(error))
}

/// 默认模型设成 `model`，换模型时 MCode 自己会删的几项也删掉。
fn set_default(doc: &Doc, text: &str, model: &str) -> Result<String, ConfigError> {
    let mut next = set(
        doc,
        text,
        &[DEFAULT],
        Some(&key(&format!("{PREFIX}{model}"))),
    )?;
    for name in VARIANTS {
        next = set(doc, &next, &[name], None)?;
    }
    Ok(next)
}

pub struct Adapter;

impl MultiFile for Adapter {
    fn files(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, String)> {
        vec![(
            data_dir(home, env).join("config.yaml"),
            "config.yaml".into(),
        )]
    }

    fn busy(&self, docs: &[Doc]) -> bool {
        let real = std::fs::canonicalize(&docs[0].path).unwrap_or_else(|_| docs[0].path.clone());
        multi::fresh_lock(&PathBuf::from(format!("{}.lock", real.display())))
    }

    fn ready(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Result<(), ConfigError> {
        let overridden = DATA_DIR_ENV
            .iter()
            .any(|name| env(name).is_some_and(|value| !value.trim().is_empty()));
        if !overridden && legacy_pending(&home.join(".minimax"), &home.join(".mavis")) {
            return Err(ConfigError::RunFirst("mcode".into()));
        }
        Ok(())
    }

    fn apply(
        &self,
        docs: &[Doc],
        target: &Target,
        previous: Option<&Value>,
    ) -> Result<(Vec<Option<String>>, Value), ConfigError> {
        let file = &docs[0];
        let current = parse(file)?;
        let snapshot = multi::snapshot::<Snapshot>(previous).unwrap_or_else(|| Snapshot {
            file_existed: file.text.is_some(),
            provider: yaml_map::saved(&current, &PROVIDER),
            default_model: yaml_map::saved(&current, &[DEFAULT]),
            variants: VARIANTS
                .iter()
                .map(|name| (name.to_string(), yaml_map::saved(&current, &[name])))
                .collect(),
        });
        let text = set(file, file.text(), &PROVIDER, Some(&provider_value(target)))?;
        let text = set_default(file, &text, target.model)?;
        Ok((vec![Some(text)], multi::to_value(&snapshot)?))
    }

    fn set_model(&self, docs: &[Doc], model: &str) -> Result<Vec<Option<String>>, ConfigError> {
        let file = &docs[0];
        let current = parse(file)?;
        let provider = yaml_map::get(&current, &PROVIDER).ok_or(ConfigError::NotConfigured)?;
        let anthropic = provider.get("api").and_then(Yaml::as_str) == Some("anthropic-messages");
        let mut text = file.text().to_string();
        if provider
            .get("models")
            .and_then(|models| models.get(model))
            .is_none()
        {
            text = set(
                file,
                &text,
                &["custom_provider", PROVIDER_ID, "models", model],
                Some(&model_value(model, anthropic)),
            )?;
        }
        Ok(vec![Some(set_default(file, &text, model)?)])
    }

    fn restore(&self, docs: &[Doc], snapshot: &Value) -> Result<Vec<Option<String>>, ConfigError> {
        let snapshot = multi::snapshot::<Snapshot>(Some(snapshot)).unwrap_or_default();
        let file = &docs[0];
        let Some(text) = &file.text else {
            return Ok(vec![None]);
        };
        let mut next = set(
            file,
            text,
            &PROVIDER,
            yaml_map::from_saved(snapshot.provider.as_deref()).as_ref(),
        )?;
        next = set(
            file,
            &next,
            &[DEFAULT],
            yaml_map::from_saved(snapshot.default_model.as_deref()).as_ref(),
        )?;
        for (name, saved) in &snapshot.variants {
            next = set(
                file,
                &next,
                &[name],
                yaml_map::from_saved(saved.as_deref()).as_ref(),
            )?;
        }
        Ok(vec![
            (snapshot.file_existed || !yaml_map::is_empty(&next)).then_some(next),
        ])
    }

    fn read(&self, docs: &[Doc]) -> Result<Connection, ConfigError> {
        let current = parse(&docs[0])?;
        let default = yaml_map::get(&current, &[DEFAULT])
            .and_then(Yaml::as_str)
            .map(str::to_string);
        let model = default
            .as_deref()
            .and_then(|value| value.strip_prefix(PREFIX))
            .map(str::to_string);
        let ours = model.is_some();
        let options = yaml_map::get(&current, &PROVIDER)
            .and_then(|provider| provider.get("options"))
            .filter(|_| ours);
        let field = |name: &str| {
            options
                .and_then(|options| options.get(name))
                .and_then(Yaml::as_str)
                .map(str::to_string)
        };
        Ok(Connection {
            ours,
            other: default.is_some() && !ours,
            base_url: field("baseURL").map(|url| {
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            }),
            token: field("apiKey"),
            model,
            provider: default,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(text: Option<&str>) -> Vec<Doc> {
        vec![Doc {
            path: PathBuf::from("/home/a/.minimax/config.yaml"),
            backup: "config.yaml".into(),
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
    fn writes_a_custom_provider_and_restores_the_defaults() {
        let before = "# MiniMax 设置\ndefaultModel: minimax/MiniMax-M3\ndefaultModelThinking: high\nstatusLine: compact\n";
        let start = docs(Some(before));
        assert!(Adapter.read(&start).unwrap().other);
        let list = vec!["MiniMax-M3".to_string(), "gpt-6.1-sol".to_string()];
        let (next, snapshot) = Adapter
            .apply(&start, &target("MiniMax-M3", &list), None)
            .unwrap();
        let text = next[0].as_deref().unwrap();
        assert!(text.starts_with("# MiniMax 设置\n"));
        assert!(text.contains("defaultModel: custom_provider:transitlink/MiniMax-M3"));
        assert!(!text.contains("defaultModelThinking"));
        let parsed = yaml_map::parse(text).unwrap();
        let ours = yaml_map::get(&parsed, &PROVIDER).unwrap();
        assert_eq!(
            ours.get("api").and_then(Yaml::as_str),
            Some("openai-completions")
        );
        assert_eq!(
            ours["options"].get("baseURL").and_then(Yaml::as_str),
            Some("https://relay.example.com/v1")
        );
        assert_eq!(
            ours["models"]["gpt-6.1-sol"]["compat"]
                .get("maxTokensField")
                .and_then(Yaml::as_str),
            Some("max_tokens")
        );

        let applied = docs(Some(text));
        let current = Adapter.read(&applied).unwrap();
        assert!(current.ours);
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.model.as_deref(), Some("MiniMax-M3"));

        let switched = docs(Adapter.set_model(&applied, "MiniMax-M3-Lite").unwrap()[0].as_deref());
        assert_eq!(
            Adapter.read(&switched).unwrap().model.as_deref(),
            Some("MiniMax-M3-Lite")
        );

        let restored = Adapter.restore(&switched, &snapshot).unwrap();
        let restored = restored[0].as_deref().unwrap();
        assert!(restored.contains("defaultModel: minimax/MiniMax-M3"));
        assert!(restored.contains("defaultModelThinking: high"));
        assert!(restored.contains("statusLine: compact"));
        assert!(!restored.contains("transitlink"));
    }

    #[test]
    fn claude_models_use_the_anthropic_api_at_the_site_root() {
        let list = vec!["claude-sonnet-5-5".to_string()];
        let (next, snapshot) = Adapter
            .apply(&docs(None), &target("claude-sonnet-5-5", &list), None)
            .unwrap();
        let parsed = yaml_map::parse(next[0].as_deref().unwrap()).unwrap();
        let ours = yaml_map::get(&parsed, &PROVIDER).unwrap();
        assert_eq!(
            ours.get("api").and_then(Yaml::as_str),
            Some("anthropic-messages")
        );
        assert_eq!(
            ours["options"].get("baseURL").and_then(Yaml::as_str),
            Some("https://relay.example.com")
        );
        assert_eq!(
            Adapter
                .restore(&docs(next[0].as_deref()), &snapshot)
                .unwrap(),
            vec![None]
        );
    }

    #[test]
    fn waits_for_mcode_to_move_its_old_data_dir() {
        let home =
            std::env::temp_dir().join(format!("transitlink-mcode-ready-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let none = |_: &str| None;
        assert!(Adapter.ready(&home, &none).is_ok(), "nothing to move");
        std::fs::create_dir_all(home.join(".mavis").join("sessions")).unwrap();
        assert!(matches!(
            Adapter.ready(&home, &none),
            Err(ConfigError::RunFirst(command)) if command == "mcode"
        ));
        // 新目录是空的也一样（MCode 会删掉空的新目录再搬）。
        std::fs::create_dir_all(home.join(".minimax").join("workspace")).unwrap();
        assert!(Adapter.ready(&home, &none).is_err());
        // 自己设了数据目录的不管。
        let custom = |name: &str| (name == "MINIMAX_DATA_DIR").then(|| "/srv/minimax".to_string());
        assert!(Adapter.ready(&home, &custom).is_ok());
        // 新目录里已经有东西：MCode 怎么处理老目录和我们写不写无关。
        std::fs::write(
            home.join(".minimax").join("config.yaml"),
            "statusLine: compact\n",
        )
        .unwrap();
        assert!(Adapter.ready(&home, &none).is_ok());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_bom_does_not_cost_the_comments() {
        let before = "\u{feff}# MiniMax 设置\nstatusLine: compact\n";
        let list = vec!["MiniMax-M3".to_string()];
        let (next, snapshot) = Adapter
            .apply(&docs(Some(before)), &target("MiniMax-M3", &list), None)
            .unwrap();
        let text = next[0].as_deref().unwrap();
        assert!(text.starts_with("# MiniMax 设置\nstatusLine: compact\n"));
        let restored = Adapter
            .restore(&docs(Some(&format!("\u{feff}{text}"))), &snapshot)
            .unwrap();
        assert_eq!(
            restored[0].as_deref(),
            Some("# MiniMax 设置\nstatusLine: compact\n")
        );
    }
}
