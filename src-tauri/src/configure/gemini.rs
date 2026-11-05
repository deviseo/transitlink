//! Gemini CLI：在 `~/.gemini/.env` 里写站点地址（`GOOGLE_GEMINI_BASE_URL`，站点根地址，Gemini 自己接
//! `/v1beta/models/...`）、Key（`GEMINI_API_KEY`，以 `x-goog-api-key` 发送）和模型（`GEMINI_MODEL`）；
//! 再在 `settings.json` 里把登录方式设成 API Key（`security.auth.selectedType = "gemini-api-key"`），
//! 否则 Gemini 会弹出登录选择，或者把站点地址当成 gateway 拒绝。
//!
//! `.env` 只改这几行，其他行和注释原样保留；`settings.json` 允许注释（JSONC），用 jsonc-parser
//! 只改这一个键，其余内容和注释不动。原值都记在快照里，还原时放回。
//!
//! 2026-11-05 对照 Gemini CLI 0.67.0 源码核实。注意：Gemini 只在用户信任的文件夹里读这个 `.env`，
//! 当前目录或上层目录有别的 `.env` 时也不读它（只认找到的第一个）。

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use serde::{Deserialize, Serialize};

use super::ConfigError;

const BASE_URL: &str = "GOOGLE_GEMINI_BASE_URL";
const API_KEY: &str = "GEMINI_API_KEY";
const MODEL: &str = "GEMINI_MODEL";
const ENV_KEYS: [&str; 3] = [BASE_URL, API_KEY, MODEL];

/// 用 API Key 登录。
pub const AUTH_TYPE: &str = "gemini-api-key";

/// 第一次配置前的原样。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    file_existed: bool,
    /// 每个变量原来的整行（可能出现不止一次）。
    lines: Vec<(String, Vec<String>)>,
    /// `settings.json` 原来的登录方式（写 `settings.json` 后填上）。
    #[serde(default)]
    pub auth: Option<AuthSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSnapshot {
    file_existed: bool,
    selected_type: Option<String>,
}

pub struct Target<'a> {
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: &'a str,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Current {
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
}

/// `.env` 一行里的变量名（注释、空行、格式不对的行返回 None）。允许前面有 `export `。
fn line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let trimmed = trimmed
        .strip_prefix("export ")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    let valid = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    valid.then_some(key)
}

/// `.env` 一行里的值：引号里的原样取出；没有引号的到 ` #` 注释为止。
fn line_value(line: &str) -> Option<String> {
    let (_, raw) = line.split_once('=')?;
    let raw = raw.trim();
    for quote in ['"', '\'', '`'] {
        if let Some(rest) = raw.strip_prefix(quote) {
            return rest.find(quote).map(|end| rest[..end].to_string());
        }
    }
    let value = match raw.find(" #") {
        Some(end) => &raw[..end],
        None => raw,
    };
    Some(value.trim().to_string())
}

/// 写成 `KEY=value`；值里有空格、引号或 `#` 时加双引号。
fn render_line(key: &str, value: &str) -> String {
    if value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '`'))
    {
        format!("{key}=\"{}\"", value.replace('"', "\\\""))
    } else {
        format!("{key}={value}")
    }
}

fn split_lines(text: Option<&str>) -> (Vec<String>, &'static str) {
    let text = text.unwrap_or_default();
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    (text.lines().map(str::to_string).collect(), newline)
}

fn join_lines(lines: &[String], newline: &str) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join(newline);
    out.push_str(newline);
    out
}

/// 把这些变量改成新值：第一次出现的地方原地替换，重复出现的删掉，原来没有的加在最后。
fn set_vars(lines: &mut Vec<String>, vars: &[(&str, &str)]) {
    let mut written: Vec<&str> = Vec::new();
    let mut out = Vec::with_capacity(lines.len() + vars.len());
    for line in lines.drain(..) {
        match line_key(&line).and_then(|key| vars.iter().find(|(name, _)| *name == key)) {
            Some((key, value)) => {
                if !written.contains(key) {
                    out.push(render_line(key, value));
                    written.push(key);
                }
            }
            None => out.push(line),
        }
    }
    for (key, value) in vars {
        if !written.contains(key) {
            out.push(render_line(key, value));
        }
    }
    *lines = out;
}

/// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过就沿用它，还原总是回到第一次之前。
pub fn apply(
    text: Option<&str>,
    target: &Target,
    previous: Option<&Snapshot>,
) -> Result<(String, Snapshot), ConfigError> {
    let (mut lines, newline) = split_lines(text);
    let taken = Snapshot {
        file_existed: text.is_some(),
        lines: ENV_KEYS
            .iter()
            .map(|key| {
                let original = lines
                    .iter()
                    .filter(|line| line_key(line) == Some(*key))
                    .cloned()
                    .collect();
                (key.to_string(), original)
            })
            .filter(|(_, original): &(String, Vec<String>)| !original.is_empty())
            .collect(),
        auth: None,
    };
    set_vars(
        &mut lines,
        &[
            (BASE_URL, target.base_url),
            (API_KEY, target.token),
            (MODEL, target.model),
        ],
    );
    Ok((
        join_lines(&lines, newline),
        previous.cloned().unwrap_or(taken),
    ))
}

/// 只改模型。
pub fn set_model(text: Option<&str>, model: &str) -> Result<String, ConfigError> {
    let (mut lines, newline) = split_lines(text);
    set_vars(&mut lines, &[(MODEL, model)]);
    Ok(join_lines(&lines, newline))
}

/// 还原：我们写的这几行换回原来的行（原来没有的删掉）。返回 None 表示文件本来不存在、
/// 现在也没有别的内容，可以删掉。
pub fn restore(text: Option<&str>, snapshot: &Snapshot) -> Result<Option<String>, ConfigError> {
    let (lines, newline) = split_lines(text);
    let mut out = Vec::with_capacity(lines.len());
    let mut restored: Vec<&'static str> = Vec::new();
    for line in lines {
        let Some(key) =
            line_key(&line).and_then(|key| ENV_KEYS.iter().copied().find(|name| *name == key))
        else {
            out.push(line);
            continue;
        };
        if restored.contains(&key) {
            continue;
        }
        restored.push(key);
        if let Some((_, original)) = snapshot.lines.iter().find(|(name, _)| name == key) {
            out.extend(original.iter().cloned());
        }
    }
    // 原来有、但现在文件里没有我们那一行的变量，也放回去。
    for (key, original) in &snapshot.lines {
        if !restored.contains(&key.as_str()) {
            out.extend(original.iter().cloned());
        }
    }
    let rendered = join_lines(&out, newline);
    if !snapshot.file_existed && rendered.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(rendered))
}

/// 读出现在的连接（同一个变量出现多次时，和 dotenv 一样以最后一次为准）。
pub fn read(text: Option<&str>) -> Result<Current, ConfigError> {
    let (lines, _) = split_lines(text);
    let get = |name: &str| {
        lines
            .iter()
            .rev()
            .find(|line| line_key(line) == Some(name))
            .and_then(|line| line_value(line))
            .filter(|value| !value.is_empty())
    };
    Ok(Current {
        base_url: get(BASE_URL).map(|url| url.trim_end_matches('/').to_string()),
        token: get(API_KEY),
        model: get(MODEL),
    })
}

fn parse_settings(text: &str) -> Result<CstRootNode, ConfigError> {
    let root =
        CstRootNode::parse(text, &ParseOptions::default()).map_err(|_| ConfigError::Invalid)?;
    // 顶层不是对象（例如写成了数组）：不去改它。
    if root.value().is_some() && root.object_value().is_none() {
        return Err(ConfigError::Invalid);
    }
    Ok(root)
}

/// `settings.json` 里设成用 API Key 登录，返回新内容和原来的登录方式。
pub fn set_auth(text: Option<&str>) -> Result<(String, AuthSnapshot), ConfigError> {
    let root = parse_settings(text.unwrap_or_default())?;
    let auth = root
        .object_value_or_set()
        .object_value_or_set("security")
        .object_value_or_set("auth");
    let previous = auth
        .get("selectedType")
        .and_then(|prop| prop.value())
        .and_then(|value| value.as_string_lit())
        .and_then(|value| value.decoded_value().ok());
    let wanted = CstInputValue::String(AUTH_TYPE.to_string());
    match auth.get("selectedType") {
        Some(prop) => prop.set_value(wanted),
        None => {
            auth.append("selectedType", wanted);
        }
    }
    Ok((
        root.to_string(),
        AuthSnapshot {
            file_existed: text.is_some(),
            selected_type: previous,
        },
    ))
}

