//! 中转站：识别站点类型、登录、读取账号和 Key。目前支持 new-api 和 sub2api。
//! 管理面板接口不支持跨域，所有请求都从这里发出，凭证不进入网页层。

pub mod crypto;
pub mod http;
pub mod newapi;
pub mod sub2api;

use serde::{Deserialize, Serialize};

use http::Http;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Newapi,
    Sub2api,
}

/// 识别出来的站点信息和登录要求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub platform: Platform,
    pub base_url: String,
    pub name: String,
    pub logo_url: Option<String>,
    /// 登录需要人机验证（Turnstile、腾讯或阿里云验证码），暂时只能在站点登录页完成。
    pub captcha_required: bool,
    /// 站点关闭了账号密码登录。
    pub password_login_disabled: bool,
    /// sub2api 只能用邮箱登录；new-api 用户名或邮箱都可以。
    pub email_only: bool,
    /// new-api：多少额度等于 1 美元（默认 500000）。
    pub quota_per_unit: Option<f64>,
    /// new-api：登录密码需要加密提交。
    #[serde(default)]
    pub password_encryption: bool,
    /// 在登录窗口里登录的站点：记下那个窗口的 User-Agent，以后的请求都用它
    /// （sub2api 可以把会话绑定到登录时的 IP 和 User-Agent）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

/// 返回给前端的错误：`code` 对应界面文案，`message` 是站点自己返回的说明（可能为空）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteError {
    InvalidUrl,
    Unreachable,
    Unsupported,
    BadCredentials(Option<String>),
    InvalidCode(Option<String>),
    CaptchaRequired,
    PasswordLoginDisabled,
    UnsupportedVerification,
    RateLimited,
    SessionExpired,
    /// 用户关掉了登录窗口（或者太久没登录完）。
    LoginCancelled,
    /// 专属链接失效了（站长关掉或重置了链接）。
    LinkNotFound,
    /// 专属链接指向的配置有问题（格式不对，或者站点地址和登录的站点不一致）。
    LinkInvalid,
    TokenNotAllowed(Option<String>),
    Site(Option<String>),
    Unexpected(String),
}

impl SiteError {
    pub fn code(&self) -> &'static str {
        match self {
            SiteError::InvalidUrl => "invalidUrl",
            SiteError::Unreachable => "unreachable",
            SiteError::Unsupported => "unsupported",
            SiteError::BadCredentials(_) => "badCredentials",
            SiteError::InvalidCode(_) => "invalidCode",
            SiteError::CaptchaRequired => "captchaRequired",
            SiteError::PasswordLoginDisabled => "passwordLoginDisabled",
            SiteError::UnsupportedVerification => "unsupportedVerification",
            SiteError::RateLimited => "rateLimited",
            SiteError::SessionExpired => "sessionExpired",
            SiteError::LoginCancelled => "loginCancelled",
            SiteError::LinkNotFound => "linkNotFound",
            SiteError::LinkInvalid => "linkInvalid",
            SiteError::TokenNotAllowed(_) => "tokenNotAllowed",
            SiteError::Site(_) => "site",
            SiteError::Unexpected(_) => "unexpected",
        }
    }

    pub fn message(&self) -> Option<String> {
        match self {
            SiteError::BadCredentials(message)
            | SiteError::InvalidCode(message)
            | SiteError::TokenNotAllowed(message)
            | SiteError::Site(message) => message.clone(),
            SiteError::Unexpected(detail) => Some(detail.clone()),
            _ => None,
        }
    }
}

/// 登录凭证，存进系统钥匙串。站点密码从不保存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Credential {
    /// 新版 new-api：长期有效、限定权限的个人访问令牌（nap_…）。
    #[serde(rename_all = "camelCase")]
    NewapiToken { token: String },
    /// 旧版 new-api：登录会话 cookie 和用户 ID。
    #[serde(rename_all = "camelCase")]
    NewapiLegacy { cookie: String, user_id: String },
    /// sub2api：访问令牌和刷新令牌（刷新令牌只能用一次，每次刷新都要保存新的）。
    #[serde(rename_all = "camelCase")]
    Sub2api {
        access_token: String,
        refresh_token: String,
    },
}

/// 一次登录尝试的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStep {
    Done {
        credential: Credential,
        account: String,
    },
    /// 需要两步验证码。new-api 新版给 flow_token；旧版用登录 cookie 继续；sub2api 给 temp_token。
    NeedLoginCode {
        flow_token: String,
        legacy_cookie: Option<String>,
    },
    /// new-api 创建个人访问令牌时需要一个新的两步验证码。
    NeedTokenCode { access_token: String },
    /// new-api 创建个人访问令牌时需要再输一次密码（在登录窗口里登录的，我们没有密码）。
    NeedTokenPassword { access_token: String },
}

/// 账号概况。余额统一换算成美元。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub name: String,
    pub group: Option<String>,
    pub balance_usd: f64,
}

/// 一个 API Key。列表里的 Key 可能是打码的（new-api），配置工具前再取完整的。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySummary {
    pub id: String,
    pub name: String,
    pub key: String,
    pub masked: bool,
    pub group: Option<String>,
    pub enabled: bool,
}

