//! Oh My Pi（`omp`）：在 `~/.omp/agent/models.yml` 里加一个 `transitlink` 服务商，再在 `config.yml` 里把
//! `modelRoles.default` 设成 `transitlink/<模型>`。
//!
//! - 配置目录：设了 `OMP_PROFILE`（或 `PI_PROFILE`）时是 `~/.omp/profiles/<名字>/agent`；否则
//!   `PI_CODING_AGENT_DIR`，再否则 `~/.omp/agent`（`PI_CONFIG_DIR` 可以换掉 `.omp`）。
//! - 模型文件优先 `models.yml`，其次 `models.yaml`。只有旧的 `models.json`（OMP 还没迁移）时不写：新建
//!   models.yml 会让 OMP 不再迁移，用户原来的服务商就没了。
//! - 一个服务商可以按模型换接口：地址带 `/v1`（Anthropic 接口 OMP 自己会去掉），Claude 模型走
//!   anthropic-messages，名字有 codex 的走 openai-responses，其他走 openai-completions。`auth: apiKey`
//!   一定要写，不写的话 Anthropic 请求会冒充 Claude Code 的格式。
//! - models.yml 里有一个服务商不合法，OMP 会停用所有自定义服务商：只写它认识的字段。
//! - 第一次运行的设置向导会改默认模型：还没做过向导的（`setupVersion` 小于 2）记成做过。
//!
//! 2026-11-06 对照 Oh My Pi 18.16.0（`@oh-my-pi/pi-coding-agent`）源码核实；写法参考了 cc-switch（MIT）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml::{Mapping, Value as Yaml};

use super::multi::{self, Doc, MultiFile, Target};
use super::{ConfigError, Connection, yaml_map};

pub const PROVIDER_ID: &str = "transitlink";
const SETUP_VERSION: u64 = 2;