/// 把 `settings.json` 的登录方式放回原值（原来没有就删掉这个键，空出来的 `auth`、`security` 也删掉）。
/// 返回 None 表示文件本来不存在、现在也没有别的内容，可以删掉。
pub fn restore_auth(
    text: Option<&str>,
    snapshot: &AuthSnapshot,
) -> Result<Option<String>, ConfigError> {
    let Some(text) = text else {
        return Ok(None);
    };
    let root = parse_settings(text)?;
    if let Some(security) = root
        .object_value()
        .and_then(|root| root.object_value("security"))
        && let Some(auth) = security.object_value("auth")
    {
        match &snapshot.selected_type {
            Some(previous) => {
                let value = CstInputValue::String(previous.clone());
                match auth.get("selectedType") {
                    Some(prop) => prop.set_value(value),
                    None => {
                        auth.append("selectedType", value);
                    }
                }
            }
            None => {
                if let Some(prop) = auth.get("selectedType") {
                    prop.remove();
                }
                if auth.properties().is_empty()
                    && let Some(prop) = security.get("auth")
                {
                    prop.remove();
                }
                if security.properties().is_empty()
                    && let Some(prop) = root.object_value().and_then(|root| root.get("security"))
                {
                    prop.remove();
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Target = Target {
        base_url: "https://relay.example.com",
        token: "sk-relay",
        model: "gemini-3.1-pro-preview",
    };

    #[test]
    fn writes_a_fresh_env_file() {
        let (text, snapshot) = apply(None, &TARGET, None).unwrap();
        assert_eq!(
            text,
            "GOOGLE_GEMINI_BASE_URL=https://relay.example.com\nGEMINI_API_KEY=sk-relay\nGEMINI_MODEL=gemini-3.1-pro-preview\n"
        );
        assert_eq!(
            read(Some(&text)).unwrap(),
            Current {
                base_url: Some("https://relay.example.com".into()),
                token: Some("sk-relay".into()),
                model: Some("gemini-3.1-pro-preview".into()),
            }
        );
        assert_eq!(restore(Some(&text), &snapshot).unwrap(), None);
    }

    #[test]
    fn keeps_other_lines_and_restores_the_originals() {
        let original = "# my keys\nOTHER=1\nexport GEMINI_API_KEY=\"old key\"\nGEMINI_MODEL=gemini-2.5-pro # pinned\n";
        let (text, snapshot) = apply(Some(original), &TARGET, None).unwrap();
        assert_eq!(
            text,
            "# my keys\nOTHER=1\nGEMINI_API_KEY=sk-relay\nGEMINI_MODEL=gemini-3.1-pro-preview\nGOOGLE_GEMINI_BASE_URL=https://relay.example.com\n"
        );
        // 已经配置过：沿用第一次的快照。
        let (again, kept) = apply(Some(&text), &TARGET, Some(&snapshot)).unwrap();
        assert_eq!(again, text);
        assert_eq!(kept, snapshot);

        let changed = set_model(Some(&text), "gemini-3.8-flash").unwrap();
        assert_eq!(
            read(Some(&changed)).unwrap().model.as_deref(),
            Some("gemini-3.8-flash")
        );
        assert_eq!(
            restore(Some(&changed), &snapshot).unwrap().as_deref(),
            Some(original)
        );
    }

    #[test]
    fn values_are_read_like_dotenv() {
        let text = "GEMINI_API_KEY='a b'\nGEMINI_MODEL=first\nGEMINI_MODEL=second # comment\n";
        let current = read(Some(text)).unwrap();
        assert_eq!(current.token.as_deref(), Some("a b"));
        assert_eq!(current.model.as_deref(), Some("second"));
        assert_eq!(current.base_url, None);
    }

    #[test]
    fn settings_keep_comments_and_get_restored() {
        let original = "{\n  // 我的设置\n  \"ui\": { \"theme\": \"GitHub\" },\n  \"security\": {\n    \"auth\": {\n      \"selectedType\": \"oauth-personal\"\n    }\n  }\n}\n";
        let (text, snapshot) = set_auth(Some(original)).unwrap();
        assert!(text.contains("// 我的设置"));
        assert!(text.contains("\"theme\": \"GitHub\""));
        assert!(text.contains("\"selectedType\": \"gemini-api-key\""));
        assert_eq!(snapshot.selected_type.as_deref(), Some("oauth-personal"));
        assert_eq!(
            restore_auth(Some(&text), &snapshot).unwrap().as_deref(),
            Some(original)
        );
    }

    #[test]
    fn settings_written_from_scratch_are_removed_again() {
        let (text, snapshot) = set_auth(None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["security"]["auth"]["selectedType"], AUTH_TYPE);
        assert_eq!(restore_auth(Some(&text), &snapshot).unwrap(), None);
        // 只加了登录方式的已有文件：还原后只去掉我们加的键。
        let (text, snapshot) = set_auth(Some("{\n  \"ui\": {}\n}\n")).unwrap();
        let restored = restore_auth(Some(&text), &snapshot).unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_str(&restored).unwrap();
        assert_eq!(value, serde_json::json!({ "ui": {} }));
    }

    #[test]
    fn settings_that_are_not_objects_are_left_alone() {
        assert_eq!(set_auth(Some("[1, 2]")), Err(ConfigError::Invalid));
        assert_eq!(set_auth(Some("{ \"a\": ")), Err(ConfigError::Invalid));
    }
}
