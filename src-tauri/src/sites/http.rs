//! 访问站点用的 HTTP 客户端：统一超时、User-Agent，把网络错误和限流转成 [`SiteError`]。
//! 和浏览器一样跟随系统的代理设置（以及 HTTP_PROXY 等环境变量）；测试里不走代理，结果和机器无关。

use std::time::Duration;

use reqwest::header::{HeaderMap, SET_COOKIE};
use serde_json::Value;

use super::SiteError;

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
}

/// 站点的响应：状态码、响应头和 JSON（不是 JSON 时为 None，例如前端页面）。
pub struct JsonResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Option<Value>,
}

/// 默认的 User-Agent。固定不变：sub2api 可以把会话绑定到 User-Agent，变了会被要求重新登录。
pub const USER_AGENT: &str = concat!("TransitLink/", env!("CARGO_PKG_VERSION"));

impl Http {
    pub fn new() -> Self {
        Self::with_user_agent(USER_AGENT)
    }

    /// 用指定的 User-Agent（在登录窗口里登录的站点，用那个窗口的 User-Agent）。
    pub fn with_user_agent(user_agent: &str) -> Self {
        let builder = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25));
        #[cfg(test)]
        let builder = builder.no_proxy();
        let client = builder.build().expect("HTTP client");
        Self { client }
    }

    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    pub fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    /// 发送请求并读取 JSON。连不上 → Unreachable；429 → RateLimited。
    pub async fn send(&self, request: reqwest::RequestBuilder) -> Result<JsonResponse, SiteError> {
        let response = request.send().await.map_err(|_| SiteError::Unreachable)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response.bytes().await.map_err(|_| SiteError::Unreachable)?;
        if status == 429 {
            return Err(SiteError::RateLimited);
        }
        Ok(JsonResponse {
            status,
            headers,
            body: serde_json::from_slice(&bytes).ok(),
        })
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

/// 把 Set-Cookie 里的 `名=值` 拼成请求用的 Cookie 头（去掉 Path、Expires 等属性）。
pub fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(str::trim)
        .filter(|pair| pair.contains('=') && !pair.ends_with('='))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 取 JSON 里的字符串字段（数字也转成字符串）。
pub fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// 取 JSON 里的数字字段（字符串形式的数字也可以）。
pub fn number(value: &Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn builds_cookie_header_from_set_cookie() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("session=abc; Path=/; HttpOnly"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("other=1; Max-Age=3600"),
        );
        headers.append(SET_COOKIE, HeaderValue::from_static("cleared=; Max-Age=0"));
        assert_eq!(cookie_header(&headers), "session=abc; other=1");
    }

    #[test]
    fn reads_loosely_typed_fields() {
        let value = serde_json::json!({"id": 12, "name": "a", "quota": "1500", "on": true});
        assert_eq!(text(&value, "id").as_deref(), Some("12"));
        assert_eq!(number(&value, "quota"), Some(1500.0));
        assert!(flag(&value, "on"));
        assert!(!flag(&value, "missing"));
    }
}