impl KeySummary {
    /// 给列表看的版本：完整 Key 只露出首尾几位，要用时再单独取（例如复制）。
    pub fn for_display(self) -> Self {
        if self.masked {
            return self;
        }
        KeySummary {
            key: mask_key(&self.key),
            masked: true,
            ..self
        }
    }
}

/// 打码方式和 new-api 一致：`sk-` 之后保留前 4 位和后 4 位。
pub fn mask_key(key: &str) -> String {
    let (prefix, body) = key.split_at(if key.starts_with("sk-") { 3 } else { 0 });
    let chars: Vec<char> = body.chars().collect();
    let masked: String = match chars.len() {
        0..=4 => "*".repeat(chars.len()),
        5..=8 => format!(
            "{}****{}",
            chars[..2].iter().collect::<String>(),
            chars[chars.len() - 2..].iter().collect::<String>()
        ),
        _ => format!(
            "{}**********{}",
            chars[..4].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        ),
    };
    format!("{prefix}{masked}")
}

/// 用户可选的分组。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub ratio: Option<f64>,
    /// sub2api 分组的平台（anthropic、openai、gemini……）；new-api 没有。
    pub platform: Option<String>,
}

/// 规范化站点地址：补全协议，只保留协议、主机和端口。
pub fn normalize_url(input: &str) -> Result<String, SiteError> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(SiteError::InvalidUrl);
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let url = url::Url::parse(&with_scheme).map_err(|_| SiteError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SiteError::InvalidUrl);
    }
    let host = url.host_str().ok_or(SiteError::InvalidUrl)?;
    Ok(match url.port() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    })
}

/// 识别站点类型：先按 sub2api 的公开设置接口识别，再按 new-api 的 /api/status 识别。
pub async fn detect(http: &Http, input: &str) -> Result<SiteInfo, SiteError> {
    let base_url = normalize_url(input)?;
    let mut unreachable = false;
    match sub2api::fetch_site(http, &base_url).await {
        Ok(Some(info)) => return Ok(info),
        Ok(None) => {}
        Err(SiteError::Unreachable) => unreachable = true,
        Err(error) => return Err(error),
    }
    match newapi::fetch_site(http, &base_url).await {
        Ok(Some(info)) => Ok(info),
        Ok(None) if unreachable => Err(SiteError::Unreachable),
        Ok(None) => Err(SiteError::Unsupported),
        Err(error) => Err(error),
    }
}

/// 把站点返回的 logo 地址转成绝对地址。
pub(crate) fn absolute_url(base_url: &str, value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") || value.starts_with("data:") {
        return Some(value.to_string());
    }
    let base = url::Url::parse(&format!("{base_url}/")).ok()?;
    base.join(value).ok().map(|url| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_site_addresses() {
        assert_eq!(
            normalize_url("relay.example.com").unwrap(),
            "https://relay.example.com"
        );
        assert_eq!(
            normalize_url(" https://relay.example.com/console/ ").unwrap(),
            "https://relay.example.com"
        );
        assert_eq!(
            normalize_url("http://127.0.0.1:3000/").unwrap(),
            "http://127.0.0.1:3000"
        );
        assert_eq!(
            normalize_url("ftp://relay.example.com"),
            Err(SiteError::InvalidUrl)
        );
        assert_eq!(normalize_url("   "), Err(SiteError::InvalidUrl));
    }

    #[test]
    fn resolves_logo_addresses() {
        let base = "https://relay.example.com";
        assert_eq!(
            absolute_url(base, Some("/logo.png")).as_deref(),
            Some("https://relay.example.com/logo.png")
        );
        assert_eq!(
            absolute_url(base, Some("https://cdn.example.com/a.png")).as_deref(),
            Some("https://cdn.example.com/a.png")
        );
        assert_eq!(absolute_url(base, Some("  ")), None);
        assert_eq!(absolute_url(base, None), None);
    }

    #[test]
    fn masks_keys_for_display() {
        assert_eq!(mask_key("sk-abcdefghijklmnopwxyz"), "sk-abcd**********wxyz");
        assert_eq!(mask_key("abcdefgh"), "ab****gh");
        assert_eq!(mask_key("sk-abc"), "sk-***");
        assert_eq!(mask_key(""), "");
        let key = KeySummary {
            id: "1".into(),
            name: "cc".into(),
            key: "sk-abcdefghijklmnopwxyz".into(),
            masked: false,
            group: None,
            enabled: true,
        };
        let shown = key.for_display();
        assert_eq!(shown.key, "sk-abcd**********wxyz");
        assert!(shown.masked);
        // 已经打过码的不再处理。
        assert_eq!(shown.clone().for_display().key, shown.key);
    }

    #[test]
    fn credentials_serialize_with_a_kind_tag() {
        let credential = Credential::Sub2api {
            access_token: "a".into(),
            refresh_token: "r".into(),
        };
        let json = serde_json::to_string(&credential).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"sub2api","accessToken":"a","refreshToken":"r"}"#
        );
        assert_eq!(
            serde_json::from_str::<Credential>(&json).unwrap(),
            credential
        );
    }
}
