//! new-api 的用户侧接口。
//!
//! 新版（2026-11 起）面板鉴权：登录返回 15 分钟的访问令牌；客户端再用密码（或两步验证码）
//! 换一次安全证明，创建限定权限、长期有效的个人访问令牌（`nap_…`），以后只用它。
//! 旧版：登录得到会话 cookie，请求时带上 cookie 和 `New-Api-User: <用户 ID>`。

use serde_json::{Value, json};

use super::http::{Http, JsonResponse, cookie_header, flag, number, text};
use super::{
    AccountSummary, Credential, GroupSummary, KeySummary, LoginStep, Platform, SiteError, SiteInfo,
    absolute_url, crypto,
};

/// 个人访问令牌的权限，只要用得到的：读账号、分组和模型；管理并查看完整 Key；读用量和钱包。
/// 不要 `wallet:write`（充值、划转），令牌长期保存在本机，权限越小越安全。
const TOKEN_SCOPES: [&str; 6] = [
    "api_key:read",
    "api_key:reveal",
    "api_key:write",
    "profile:read",
    "usage:read",
    "wallet:read",
];
const TOKEN_SCOPE_NAME: &str = "access_token.generate";
/// 新版网页登录后留下的刷新令牌 cookie（Path 是 `/api/user/auth`）。
pub const REFRESH_COOKIE: &str = "new_api_refresh";
const DEFAULT_QUOTA_PER_UNIT: f64 = 500_000.0;