fn non_empty(env: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    env(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// OMP 的配置目录。
pub fn agent_dir(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let base = home.join(non_empty(env, "PI_CONFIG_DIR").unwrap_or_else(|| ".omp".into()));
    let profile = non_empty(env, "OMP_PROFILE")
        .or_else(|| non_empty(env, "PI_PROFILE"))
        .filter(|name| name != "default");
    match profile {
        Some(name) => base.join("profiles").join(name).join("agent"),
        None => non_empty(env, "PI_CODING_AGENT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.join("agent")),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    models_existed: bool,
    provider: Option<String>,
    config_existed: bool,
    default_role: Option<String>,
    setup_version: Option<String>,
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

fn model_value(model: &str) -> Yaml {
    let (context, output) = super::opencode::limits(model);
    let lower = model.to_ascii_lowercase();
    let mut entry = Mapping::new();
    entry.insert(key("id"), key(model));
    if lower.contains("claude") {
        entry.insert(key("api"), key("anthropic-messages"));
    } else if lower.contains("codex") {
        entry.insert(key("api"), key("openai-responses"));
    }
    entry.insert(key("contextWindow"), Yaml::Number(context.into()));
    entry.insert(key("maxTokens"), Yaml::Number(output.into()));
    Yaml::Mapping(entry)
}

fn provider_value(target: &Target) -> Yaml {
    let mut models: Vec<&str> = target.models.iter().map(String::as_str).collect();
    if !models.contains(&target.model) {
        models.insert(0, target.model);
    }
    let mut compat = Mapping::new();
    compat.insert(key("supportsStore"), Yaml::Bool(false));
    compat.insert(key("supportsDeveloperRole"), Yaml::Bool(false));
    compat.insert(key("maxTokensField"), key("max_tokens"));
    let mut provider = Mapping::new();
    provider.insert(key("baseUrl"), key(&format!("{}/v1", target.base_url)));
    provider.insert(key("apiKey"), key(target.token));
    provider.insert(key("api"), key("openai-completions"));
    provider.insert(key("auth"), key("apiKey"));
    provider.insert(key("compat"), Yaml::Mapping(compat));
    provider.insert(
        key("models"),
        Yaml::Sequence(models.into_iter().map(model_value).collect()),
    );
    Yaml::Mapping(provider)
}

const PROVIDER: [&str; 2] = ["providers", PROVIDER_ID];
const DEFAULT_ROLE: [&str; 2] = ["modelRoles", "default"];

fn parse(doc: &Doc) -> Result<Yaml, ConfigError> {
    yaml_map::parse(doc.text()).map_err(|error| doc.at(error))
}

fn set(doc: &Doc, text: &str, path: &[&str], value: Option<&Yaml>) -> Result<String, ConfigError> {
    yaml_map::set(text, path, value).map_err(|error| doc.at(error))
}

pub struct Adapter;

impl MultiFile for Adapter {
    fn files(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, String)> {
        let dir = agent_dir(home, env);
        let models = ["models.yml", "models.yaml"]
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| dir.join("models.yml"));
        vec![
            (models, "models.yml".into()),
            (dir.join("config.yml"), "config.yml".into()),
            (dir.join("models.json"), "models.json".into()),
        ]
    }

    fn apply(
        &self,
        docs: &[Doc],
        target: &Target,
        previous: Option<&Value>,
    ) -> Result<(Vec<Option<String>>, Value), ConfigError> {
        let (models, config, legacy) = (&docs[0], &docs[1], &docs[2]);
        if models.text.is_none() && legacy.text.is_some() {
            return Err(legacy.unsupported());
        }
        let models_doc = parse(models)?;
        let config_doc = parse(config)?;
        let snapshot = multi::snapshot::<Snapshot>(previous).unwrap_or_else(|| Snapshot {
            models_existed: models.text.is_some(),
            provider: yaml_map::saved(&models_doc, &PROVIDER),
            config_existed: config.text.is_some(),
            default_role: yaml_map::saved(&config_doc, &DEFAULT_ROLE),
            setup_version: yaml_map::saved(&config_doc, &["setupVersion"]),
        });
        let models_text = set(
            models,
            models.text(),
            &PROVIDER,
            Some(&provider_value(target)),
        )?;
        let role = key(&format!("{PROVIDER_ID}/{}", target.model));
        let mut config_text = set(config, config.text(), &DEFAULT_ROLE, Some(&role))?;
        let done = yaml_map::get(&config_doc, &["setupVersion"])
            .and_then(Yaml::as_u64)
            .is_some_and(|version| version >= SETUP_VERSION);
        if !done {
            config_text = set(
                config,
                &config_text,
                &["setupVersion"],
                Some(&Yaml::Number(SETUP_VERSION.into())),
            )?;
        }
        Ok((
            vec![Some(models_text), Some(config_text), legacy.text.clone()],
            multi::to_value(&snapshot)?,
        ))
    }

    fn set_model(&self, docs: &[Doc], model: &str) -> Result<Vec<Option<String>>, ConfigError> {
        let (models, config, legacy) = (&docs[0], &docs[1], &docs[2]);
        let models_doc = parse(models)?;
        let mut list = yaml_map::get(&models_doc, &PROVIDER)
            .and_then(|provider| provider.get("models"))
            .and_then(Yaml::as_sequence)
            .cloned()
            .ok_or(ConfigError::NotConfigured)?;
        let mut models_text = models.text().to_string();
        if !list
            .iter()
            .any(|entry| entry.get("id").and_then(Yaml::as_str) == Some(model))
        {
            list.push(model_value(model));
            models_text = set(
                models,
                &models_text,
                &["providers", PROVIDER_ID, "models"],
                Some(&Yaml::Sequence(list)),
            )?;
        }
        let role = key(&format!("{PROVIDER_ID}/{model}"));
        let config_text = set(config, config.text(), &DEFAULT_ROLE, Some(&role))?;
        Ok(vec![
            Some(models_text),
            Some(config_text),
            legacy.text.clone(),
        ])
    }

    fn restore(&self, docs: &[Doc], snapshot: &Value) -> Result<Vec<Option<String>>, ConfigError> {
        let snapshot = multi::snapshot::<Snapshot>(Some(snapshot)).unwrap_or_default();
        let (models, config, legacy) = (&docs[0], &docs[1], &docs[2]);
        let models_next = match &models.text {
            None => None,
            Some(text) => {
                let restored = set(
                    models,
                    text,
                    &PROVIDER,
                    yaml_map::from_saved(snapshot.provider.as_deref()).as_ref(),
                )?;
                (snapshot.models_existed || !yaml_map::is_empty(&restored)).then_some(restored)
            }
        };
        let config_next = match &config.text {
            None => None,
            Some(text) => {
                let role = yaml_map::from_saved(snapshot.default_role.as_deref());
                let mut restored = set(config, text, &DEFAULT_ROLE, role.as_ref())?;
                let version = yaml_map::from_saved(snapshot.setup_version.as_deref());
                restored = set(config, &restored, &["setupVersion"], version.as_ref())?;
                (snapshot.config_existed || !yaml_map::is_empty(&restored)).then_some(restored)
            }
        };
        Ok(vec![models_next, config_next, legacy.text.clone()])
    }

    fn read(&self, docs: &[Doc]) -> Result<Connection, ConfigError> {
        let models_doc = parse(&docs[0])?;
        let config_doc = parse(&docs[1])?;
        let role = yaml_map::get(&config_doc, &DEFAULT_ROLE).and_then(Yaml::as_str);
        let (provider, model) = match role.and_then(|role| role.split_once('/')) {
            Some((provider, model)) => (Some(provider.to_string()), Some(model.to_string())),
            None => (role.map(str::to_string), None),
        };
        let ours = provider.as_deref() == Some(PROVIDER_ID);
        let entry = yaml_map::get(&models_doc, &PROVIDER).filter(|_| ours);
        let field = |name: &str| {
            entry
                .and_then(|entry| entry.get(name))
                .and_then(Yaml::as_str)
        };
        Ok(Connection {
            ours,
            other: provider.is_some() && !ours,
            base_url: field("baseUrl").map(|url| {
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            }),
            token: field("apiKey").map(str::to_string),
            model: model.filter(|_| ours),
            provider,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(models: Option<&str>, config: Option<&str>, legacy: Option<&str>) -> Vec<Doc> {
        [
            ("models.yml", models),
            ("config.yml", config),
            ("models.json", legacy),
        ]
        .into_iter()
        .map(|(name, text)| Doc {
            path: PathBuf::from(format!("/home/a/.omp/agent/{name}")),
            backup: name.into(),
            text: text.map(str::to_string),
        })
        .collect()
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
    fn writes_reads_switches_and_restores() {
        let models_before = "# 我的服务商\nproviders:\n  local:\n    baseUrl: http://localhost:8080/v1\n    apiKey: none\n    api: openai-completions\n    models:\n      - id: qwen\n";
        let config_before = "modelRoles:\n  default: local/qwen\ntheme: dark\nsetupVersion: 2\n";
        let start = docs(Some(models_before), Some(config_before), None);
        assert!(Adapter.read(&start).unwrap().other);
        let list = vec![
            "claude-sonnet-5-5".to_string(),
            "claude-haiku-5-5".to_string(),
        ];
        let (next, snapshot) = Adapter
            .apply(&start, &target("claude-sonnet-5-5", &list), None)
            .unwrap();
        let models = next[0].as_deref().unwrap();
        assert!(
            models.starts_with("# 我的服务商\nproviders:\n  local:\n"),
            "other providers and comments stay"
        );
        let parsed = yaml_map::parse(models).unwrap();
        let ours = yaml_map::get(&parsed, &PROVIDER).unwrap();
        assert_eq!(ours.get("auth").and_then(Yaml::as_str), Some("apiKey"));
        assert_eq!(
            ours.get("baseUrl").and_then(Yaml::as_str),
            Some("https://relay.example.com/v1")
        );
        assert_eq!(
            ours["models"][0].get("api").and_then(Yaml::as_str),
            Some("anthropic-messages")
        );
        assert!(
            next[1]
                .as_deref()
                .unwrap()
                .contains("default: transitlink/claude-sonnet-5-5")
        );
        assert!(next[1].as_deref().unwrap().contains("theme: dark"));

        let applied = with(&start, next);
        let current = Adapter.read(&applied).unwrap();
        assert!(current.ours);
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.model.as_deref(), Some("claude-sonnet-5-5"));

        let switched = with(
            &applied,
            Adapter.set_model(&applied, "claude-opus-5-5").unwrap(),
        );
        assert_eq!(
            Adapter.read(&switched).unwrap().model.as_deref(),
            Some("claude-opus-5-5")
        );
        assert!(switched[0].text().contains("claude-opus-5-5"));

        let restored = Adapter.restore(&switched, &snapshot).unwrap();
        assert_eq!(restored[0].as_deref(), Some(models_before));
        assert_eq!(restored[1].as_deref(), Some(config_before));
    }

    #[test]
    fn a_fresh_setup_skips_the_wizard_and_restores_to_nothing() {
        let start = docs(None, None, None);
        let list = vec!["gpt-6.1-sol".to_string()];
        let (next, snapshot) = Adapter
            .apply(&start, &target("gpt-6.1-sol", &list), None)
            .unwrap();
        assert!(next[1].as_deref().unwrap().contains("setupVersion: 2"));
        let applied = with(&start, next);
        assert_eq!(
            Adapter.restore(&applied, &snapshot).unwrap(),
            vec![None, None, None]
        );
    }

    #[test]
    fn an_unmigrated_models_json_is_left_alone() {
        let start = docs(None, None, Some("{\"providers\": {}}"));
        let list = vec!["gpt-6.1-sol".to_string()];
        assert!(matches!(
            Adapter.apply(&start, &target("gpt-6.1-sol", &list), None),
            Err(ConfigError::Unsupported(what)) if what.ends_with("models.json")
        ));
    }

    #[test]
    fn profiles_and_overrides_pick_the_directory() {
        let home = Path::new("/home/a");
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.to_string())
            }
        };
        assert_eq!(
            agent_dir(home, &env(&[])),
            PathBuf::from("/home/a/.omp/agent")
        );
        assert_eq!(
            agent_dir(home, &env(&[("OMP_PROFILE", "work")])),
            PathBuf::from("/home/a/.omp/profiles/work/agent")
        );
        assert_eq!(
            agent_dir(home, &env(&[("PI_CODING_AGENT_DIR", "/x/agent")])),
            PathBuf::from("/x/agent")
        );
    }
}
