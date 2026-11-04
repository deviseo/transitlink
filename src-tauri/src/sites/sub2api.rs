//! sub2api 的用户侧接口。响应统一是 `{code: 0, message, data}`。
//! 登录得到访问令牌（默认 24 小时）和刷新令牌；刷新令牌用一次就作废，每次刷新都要保存新的一对。

use serde_json::{Value, json};

use super::http::{Http, JsonResponse, flag, number, text};
use super::{
    AccountSummary, Credential, GroupSummary, KeySummary, LoginStep, Platform, SiteError, SiteInfo,
    absolute_url,
};

/// 读取公开设置识别 sub2api。不是 sub2api 时返回 None。
pub async fn fetch_site(http: &Http, base_url: &str) -> Result<Option<SiteInfo>, SiteError> {
    let response = http
        .send(http.get(&format!("{base_url}/api/v1/settings/public")))
        .await?;
    let Some(body) = response.body else {
        return Ok(None);
    };
    let data = match body.get("data") {
        Some(data)
            if number(&body, "code") == Some(0.0)
                && data.is_object()
                && (data.get("registration_enabled").is_some()
                    || data.get("site_name").is_some()) =>
        {
            data
        }
        _ => return Ok(None),
    };
    let name = text(data, "site_name")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Sub2API".to_string());
    Ok(Some(SiteInfo {
        platform: Platform::Sub2api,
        base_url: base_url.to_string(),
        name,
        logo_url: absolute_url(base_url, text(data, "site_logo").as_deref()),
        captcha_required: flag(data, "turnstile_enabled")
            || flag(data, "tencent_captcha_enabled")
            || flag(data, "aliyun_captcha_enabled"),
        // backend 模式下只允许管理员登录。
        password_login_disabled: flag(data, "backend_mode_enabled"),
        email_only: true,
        quota_per_unit: None,
        password_encryption: false,
        user_agent: None,
    }))
}

