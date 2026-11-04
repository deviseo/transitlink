//! 连通测试：用工具里配置的地址和 Key 请求 `/v1/models`。不消耗额度，能确认地址对、Key 有效、
//! 分组里有这个工具能用的模型。顺便给 Codex 挑一个默认模型。

use serde::Serialize;
use serde_json::Value;

use crate::sites::SiteError;
use crate::sites::http::{Http, text};
use crate::tools::ToolKind;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TestResult {
    /// 连接正常。`family` 是适合这个工具的模型数。
    Ok { models: usize, family: usize },
    /// 连上了，但分组里没有适合这个工具的模型（Claude Code 要 Claude 模型，Codex 要 GPT 模型）。
    #[serde(rename_all = "camelCase")]
    NoFamilyModels { models: usize },
    /// Key 无效、停用或额度用完。
    BadKey { message: Option<String> },
    /// 连不上或站点返回了错误。
    Failed { message: Option<String> },
}

impl TestResult {
    pub fn passed(&self) -> bool {
        matches!(self, TestResult::Ok { .. })
    }
}

/// 这个模型适不适合该工具。
pub fn fits(kind: ToolKind, id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    match kind {
        ToolKind::ClaudeCode => id.contains("claude"),
        ToolKind::Codex => id.starts_with("gpt-") || id.contains("codex"),
    }
}

/// 取模型列表。出错时返回能直接显示的测试结果。
pub async fn list_models(
    http: &Http,
    base_url: &str,
    token: &str,
) -> Result<Vec<String>, TestResult> {
    let request = http
        .get(&format!("{}/v1/models", base_url.trim_end_matches('/')))
        .bearer_auth(token);
    let response = match http.send(request).await {
        Ok(response) => response,
        Err(SiteError::RateLimited) => {
            return Err(TestResult::Failed {
                message: Some("429".into()),
            });
        }
        Err(_) => return Err(TestResult::Failed { message: None }),
    };
    let message = response.body.as_ref().and_then(error_message);
    if matches!(response.status, 401 | 403) {
        return Err(TestResult::BadKey { message });
    }
    let Some(body) = response.body.filter(|_| response.status < 400) else {
        return Err(TestResult::Failed {
            message: message.or_else(|| Some(format!("HTTP {}", response.status))),
        });
    };
    let mut ids: Vec<String> = body
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| text(model, "id"))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// `{"error": {"message": ...}}`（OpenAI 格式）或 `{"message": ...}`。
fn error_message(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| text(error, "message").or_else(|| error.as_str().map(str::to_string)))
        .or_else(|| text(body, "message"))
        .filter(|message| !message.trim().is_empty())
}

/// 测试：列出模型，看有没有适合这个工具的。
pub async fn test(http: &Http, kind: ToolKind, base_url: &str, token: &str) -> TestResult {
    match list_models(http, base_url, token).await {
        Ok(ids) => summarize(kind, &ids),
        Err(result) => result,
    }
}

pub fn summarize(kind: ToolKind, ids: &[String]) -> TestResult {
    let family = ids.iter().filter(|id| fits(kind, id)).count();
    if family == 0 {
        TestResult::NoFamilyModels { models: ids.len() }
    } else {
        TestResult::Ok {
            models: ids.len(),
            family,
        }
    }
}

/// 给 Codex 挑默认模型：版本号最高的 GPT 模型，不要 mini、nano 这类小型号和专用型号，
/// 同等条件下名字短的优先（`gpt-5.5` 优于 `gpt-5.5-2026-08-01`）。站长推荐的模型以后由
/// TransitHub 下发，优先于这里。
pub fn pick_codex_model(ids: &[String]) -> Option<String> {
    const SPECIAL: [&str; 13] = [
        "mini",
        "nano",
        "lite",
        "chat",
        "audio",
        "realtime",
        "image",
        "search",
        "transcribe",
        "tts",
        "oss",
        "instruct",
        "preview",
    ];
    ids.iter()
        .filter(|id| fits(ToolKind::Codex, id))
        .max_by_key(|id| {
            let lower = id.to_ascii_lowercase();
            let general = !SPECIAL.iter().any(|word| lower.contains(word));
            (
                general,
                gpt_version(&lower),
                std::cmp::Reverse(lower.len()),
                std::cmp::Reverse(lower.clone()),
            )
        })
        .cloned()
}

/// `gpt-6.1-sol` → `[6, 1]`；没有版本号时为空。
fn gpt_version(id: &str) -> Vec<u32> {
    id.strip_prefix("gpt-")
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
        })
        .unwrap_or_default()
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// 给界面的模型列表：适合这个工具的排在前面。
pub fn order_for(kind: ToolKind, mut ids: Vec<String>) -> Vec<String> {
    ids.sort_by_key(|id| !fits(kind, id));
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn picks_the_newest_general_gpt_model() {
        let list = ids(&[
            "claude-sonnet-5-5",
            "gpt-5.5",
            "gpt-6.1-terra",
            "gpt-6.1-sol",
            "gpt-6.1-mini",
            "gpt-6-astra",
            "gpt-image-2",
            "text-embedding-3-large",
        ]);
        assert_eq!(pick_codex_model(&list).as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(
            pick_codex_model(&ids(&["gpt-5.5-2026-08-01", "gpt-5.5"])).as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(
            pick_codex_model(&ids(&["gpt-5-mini"])).as_deref(),
            Some("gpt-5-mini"),
            "a small model is still better than none"
        );
        assert_eq!(pick_codex_model(&ids(&["claude-opus-5-5"])), None);
    }

    #[test]
    fn summarizes_models_per_tool() {
        let list = ids(&["claude-opus-5-5", "gpt-6.1-sol", "gpt-5.5"]);
        assert_eq!(
            summarize(ToolKind::ClaudeCode, &list),
            TestResult::Ok {
                models: 3,
                family: 1
            }
        );
        assert_eq!(
            summarize(ToolKind::Codex, &ids(&["claude-opus-5-5"])),
            TestResult::NoFamilyModels { models: 1 }
        );
        assert_eq!(
            order_for(ToolKind::Codex, list),
            ids(&["gpt-6.1-sol", "gpt-5.5", "claude-opus-5-5"])
        );
    }

    #[tokio::test]
    async fn tests_the_key_against_the_models_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer sk-good"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [{ "id": "claude-opus-5-5" }, { "id": "claude-sonnet-5-5" }, { "id": "gpt-6.1-sol" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer sk-bad"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": { "message": "无效的令牌", "type": "new_api_error" }
            })))
            .mount(&server)
            .await;

        let http = Http::new();
        assert_eq!(
            test(&http, ToolKind::ClaudeCode, &server.uri(), "sk-good").await,
            TestResult::Ok {
                models: 3,
                family: 2
            }
        );
        assert_eq!(
            test(&http, ToolKind::Codex, &server.uri(), "sk-bad").await,
            TestResult::BadKey {
                message: Some("无效的令牌".into())
            }
        );
        assert_eq!(
            test(&http, ToolKind::Codex, "http://127.0.0.1:9", "sk").await,
            TestResult::Failed { message: None }
        );
    }
}