/// 读取 /api/status 识别 new-api。不是 new-api 时返回 None。
pub async fn fetch_site(http: &Http, base_url: &str) -> Result<Option<SiteInfo>, SiteError> {
    let response = http
        .send(http.get(&format!("{base_url}/api/status")))
        .await?;
    let Some(body) = response.body else {
        return Ok(None);
    };
    let data = match body.get("data") {
        Some(data)
            if flag(&body, "success")
                && (data.get("quota_per_unit").is_some() || data.get("system_name").is_some()) =>
        {
            data
        }
        _ => return Ok(None),
    };
    let name = text(data, "system_name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "New API".to_string());
    Ok(Some(SiteInfo {
        platform: Platform::Newapi,
        base_url: base_url.to_string(),
        name,
        logo_url: absolute_url(base_url, text(data, "logo").as_deref()),
        captcha_required: flag(data, "turnstile_check"),
        // 旧版没有这个字段，视为允许。
        password_login_disabled: data.get("password_login_enabled").and_then(Value::as_bool)
            == Some(false),
        email_only: false,
        quota_per_unit: number(data, "quota_per_unit")
            .filter(|value| *value > 0.0)
            .or(Some(DEFAULT_QUOTA_PER_UNIT)),
        password_encryption: flag(data, "password_login_encryption_enabled"),
        user_agent: None,
    }))
}

/// 读取 new-api 的标准响应 `{success, message, data}`。
fn data_of(response: JsonResponse) -> Result<Value, SiteError> {
    if response.status == 401 || response.status == 403 {
        return Err(SiteError::SessionExpired);
    }
    let body = response
        .body
        .ok_or_else(|| SiteError::Unexpected(format!("HTTP {}", response.status)))?;
    if !flag(&body, "success") {
        return Err(SiteError::Site(
            text(&body, "message").filter(|message| !message.is_empty()),
        ));
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

/// 需要时加密密码，返回要放进请求体的字段。
async fn password_fields(http: &Http, info: &SiteInfo, password: &str) -> Result<Value, SiteError> {
    if !info.password_encryption {
        return Ok(json!({ "password": password }));
    }
    let response = http
        .send(http.get(&format!("{}/api/user/login/encryption-key", info.base_url)))
        .await?;
    let data = data_of(response)?;
    if !flag(&data, "enabled") {
        return Ok(json!({ "password": password }));
    }
    let key_id =
        text(&data, "kid").ok_or_else(|| SiteError::Unexpected("encryption key id".into()))?;
    let public_key = text(&data, "public_key")
        .ok_or_else(|| SiteError::Unexpected("encryption public key".into()))?;
    let encrypted =
        crypto::encrypt_password(&public_key, &key_id, password).map_err(SiteError::Unexpected)?;
    Ok(json!({ "password_encrypted": encrypted, "encryption_key_id": key_id }))
}

/// 账号密码登录。新版成功后接着创建个人访问令牌。
pub async fn login(
    http: &Http,
    info: &SiteInfo,
    account: &str,
    password: &str,
) -> Result<LoginStep, SiteError> {
    if info.password_login_disabled {
        return Err(SiteError::PasswordLoginDisabled);
    }
    if info.captcha_required {
        return Err(SiteError::CaptchaRequired);
    }
    let mut body = password_fields(http, info, password).await?;
    body["username"] = json!(account);
    let response = http
        .send(
            http.post(&format!("{}/api/user/login", info.base_url))
                .json(&body),
        )
        .await?;
    let cookie = cookie_header(&response.headers);
    let data = data_of(response).map_err(|error| match error {
        SiteError::Site(message) => SiteError::BadCredentials(message),
        other => other,
    })?;

    if flag(&data, "require_verification") {
        let can_use_code = data
            .get("methods")
            .and_then(Value::as_array)
            .is_none_or(|methods| {
                methods.iter().any(|method| {
                    text(method, "method").as_deref() == Some("2fa")
                        && method.get("available").and_then(Value::as_bool) != Some(false)
                })
            });
        if !can_use_code {
            return Err(SiteError::UnsupportedVerification);
        }
        let flow_token =
            text(&data, "flow_token").ok_or_else(|| SiteError::Unexpected("flow_token".into()))?;
        return Ok(LoginStep::NeedLoginCode {
            flow_token,
            legacy_cookie: None,
        });
    }
    if flag(&data, "require_2fa") {
        return Ok(LoginStep::NeedLoginCode {
            flow_token: String::new(),
            legacy_cookie: Some(cookie),
        });
    }
    if let Some(access_token) = text(&data, "access_token") {
        return create_token(http, info, &access_token, Some(password), None).await;
    }
    legacy_done(&data, cookie)
}

fn legacy_done(data: &Value, cookie: String) -> Result<LoginStep, SiteError> {
    let user_id = text(data, "id").ok_or_else(|| SiteError::Unexpected("user id".into()))?;
    if cookie.is_empty() {
        return Err(SiteError::Unexpected(
            "site returned no session cookie".into(),
        ));
    }
    let account = text(data, "username").unwrap_or_default();
    Ok(LoginStep::Done {
        credential: Credential::NewapiLegacy { cookie, user_id },
        account,
    })
}

/// 在登录窗口里登录后，用网页留下的刷新令牌（`new_api_refresh` cookie）换一个访问令牌。
/// 返回（访问令牌, 账号名）。刷新令牌用一次就换新，所以调用前要先关掉登录窗口。
pub async fn access_from_refresh(
    http: &Http,
    info: &SiteInfo,
    refresh: &str,
) -> Result<(String, String), SiteError> {
    let request = http
        .post(&format!("{}/api/user/auth/refresh", info.base_url))
        .header("Cookie", format!("{REFRESH_COOKIE}={refresh}"))
        // 站点会核对请求来源，和站点自己的网页一样带上它的地址。
        .header("Origin", &info.base_url);
    let data = data_of(http.send(request).await?).map_err(|error| match error {
        SiteError::Site(_) => SiteError::SessionExpired,
        other => other,
    })?;
    let access_token =
        text(&data, "access_token").ok_or_else(|| SiteError::Unexpected("access_token".into()))?;
    let account = data
        .get("user")
        .and_then(|user| text(user, "username").or_else(|| text(user, "email")))
        .unwrap_or_default();
    Ok((access_token, account))
}

/// 旧版在登录窗口里登录后：用会话 cookie 和用户 ID 组成凭证，顺便读出账号名。
pub async fn legacy_from_session(
    http: &Http,
    info: &SiteInfo,
    cookie: String,
    user_id: String,
) -> Result<LoginStep, SiteError> {
    let credential = Credential::NewapiLegacy { cookie, user_id };
    let (_, data) = self_info(http, &info.base_url, &credential).await?;
    Ok(LoginStep::Done {
        account: text(&data, "username").unwrap_or_default(),
        credential,
    })
}

/// 提交登录时的两步验证码。新版用 flow_token，旧版用登录 cookie。
pub async fn verify_login_code(
    http: &Http,
    info: &SiteInfo,
    flow_token: &str,
    legacy_cookie: Option<&str>,
    code: &str,
    password: &str,
) -> Result<LoginStep, SiteError> {
    let as_code_error = |error: SiteError| match error {
        SiteError::Site(message) => SiteError::InvalidCode(message),
        other => other,
    };
    if let Some(cookie) = legacy_cookie {
        let request = http
            .post(&format!("{}/api/user/login/2fa", info.base_url))
            .header("Cookie", cookie)
            .json(&json!({ "code": code }));
        let response = http.send(request).await?;
        let renewed = cookie_header(&response.headers);
        let data = data_of(response).map_err(as_code_error)?;
        return legacy_done(
            &data,
            if renewed.is_empty() {
                cookie.to_string()
            } else {
                renewed
            },
        );
    }
    let request = http
        .post(&format!("{}/api/user/login/verify", info.base_url))
        .json(&json!({ "flow_token": flow_token, "method": "2fa", "code": code }));
    let data = data_of(http.send(request).await?).map_err(as_code_error)?;
    let access_token =
        text(&data, "access_token").ok_or_else(|| SiteError::Unexpected("access_token".into()))?;
    create_token(http, info, &access_token, Some(password), None).await
}

/// 用登录得到的访问令牌创建个人访问令牌。站点要求两步验证码而 `code` 为空时，返回 NeedTokenCode。
pub async fn create_token(
    http: &Http,
    info: &SiteInfo,
    access_token: &str,
    password: Option<&str>,
    code: Option<&str>,
) -> Result<LoginStep, SiteError> {
    let base = &info.base_url;
    let methods_response = http
        .send(
            http.get(&format!(
                "{base}/api/verify/methods?scope={TOKEN_SCOPE_NAME}"
            ))
            .bearer_auth(access_token),
        )
        .await?;
    let methods = data_of(methods_response)?;
    let available = |name: &str| {
        methods
            .get("methods")
            .and_then(Value::as_array)
            .is_some_and(|list| {
                list.iter().any(|method| {
                    text(method, "method").as_deref() == Some(name)
                        && method.get("available").and_then(Value::as_bool) != Some(false)
                })
            })
    };

    let mut verify = json!({
        "scope": TOKEN_SCOPE_NAME,
        "context": { "scopes": TOKEN_SCOPES, "expires_at": 0 },
    });
    match (password, code) {
        (_, Some(code)) if available("2fa") => {
            verify["method"] = json!("2fa");
            verify["code"] = json!(code);
        }
        (Some(password), _) if available("password") => {
            verify["method"] = json!("password");
            let fields = password_fields(http, info, password).await?;
            for (key, value) in fields.as_object().into_iter().flatten() {
                verify[key] = value.clone();
            }
        }
        // 在登录窗口里登录的：我们没有密码，优先请用户再输一次密码，没有密码方式再要验证码。
        (None, None) if available("password") => {
            return Ok(LoginStep::NeedTokenPassword {
                access_token: access_token.to_string(),
            });
        }
        _ if available("2fa") => {
            return Ok(LoginStep::NeedTokenCode {
                access_token: access_token.to_string(),
            });
        }
        // 只能用 Passkey、第三方登录等方式验证。
        (None, None) => return Err(SiteError::UnsupportedVerification),
        _ => return Err(SiteError::TokenNotAllowed(None)),
    }

    let verify_response = http
        .send(
            http.post(&format!("{base}/api/verify"))
                .bearer_auth(access_token)
                .json(&verify),
        )
        .await?;
    let proof = data_of(verify_response).map_err(|error| match (error, code.is_some()) {
        (SiteError::Site(message), true) => SiteError::InvalidCode(message),
        (SiteError::Site(message), false) => SiteError::BadCredentials(message),
        (other, _) => other,
    })?;
    let proof_token =
        text(&proof, "proof_token").ok_or_else(|| SiteError::Unexpected("proof_token".into()))?;

    let name = format!(
        "TransitLink {}",
        crate::clock::date(crate::clock::now_secs())
    );
    let create = http
        .post(&format!("{base}/api/user/access_tokens"))
        .bearer_auth(access_token)
        .header("X-Security-Proof", proof_token)
        .json(&json!({ "name": name, "scopes": TOKEN_SCOPES, "expires_at": 0 }));
    let created = data_of(http.send(create).await?).map_err(|error| match error {
        SiteError::Site(message) => SiteError::TokenNotAllowed(message),
        other => other,
    })?;
    let token = text(&created, "token").ok_or_else(|| SiteError::Unexpected("token".into()))?;
    let account = self_info(
        http,
        base,
        &Credential::NewapiToken {
            token: token.clone(),
        },
    )
    .await
    .map(|user| user.0)
    .unwrap_or_default();
    Ok(LoginStep::Done {
        credential: Credential::NewapiToken { token },
        account,
    })
}

/// 令牌名称里带上日期，方便用户在站点上辨认（不依赖时间库）。
fn authorize(request: reqwest::RequestBuilder, credential: &Credential) -> reqwest::RequestBuilder {
    match credential {
        Credential::NewapiToken { token } => request.bearer_auth(token),
        Credential::NewapiLegacy { cookie, user_id } => request
            .header("Cookie", cookie)
            .header("New-Api-User", user_id),
        Credential::Sub2api { access_token, .. } => request.bearer_auth(access_token),
    }
}

/// 读取 /api/user/self，返回（显示名, 原始数据）。
async fn self_info(
    http: &Http,
    base: &str,
    credential: &Credential,
) -> Result<(String, Value), SiteError> {
    let data = data_of(
        http.send(authorize(
            http.get(&format!("{base}/api/user/self")),
            credential,
        ))
        .await?,
    )?;
    let name = text(&data, "display_name")
        .filter(|name| !name.trim().is_empty())
        .or_else(|| text(&data, "username"))
        .unwrap_or_default();
    Ok((name, data))
}

/// 账号概况：余额按 quota_per_unit 换算成美元。
pub async fn account(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<AccountSummary, SiteError> {
    let (name, data) = self_info(http, &info.base_url, credential).await?;
    let per_unit = info.quota_per_unit.unwrap_or(DEFAULT_QUOTA_PER_UNIT);
    Ok(AccountSummary {
        name,
        group: text(&data, "group").filter(|group| !group.is_empty()),
        balance_usd: number(&data, "quota").unwrap_or(0.0) / per_unit,
    })
}

/// Key 列表（新版是打码的，旧版是完整的）。兼容 `{items: [...]}` 分页结构和旧版直接返回数组。
pub async fn list_keys(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<Vec<KeySummary>, SiteError> {
    let request = authorize(
        http.get(&format!("{}/api/token/?p=1&page_size=100", info.base_url)),
        credential,
    );
    let data = data_of(http.send(request).await?)?;
    let items = data
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| data.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| {
            let key = text(item, "key").unwrap_or_default();
            Some(KeySummary {
                id: text(item, "id")?,
                name: text(item, "name").unwrap_or_default(),
                masked: key.contains('*'),
                // 站点存的 Key 不带 sk-，打码的也一样；统一加上前缀，和站点页面显示的一致。
                key: if key.is_empty() || key.starts_with("sk-") {
                    key
                } else {
                    format!("sk-{key}")
                },
                group: text(item, "group").filter(|group| !group.is_empty()),
                enabled: number(item, "status") == Some(1.0),
            })
        })
        .collect())
}

/// 取完整 Key（新版需要；每次调用都计入站点的限流，所以一次批量取）。
pub async fn reveal_keys(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
    ids: &[String],
) -> Result<Vec<(String, String)>, SiteError> {
    let numeric: Vec<i64> = ids.iter().filter_map(|id| id.parse().ok()).collect();
    let request = authorize(
        http.post(&format!("{}/api/token/batch/keys", info.base_url)),
        credential,
    )
    .json(&json!({ "ids": numeric }));
    let data = data_of(http.send(request).await?)?;
    let keys = data
        .get("keys")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(keys
        .into_iter()
        .filter_map(|(id, key)| {
            key.as_str().map(|key| {
                (
                    id,
                    if key.starts_with("sk-") {
                        key.to_string()
                    } else {
                        format!("sk-{key}")
                    },
                )
            })
        })
        .collect())
}

/// 用户可选的分组：`{名称: {ratio, desc}}`。
pub async fn groups(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<Vec<GroupSummary>, SiteError> {
    let request = authorize(
        http.get(&format!("{}/api/user/self/groups", info.base_url)),
        credential,
    );
    let data = data_of(http.send(request).await?)?;
    let mut groups: Vec<GroupSummary> = data
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, detail)| GroupSummary {
            id: name.clone(),
            name: name.clone(),
            description: text(detail, "desc").unwrap_or_default(),
            ratio: number(detail, "ratio"),
            platform: None,
        })
        .collect();
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(groups)
}

/// 新建一个不限额、不过期的 Key。站点不返回新 Key 的 ID，所以创建后按名称重新查一次。
pub async fn create_key(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
    name: &str,
    group: Option<&str>,
) -> Result<KeySummary, SiteError> {
    let body = json!({
        "name": name,
        "group": group.unwrap_or(""),
        "remain_quota": 0,
        "unlimited_quota": true,
        "expired_time": -1,
        "model_limits_enabled": false,
        "model_limits": "",
        "allow_ips": "",
    });
    data_of(
        http.send(
            authorize(
                http.post(&format!("{}/api/token/", info.base_url)),
                credential,
            )
            .json(&body),
        )
        .await?,
    )?;
    let created = list_keys(http, info, credential)
        .await?
        .into_iter()
        .filter(|key| key.name == name)
        .max_by_key(|key| key.id.parse::<i64>().unwrap_or(0))
        .ok_or_else(|| SiteError::Unexpected("created key not found".into()))?;
    unmask(http, info, credential, created).await
}

/// 取某个 Key 的完整内容。旧版列表里就是完整的；新版打了码，再单独取一次。
pub async fn full_key(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
    key_id: &str,
) -> Result<String, SiteError> {
    let key = list_keys(http, info, credential)
        .await?
        .into_iter()
        .find(|key| key.id == key_id)
        .ok_or_else(|| SiteError::Unexpected("key not found".into()))?;
    Ok(unmask(http, info, credential, key).await?.key)
}

pub(crate) async fn unmask(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
    key: KeySummary,
) -> Result<KeySummary, SiteError> {
    if !key.masked {
        return Ok(key);
    }
    let full = reveal_keys(http, info, credential, std::slice::from_ref(&key.id))
        .await?
        .into_iter()
        .find(|(id, _)| *id == key.id)
        .map(|(_, full)| full)
        .ok_or_else(|| SiteError::Unexpected("key reveal".into()))?;
    Ok(KeySummary {
        key: full,
        masked: false,
        ..key
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({ "success": true, "message": "", "data": data }))
    }

    async fn site(server: &MockServer, status: Value) -> SiteInfo {
        Mock::given(method("GET"))
            .and(path("/api/status"))
            .respond_with(ok(status))
            .mount(server)
            .await;
        fetch_site(&Http::new(), &server.uri())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn detects_new_api_and_its_login_requirements() {
        let server = MockServer::start().await;
        let info = site(&server, json!({ "system_name": "示例中转", "logo": "/logo.png", "quota_per_unit": 500000, "turnstile_check": false, "password_login_encryption_enabled": true })).await;
        assert_eq!(info.platform, Platform::Newapi);
        assert_eq!(info.name, "示例中转");
        assert_eq!(
            info.logo_url.as_deref(),
            Some(format!("{}/logo.png", server.uri()).as_str())
        );
        assert!(
            info.password_encryption && !info.captcha_required && !info.password_login_disabled
        );
    }

    #[tokio::test]
    async fn new_login_creates_a_personal_access_token() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "S", "quota_per_unit": 500000 }),
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/user/login"))
            .and(body_partial_json(
                json!({ "username": "alice", "password": "pw" }),
            ))
            .respond_with(ok(
                json!({ "access_token": "jwt-1", "token_type": "Bearer", "user": { "id": 7 } }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/verify/methods")).and(query_param("scope", "access_token.generate")).and(header("Authorization", "Bearer jwt-1"))
            .respond_with(ok(json!({ "methods": [{ "method": "password", "available": true }, { "method": "2fa", "available": false }] })))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/api/verify"))
            .and(body_partial_json(json!({ "method": "password", "password": "pw", "scope": "access_token.generate", "context": { "expires_at": 0 } })))
            .respond_with(ok(json!({ "proof_token": "proof-1" })))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/user/access_tokens"))
            .and(header("X-Security-Proof", "proof-1"))
            .respond_with(ok(json!({ "token": "nap_abc" })))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/user/self")).and(header("Authorization", "Bearer nap_abc"))
            .respond_with(ok(json!({ "id": 7, "username": "alice", "display_name": "", "quota": 1_000_000, "group": "default" })))
            .mount(&server).await;

        let step = login(&Http::new(), &info, "alice", "pw").await.unwrap();
        assert_eq!(
            step,
            LoginStep::Done {
                credential: Credential::NewapiToken {
                    token: "nap_abc".into()
                },
                account: "alice".into()
            }
        );

        let summary = account(
            &Http::new(),
            &info,
            &Credential::NewapiToken {
                token: "nap_abc".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.balance_usd, 2.0);
        assert_eq!(summary.group.as_deref(), Some("default"));
    }

    #[tokio::test]
    async fn web_login_refresh_then_password_creates_a_token() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "S", "quota_per_unit": 500000, "turnstile_check": true }),
        )
        .await;
        assert!(info.captcha_required);
        Mock::given(method("POST"))
            .and(path("/api/user/auth/refresh"))
            .and(header("Cookie", "new_api_refresh=sid.secret"))
            .and(header("Origin", server.uri().as_str()))
            .respond_with(ok(
                json!({ "access_token": "jwt-web", "user": { "id": 7, "username": "alice" } }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/verify/methods")).and(header("Authorization", "Bearer jwt-web"))
            .respond_with(ok(json!({ "methods": [{ "method": "password", "available": true }, { "method": "2fa", "available": true }] })))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/verify"))
            .and(body_partial_json(
                json!({ "method": "password", "password": "pw" }),
            ))
            .respond_with(ok(json!({ "proof_token": "proof-web" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/user/access_tokens"))
            .and(header("X-Security-Proof", "proof-web"))
            .respond_with(ok(json!({ "token": "nap_web" })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/user/self"))
            .and(header("Authorization", "Bearer nap_web"))
            .respond_with(ok(json!({ "id": 7, "username": "alice", "quota": 0 })))
            .mount(&server)
            .await;

        let http = Http::new();
        let (access_token, account) = access_from_refresh(&http, &info, "sid.secret")
            .await
            .unwrap();
        assert_eq!(
            (access_token.as_str(), account.as_str()),
            ("jwt-web", "alice")
        );
        // 有密码方式时先要密码（不用等验证器刷新）。
        assert_eq!(
            create_token(&http, &info, &access_token, None, None)
                .await
                .unwrap(),
            LoginStep::NeedTokenPassword {
                access_token: "jwt-web".into()
            }
        );
        assert_eq!(
            create_token(&http, &info, &access_token, Some("pw"), None)
                .await
                .unwrap(),
            LoginStep::Done {
                credential: Credential::NewapiToken {
                    token: "nap_web".into()
                },
                account: "alice".into()
            }
        );

        // 只能用 Passkey 验证的账号：明确告诉用户不支持。
        Mock::given(method("GET"))
            .and(path("/api/verify/methods"))
            .and(header("Authorization", "Bearer jwt-passkey"))
            .respond_with(ok(
                json!({ "methods": [{ "method": "passkey", "available": true }] }),
            ))
            .mount(&server)
            .await;
        assert_eq!(
            create_token(&http, &info, "jwt-passkey", None, None)
                .await
                .unwrap_err(),
            SiteError::UnsupportedVerification
        );
        // 刷新令牌失效（站点回 401）：需要重新登录。
        Mock::given(method("POST"))
            .and(path("/api/user/auth/refresh"))
            .and(header("Cookie", "new_api_refresh=stale"))
            .respond_with(ResponseTemplate::new(401).set_body_json(
                json!({ "success": false, "code": "AUTH_REFRESH_INVALID", "message": "Unauthorized" }),
            ))
            .mount(&server)
            .await;
        assert_eq!(
            access_from_refresh(&http, &info, "stale")
                .await
                .unwrap_err(),
            SiteError::SessionExpired
        );
    }

    #[tokio::test]
    async fn two_factor_login_and_token_code_are_requested_when_needed() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "S", "quota_per_unit": 500000 }),
        )
        .await;
        Mock::given(method("POST")).and(path("/api/user/login"))
            .respond_with(ok(json!({ "require_verification": true, "flow_token": "flow-1", "methods": [{ "method": "2fa", "available": true }] })))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/user/login/verify"))
            .and(body_partial_json(
                json!({ "flow_token": "flow-1", "code": "123456" }),
            ))
            .respond_with(ok(json!({ "access_token": "jwt-2" })))
            .mount(&server)
            .await;
        // 有两步验证的账号：创建令牌只允许用验证码。
        Mock::given(method("GET")).and(path("/api/verify/methods"))
            .respond_with(ok(json!({ "methods": [{ "method": "password", "available": false }, { "method": "2fa", "available": true }] })))
            .mount(&server).await;

        let http = Http::new();
        let step = login(&http, &info, "alice", "pw").await.unwrap();
        assert_eq!(
            step,
            LoginStep::NeedLoginCode {
                flow_token: "flow-1".into(),
                legacy_cookie: None
            }
        );
        let step = verify_login_code(&http, &info, "flow-1", None, "123456", "pw")
            .await
            .unwrap();
        assert_eq!(
            step,
            LoginStep::NeedTokenCode {
                access_token: "jwt-2".into()
            },
            "a fresh code is needed for the access token"
        );
    }

    #[tokio::test]
    async fn legacy_login_keeps_the_session_cookie() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "Old", "quota_per_unit": 500000 }),
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/user/login"))
            .respond_with(
                ok(json!({ "id": 42, "username": "bob" }))
                    .insert_header("Set-Cookie", "session=s1; Path=/; HttpOnly"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/token/"))
            .and(header("New-Api-User", "42"))
            .and(header("Cookie", "session=s1"))
            .respond_with(ok(
                json!([{ "id": 3, "name": "main", "key": "abc123", "group": "vip", "status": 1 }]),
            ))
            .mount(&server)
            .await;

        let http = Http::new();
        let step = login(&http, &info, "bob", "pw").await.unwrap();
        let credential = Credential::NewapiLegacy {
            cookie: "session=s1".into(),
            user_id: "42".into(),
        };
        assert_eq!(
            step,
            LoginStep::Done {
                credential: credential.clone(),
                account: "bob".into()
            }
        );

        let keys = list_keys(&http, &info, &credential).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0].key, "sk-abc123",
            "legacy lists return full keys without the sk- prefix"
        );
        assert!(!keys[0].masked && keys[0].enabled);
    }

    #[tokio::test]
    async fn failures_map_to_friendly_errors() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "S", "quota_per_unit": 500000 }),
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/user/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "success": false, "message": "用户名或密码错误" })),
            )
            .mount(&server)
            .await;
        let error = login(&Http::new(), &info, "alice", "bad")
            .await
            .unwrap_err();
        assert_eq!(
            error,
            SiteError::BadCredentials(Some("用户名或密码错误".into()))
        );

        let captcha = SiteInfo {
            captcha_required: true,
            ..info.clone()
        };
        assert_eq!(
            login(&Http::new(), &captcha, "a", "b").await.unwrap_err(),
            SiteError::CaptchaRequired
        );
    }

    #[tokio::test]
    async fn masked_keys_are_revealed_in_one_batch() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "S", "quota_per_unit": 500000 }),
        )
        .await;
        let credential = Credential::NewapiToken {
            token: "nap_abc".into(),
        };
        Mock::given(method("POST"))
            .and(path("/api/token/"))
            .and(body_partial_json(
                json!({ "name": "TransitLink Codex", "group": "codex", "unlimited_quota": true }),
            ))
            .respond_with(ok(Value::Null))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/token/"))
            .respond_with(ok(json!({ "page": 1, "page_size": 100, "total": 2, "items": [
                { "id": 9, "name": "TransitLink Codex", "key": "abcd**********wxyz", "group": "codex", "status": 1 },
                { "id": 5, "name": "other", "key": "efgh**********stuv", "group": "", "status": 2 },
            ] })))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/token/batch/keys"))
            .and(body_partial_json(json!({ "ids": [9] })))
            .respond_with(ok(json!({ "keys": { "9": "abcdFULLwxyz" } })))
            .mount(&server)
            .await;

        let key = create_key(
            &Http::new(),
            &info,
            &credential,
            "TransitLink Codex",
            Some("codex"),
        )
        .await
        .unwrap();
        assert_eq!(key.key, "sk-abcdFULLwxyz");
        assert!(!key.masked);
        assert_eq!(key.group.as_deref(), Some("codex"));

        let listed = list_keys(&Http::new(), &info, &credential).await.unwrap();
        assert_eq!(listed[0].key, "sk-abcd**********wxyz");
        assert!(listed[0].masked);
        assert_eq!(
            full_key(&Http::new(), &info, &credential, "9")
                .await
                .unwrap(),
            "sk-abcdFULLwxyz"
        );
    }

    #[tokio::test]
    async fn legacy_lists_full_keys_without_a_reveal_call() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "system_name": "S", "quota_per_unit": 500000 }),
        )
        .await;
        let credential = Credential::NewapiLegacy {
            cookie: "session=abc".into(),
            user_id: "7".into(),
        };
        Mock::given(method("GET"))
            .and(path("/api/token/"))
            .and(header("New-Api-User", "7"))
            .respond_with(ok(
                json!([{ "id": 3, "name": "cc", "key": "plainkey1234", "status": 1 }]),
            ))
            .mount(&server)
            .await;
        // 旧版没有批量取 Key 的接口，调用了就会失败。
        Mock::given(method("POST"))
            .and(path("/api/token/batch/keys"))
            .respond_with(ResponseTemplate::new(404))
            .expect(0)
            .mount(&server)
            .await;

        let key = full_key(&Http::new(), &info, &credential, "3")
            .await
            .unwrap();
        assert_eq!(key, "sk-plainkey1234");
        assert_eq!(
            full_key(&Http::new(), &info, &credential, "404")
                .await
                .unwrap_err()
                .code(),
            "unexpected"
        );
    }
}