fn data_of(response: JsonResponse) -> Result<Value, SiteError> {
    if response.status == 401 {
        return Err(SiteError::SessionExpired);
    }
    let body = response
        .body
        .ok_or_else(|| SiteError::Unexpected(format!("HTTP {}", response.status)))?;
    if number(&body, "code") != Some(0.0) {
        return Err(SiteError::Site(
            text(&body, "message").filter(|message| !message.is_empty()),
        ));
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

fn tokens_from(data: &Value) -> Result<Credential, SiteError> {
    Ok(Credential::Sub2api {
        access_token: text(data, "access_token")
            .ok_or_else(|| SiteError::Unexpected("access_token".into()))?,
        refresh_token: text(data, "refresh_token").unwrap_or_default(),
    })
}

fn account_name(data: &Value) -> String {
    data.get("user")
        .and_then(|user| text(user, "email").or_else(|| text(user, "username")))
        .unwrap_or_default()
}

/// 邮箱密码登录。开启了两步验证时返回 NeedLoginCode（flow_token 是 temp_token）。
pub async fn login(
    http: &Http,
    info: &SiteInfo,
    email: &str,
    password: &str,
) -> Result<LoginStep, SiteError> {
    if info.password_login_disabled {
        return Err(SiteError::PasswordLoginDisabled);
    }
    if info.captcha_required {
        return Err(SiteError::CaptchaRequired);
    }
    let request = http
        .post(&format!("{}/api/v1/auth/login", info.base_url))
        .json(&json!({ "email": email, "password": password }));
    let data = data_of(http.send(request).await?).map_err(|error| match error {
        SiteError::Site(message) | SiteError::BadCredentials(message) => {
            SiteError::BadCredentials(message)
        }
        SiteError::SessionExpired => SiteError::BadCredentials(None),
        other => other,
    })?;
    if flag(&data, "requires_2fa") {
        let temp_token =
            text(&data, "temp_token").ok_or_else(|| SiteError::Unexpected("temp_token".into()))?;
        return Ok(LoginStep::NeedLoginCode {
            flow_token: temp_token,
            legacy_cookie: None,
        });
    }
    Ok(LoginStep::Done {
        credential: tokens_from(&data)?,
        account: account_name(&data),
    })
}

/// 提交两步验证码。
pub async fn verify_login_code(
    http: &Http,
    info: &SiteInfo,
    temp_token: &str,
    code: &str,
) -> Result<LoginStep, SiteError> {
    let request = http
        .post(&format!("{}/api/v1/auth/login/2fa", info.base_url))
        .json(&json!({ "temp_token": temp_token, "totp_code": code }));
    let data = data_of(http.send(request).await?).map_err(|error| match error {
        SiteError::Site(message) => SiteError::InvalidCode(message),
        SiteError::SessionExpired => SiteError::InvalidCode(None),
        other => other,
    })?;
    Ok(LoginStep::Done {
        credential: tokens_from(&data)?,
        account: account_name(&data),
    })
}

/// 用刷新令牌换一对新令牌。刷新令牌失效时返回 SessionExpired（需要重新登录）。
pub async fn refresh(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<Credential, SiteError> {
    let Credential::Sub2api { refresh_token, .. } = credential else {
        return Err(SiteError::SessionExpired);
    };
    if refresh_token.is_empty() {
        return Err(SiteError::SessionExpired);
    }
    let request = http
        .post(&format!("{}/api/v1/auth/refresh", info.base_url))
        .json(&json!({ "refresh_token": refresh_token }));
    let data = data_of(http.send(request).await?).map_err(|error| match error {
        SiteError::Site(_) => SiteError::SessionExpired,
        other => other,
    })?;
    tokens_from(&data)
}

fn bearer(request: reqwest::RequestBuilder, credential: &Credential) -> reqwest::RequestBuilder {
    match credential {
        Credential::Sub2api { access_token, .. } => request.bearer_auth(access_token),
        _ => request,
    }
}

/// 账号概况，余额本身就是美元。
pub async fn account(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<AccountSummary, SiteError> {
    let data = data_of(
        http.send(bearer(
            http.get(&format!("{}/api/v1/auth/me", info.base_url)),
            credential,
        ))
        .await?,
    )?;
    Ok(AccountSummary {
        name: text(&data, "email")
            .or_else(|| text(&data, "username"))
            .unwrap_or_default(),
        group: None,
        balance_usd: number(&data, "balance").unwrap_or(0.0),
    })
}

/// Key 列表（sub2api 返回完整 Key）。
pub async fn list_keys(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<Vec<KeySummary>, SiteError> {
    let request = bearer(
        http.get(&format!(
            "{}/api/v1/keys?page=1&page_size=100",
            info.base_url
        )),
        credential,
    );
    let data = data_of(http.send(request).await?)?;
    let items = data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let groups = groups(http, info, credential).await.unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| key_summary(item, &groups))
        .collect())
}

/// 取某个 Key 的完整内容（sub2api 的列表本来就是完整的）。
pub async fn full_key(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
    key_id: &str,
) -> Result<String, SiteError> {
    list_keys(http, info, credential)
        .await?
        .into_iter()
        .find(|key| key.id == key_id)
        .map(|key| key.key)
        .ok_or_else(|| SiteError::Unexpected("key not found".into()))
}

fn key_summary(item: &Value, groups: &[GroupSummary]) -> Option<KeySummary> {
    let group_id = text(item, "group_id");
    let group = group_id.as_ref().map(|id| {
        groups
            .iter()
            .find(|group| &group.id == id)
            .map(|group| group.name.clone())
            .unwrap_or_else(|| id.clone())
    });
    Some(KeySummary {
        id: text(item, "id")?,
        name: text(item, "name").unwrap_or_default(),
        key: text(item, "key").unwrap_or_default(),
        masked: false,
        group,
        enabled: text(item, "status").as_deref() == Some("active"),
    })
}

/// 可用分组（带平台类型和倍率）。
pub async fn groups(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
) -> Result<Vec<GroupSummary>, SiteError> {
    let data = data_of(
        http.send(bearer(
            http.get(&format!("{}/api/v1/groups/available", info.base_url)),
            credential,
        ))
        .await?,
    )?;
    let list = data
        .as_array()
        .or_else(|| data.get("items").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    Ok(list
        .iter()
        .filter_map(|group| {
            Some(GroupSummary {
                id: text(group, "id")?,
                name: text(group, "name").unwrap_or_default(),
                description: text(group, "description").unwrap_or_default(),
                ratio: number(group, "rate_multiplier"),
                platform: text(group, "platform"),
            })
        })
        .collect())
}

/// 在指定分组下新建一个 Key。
pub async fn create_key(
    http: &Http,
    info: &SiteInfo,
    credential: &Credential,
    name: &str,
    group_id: Option<&str>,
) -> Result<KeySummary, SiteError> {
    let mut body = json!({ "name": name });
    if let Some(id) = group_id.and_then(|id| id.parse::<i64>().ok()) {
        body["group_id"] = json!(id);
    }
    let data = data_of(
        http.send(
            bearer(
                http.post(&format!("{}/api/v1/keys", info.base_url)),
                credential,
            )
            .json(&body),
        )
        .await?,
    )?;
    let groups = groups(http, info, credential).await.unwrap_or_default();
    key_summary(&data, &groups).ok_or_else(|| SiteError::Unexpected("created key".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({ "code": 0, "message": "success", "data": data }))
    }

    async fn site(server: &MockServer, settings: Value) -> SiteInfo {
        Mock::given(method("GET"))
            .and(path("/api/v1/settings/public"))
            .respond_with(ok(settings))
            .mount(server)
            .await;
        fetch_site(&Http::new(), &server.uri())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn detects_sub2api_and_captcha() {
        let server = MockServer::start().await;
        let info = site(&server, json!({ "site_name": "Relay", "site_logo": "https://cdn.example.com/l.png", "registration_enabled": true, "turnstile_enabled": true })).await;
        assert_eq!(info.platform, Platform::Sub2api);
        assert_eq!(info.name, "Relay");
        assert!(info.captcha_required && info.email_only);
        assert_eq!(
            login(&Http::new(), &info, "a@example.com", "pw")
                .await
                .unwrap_err(),
            SiteError::CaptchaRequired
        );
    }

    #[tokio::test]
    async fn login_with_two_factor_then_refresh() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "site_name": "Relay", "registration_enabled": true }),
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/login"))
            .and(body_partial_json(
                json!({ "email": "a@example.com", "password": "pw" }),
            ))
            .respond_with(ok(json!({ "requires_2fa": true, "temp_token": "tmp-1" })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/api/v1/auth/login/2fa")).and(body_partial_json(json!({ "temp_token": "tmp-1", "totp_code": "654321" })))
            .respond_with(ok(json!({ "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 86400, "user": { "email": "a@example.com" } })))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/refresh"))
            .and(body_partial_json(json!({ "refresh_token": "rt-1" })))
            .respond_with(ok(
                json!({ "access_token": "at-2", "refresh_token": "rt-2", "expires_in": 86400 }),
            ))
            .mount(&server)
            .await;

        let http = Http::new();
        let step = login(&http, &info, "a@example.com", "pw").await.unwrap();
        assert_eq!(
            step,
            LoginStep::NeedLoginCode {
                flow_token: "tmp-1".into(),
                legacy_cookie: None
            }
        );
        let step = verify_login_code(&http, &info, "tmp-1", "654321")
            .await
            .unwrap();
        let first = Credential::Sub2api {
            access_token: "at-1".into(),
            refresh_token: "rt-1".into(),
        };
        assert_eq!(
            step,
            LoginStep::Done {
                credential: first.clone(),
                account: "a@example.com".into()
            }
        );
        assert_eq!(
            refresh(&http, &info, &first).await.unwrap(),
            Credential::Sub2api {
                access_token: "at-2".into(),
                refresh_token: "rt-2".into()
            }
        );
    }

    #[tokio::test]
    async fn reads_account_keys_and_groups() {
        let server = MockServer::start().await;
        let info = site(
            &server,
            json!({ "site_name": "Relay", "registration_enabled": true }),
        )
        .await;
        let credential = Credential::Sub2api {
            access_token: "at-1".into(),
            refresh_token: "rt-1".into(),
        };
        Mock::given(method("GET"))
            .and(path("/api/v1/auth/me"))
            .and(header("Authorization", "Bearer at-1"))
            .respond_with(ok(json!({ "email": "a@example.com", "balance": 12.5 })))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/v1/groups/available"))
            .respond_with(ok(json!([{ "id": 2, "name": "Claude Max", "platform": "anthropic", "rate_multiplier": 1.2 }]))).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v1/keys"))
            .respond_with(ok(json!({ "items": [{ "id": 11, "name": "cc", "key": "sk-full", "group_id": 2, "status": "active" }], "total": 1 }))).mount(&server).await;

        let http = Http::new();
        let summary = account(&http, &info, &credential).await.unwrap();
        assert_eq!(summary.balance_usd, 12.5);
        let keys = list_keys(&http, &info, &credential).await.unwrap();
        assert_eq!(keys[0].key, "sk-full");
        assert_eq!(keys[0].group.as_deref(), Some("Claude Max"));
        assert!(keys[0].enabled);
        assert_eq!(
            full_key(&http, &info, &credential, "11").await.unwrap(),
            "sk-full"
        );
        let groups = groups(&http, &info, &credential).await.unwrap();
        assert_eq!(groups[0].platform.as_deref(), Some("anthropic"));

        Mock::given(method("GET"))
            .and(path("/api/v1/auth/me"))
            .and(header("Authorization", "Bearer expired"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({ "code": 401, "message": "TOKEN_EXPIRED" })),
            )
            .mount(&server)
            .await;
        let expired = Credential::Sub2api {
            access_token: "expired".into(),
            refresh_token: "rt".into(),
        };
        assert_eq!(
            account(&http, &info, &expired).await.unwrap_err(),
            SiteError::SessionExpired
        );
    }
}
