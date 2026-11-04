//! 站点账号：识别、登录（含两步验证的多步流程）、保存，以及用保存的凭证读取账号和 Key。
//! sub2api 的访问令牌过期时自动用刷新令牌续期并保存新的一对。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::RngCore;
use serde::Serialize;

use crate::secrets::SecretStore;
use crate::sites::http::Http;
use crate::sites::{
    self, AccountSummary, Credential, GroupSummary, KeySummary, LoginStep, Platform, SiteError,
    SiteInfo, newapi, sub2api,
};
use crate::store::{SiteRecord, SiteStore};

/// 等待验证码的登录流程保留 10 分钟。
const PENDING_TTL: Duration = Duration::from_secs(600);

type SiteFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SiteError>> + Send + 'a>>;

/// 给前端的站点信息（不含凭证）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteView {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub logo_url: Option<String>,
    pub platform: Platform,
    pub account: String,
    pub added_at: u64,
}

impl From<&SiteRecord> for SiteView {
    fn from(record: &SiteRecord) -> Self {
        SiteView {
            id: record.id.clone(),
            name: record.info.name.clone(),
            base_url: record.info.base_url.clone(),
            logo_url: record.info.logo_url.clone(),
            platform: record.info.platform,
            account: record.account.clone(),
            added_at: record.added_at,
        }
    }
}

/// 登录结果：完成；或者需要验证码（login：登录时的两步验证；token：创建个人访问令牌时再验证一次）；
/// 或者需要再输一次密码（在登录窗口里登录的 new-api 站点，创建个人访问令牌时要用）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LoginOutcome {
    Done {
        site: SiteView,
    },
    #[serde(rename_all = "camelCase")]
    NeedCode {
        flow_id: String,
        stage: CodeStage,
    },
    #[serde(rename_all = "camelCase")]
    NeedPassword {
        flow_id: String,
    },
}

/// 在登录窗口里登录后取到的登录状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSession {
    /// 新版 new-api：刷新令牌 cookie 的值。
    NewapiRefresh(String),
    /// 旧版 new-api：会话 cookie（`session=…`）和用户 ID。
    NewapiLegacy { cookie: String, user_id: String },
    /// sub2api：网页存的令牌和账号，以及登录窗口的 User-Agent。
    Sub2api {
        access_token: String,
        refresh_token: String,
        account: String,
        user_agent: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CodeStage {
    Login,
    Token,
}

enum Pending {
    LoginCode {
        info: SiteInfo,
        account: String,
        password: String,
        flow_token: String,
        legacy_cookie: Option<String>,
    },
    TokenCode {
        info: SiteInfo,
        account: String,
        access_token: String,
    },
    TokenPassword {
        info: SiteInfo,
        account: String,
        access_token: String,
    },
}

pub struct Accounts {
    http: Http,
    store: SiteStore,
    secrets: Box<dyn SecretStore>,
    pending: Mutex<HashMap<String, (Instant, Pending)>>,
    /// sub2api 的刷新令牌只能用一次：同一时间只做一次续期，避免并发请求把会话弄失效。
    refresh_lock: tauri::async_runtime::Mutex<()>,
}

fn new_id() -> String {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn storage(error: String) -> SiteError {
    SiteError::Unexpected(format!("storage: {error}"))
}

impl Accounts {
    pub fn new(http: Http, store: SiteStore, secrets: Box<dyn SecretStore>) -> Self {
        Self {
            http,
            store,
            secrets,
            pending: Mutex::new(HashMap::new()),
            refresh_lock: tauri::async_runtime::Mutex::new(()),
        }
    }

    pub async fn detect(&self, url: &str) -> Result<SiteInfo, SiteError> {
        sites::detect(&self.http, url).await
    }

    /// 账号密码登录；成功后保存站点和凭证，需要验证码时返回 NeedCode。
    pub async fn login(
        &self,
        url: &str,
        account: &str,
        password: &str,
    ) -> Result<LoginOutcome, SiteError> {
        let info = self.detect(url).await?;
        let account = account.trim();
        let step = match info.platform {
            Platform::Newapi => newapi::login(&self.http, &info, account, password).await?,
            Platform::Sub2api => sub2api::login(&self.http, &info, account, password).await?,
        };
        self.advance(info, account, Some(password), step)
    }

    /// 提交验证码，继续之前的登录流程。
    pub async fn submit_code(&self, flow_id: &str, code: &str) -> Result<LoginOutcome, SiteError> {
        let pending = self
            .take_pending(flow_id)
            .ok_or(SiteError::SessionExpired)?;
        let code = code.trim();
        match pending {
            pending @ Pending::TokenPassword { .. } => {
                self.put_pending(flow_id, pending);
                Err(SiteError::SessionExpired)
            }
            Pending::LoginCode {
                info,
                account,
                password,
                flow_token,
                legacy_cookie,
            } => {
                let result = match info.platform {
                    Platform::Newapi => {
                        newapi::verify_login_code(
                            &self.http,
                            &info,
                            &flow_token,
                            legacy_cookie.as_deref(),
                            code,
                            &password,
                        )
                        .await
                    }
                    Platform::Sub2api => {
                        sub2api::verify_login_code(&self.http, &info, &flow_token, code).await
                    }
                };
                match result {
                    Ok(step) => self.advance(info, &account, Some(&password), step),
                    Err(error @ SiteError::InvalidCode(_)) => {
                        // 验证码错了可以再输一次。
                        self.put_pending(
                            flow_id,
                            Pending::LoginCode {
                                info,
                                account,
                                password,
                                flow_token,
                                legacy_cookie,
                            },
                        );
                        Err(error)
                    }
                    Err(error) => Err(error),
                }
            }
            Pending::TokenCode {
                info,
                account,
                access_token,
            } => {
                match newapi::create_token(&self.http, &info, &access_token, None, Some(code)).await
                {
                    Ok(step) => self.advance(info, &account, None, step),
                    Err(error @ SiteError::InvalidCode(_)) => {
                        self.put_pending(
                            flow_id,
                            Pending::TokenCode {
                                info,
                                account,
                                access_token,
                            },
                        );
                        Err(error)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }

    /// 在登录窗口里登录完成：把取到的登录状态变成凭证。new-api 新版还要创建个人访问令牌，
    /// 一般需要再输一次密码（返回 NeedPassword）。
    pub async fn finish_web_login(
        &self,
        mut info: SiteInfo,
        session: WebSession,
    ) -> Result<LoginOutcome, SiteError> {
        match session {
            WebSession::NewapiRefresh(refresh) => {
                let (access_token, account) =
                    newapi::access_from_refresh(&self.http, &info, &refresh).await?;
                let step =
                    newapi::create_token(&self.http, &info, &access_token, None, None).await?;
                self.advance(info, &account, None, step)
            }
            WebSession::NewapiLegacy { cookie, user_id } => {
                let step = newapi::legacy_from_session(&self.http, &info, cookie, user_id).await?;
                self.advance(info, "", None, step)
            }
            WebSession::Sub2api {
                access_token,
                refresh_token,
                account,
                user_agent,
            } => {
                info.user_agent = Some(user_agent);
                let step = LoginStep::Done {
                    credential: Credential::Sub2api {
                        access_token,
                        refresh_token,
                    },
                    account: account.clone(),
                };
                self.advance(info, &account, None, step)
            }
        }
    }

    /// 提交密码（创建个人访问令牌时的验证）。密码错了可以再试。
    pub async fn submit_password(
        &self,
        flow_id: &str,
        password: &str,
    ) -> Result<LoginOutcome, SiteError> {
        let pending = self
            .take_pending(flow_id)
            .ok_or(SiteError::SessionExpired)?;
        let Pending::TokenPassword {
            info,
            account,
            access_token,
        } = pending
        else {
            self.put_pending(flow_id, pending);
            return Err(SiteError::SessionExpired);
        };
        match newapi::create_token(&self.http, &info, &access_token, Some(password), None).await {
            Ok(step) => self.advance(info, &account, Some(password), step),
            Err(error @ SiteError::BadCredentials(_)) => {
                self.put_pending(
                    flow_id,
                    Pending::TokenPassword {
                        info,
                        account,
                        access_token,
                    },
                );
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub fn cancel(&self, flow_id: &str) {
        self.take_pending(flow_id);
    }

    fn advance(
        &self,
        info: SiteInfo,
        account: &str,
        password: Option<&str>,
        step: LoginStep,
    ) -> Result<LoginOutcome, SiteError> {
        let flow_id = new_id();
        match step {
            LoginStep::Done {
                credential,
                account: reported,
            } => {
                let display = if account.is_empty() {
                    reported
                } else {
                    account.to_string()
                };
                self.save(info, &display, &credential)
                    .map(|site| LoginOutcome::Done { site })
            }
            LoginStep::NeedLoginCode {
                flow_token,
                legacy_cookie,
            } => {
                let password = password.unwrap_or_default().to_string();
                self.put_pending(
                    &flow_id,
                    Pending::LoginCode {
                        info,
                        account: account.to_string(),
                        password,
                        flow_token,
                        legacy_cookie,
                    },
                );
                Ok(LoginOutcome::NeedCode {
                    flow_id,
                    stage: CodeStage::Login,
                })
            }
            LoginStep::NeedTokenPassword { access_token } => {
                self.put_pending(
                    &flow_id,
                    Pending::TokenPassword {
                        info,
                        account: account.to_string(),
                        access_token,
                    },
                );
                Ok(LoginOutcome::NeedPassword { flow_id })
            }
            LoginStep::NeedTokenCode { access_token } => {
                self.put_pending(
                    &flow_id,
                    Pending::TokenCode {
                        info,
                        account: account.to_string(),
                        access_token,
                    },
                );
                Ok(LoginOutcome::NeedCode {
                    flow_id,
                    stage: CodeStage::Token,
                })
            }
        }
    }

    fn put_pending(&self, flow_id: &str, pending: Pending) {
        if let Ok(mut map) = self.pending.lock() {
            map.retain(|_, (created, _)| created.elapsed() < PENDING_TTL);
            map.insert(flow_id.to_string(), (Instant::now(), pending));
        }
    }

    fn take_pending(&self, flow_id: &str) -> Option<Pending> {
        let mut map = self.pending.lock().ok()?;
        map.retain(|_, (created, _)| created.elapsed() < PENDING_TTL);
        map.remove(flow_id).map(|(_, pending)| pending)
    }

    /// 保存站点记录和凭证。同一站点同一账号重新登录时沿用原来的记录。
    pub(crate) fn save(
        &self,
        info: SiteInfo,
        account: &str,
        credential: &Credential,
    ) -> Result<SiteView, SiteError> {
        let existing = self.store.find(&info.base_url, account).map_err(storage)?;
        let record = SiteRecord {
            id: existing
                .as_ref()
                .map(|site| site.id.clone())
                .unwrap_or_else(new_id),
            info,
            account: account.to_string(),
            added_at: existing
                .map(|site| site.added_at)
                .unwrap_or_else(crate::clock::now_secs),
        };
        self.store_credential(&record.id, credential)?;
        self.store.upsert(record.clone()).map_err(storage)?;
        Ok(SiteView::from(&record))
    }

    fn store_credential(&self, id: &str, credential: &Credential) -> Result<(), SiteError> {
        let json = serde_json::to_string(credential).map_err(|error| storage(error.to_string()))?;
        self.secrets.set(id, &json).map_err(storage)
    }

    fn load(&self, id: &str) -> Result<(SiteRecord, Credential), SiteError> {
        let record = self
            .store
            .get(id)
            .map_err(storage)?
            .ok_or(SiteError::SessionExpired)?;
        let json = self
            .secrets
            .get(id)
            .map_err(storage)?
            .ok_or(SiteError::SessionExpired)?;
        let credential = serde_json::from_str(&json).map_err(|_| SiteError::SessionExpired)?;
        Ok((record, credential))
    }

    /// 站点记录（不含凭证）。
    pub fn site(&self, id: &str) -> Result<SiteRecord, SiteError> {
        self.store
            .get(id)
            .map_err(storage)?
            .ok_or(SiteError::SessionExpired)
    }

    /// 给工具用的专用 Key：这个分组里已经有同名、启用中的 Key 就直接用，否则新建一个。
    /// 返回完整 Key（只在 Rust 侧使用，写进工具配置）和是否新建。
    pub async fn tool_key(
        &self,
        id: &str,
        name: &str,
        group_id: Option<&str>,
    ) -> Result<(KeySummary, bool), SiteError> {
        let name = name.to_string();
        let group_id = group_id.map(str::to_string);
        self.call(id, |http, info, credential| {
            let name = name.clone();
            let group_id = group_id.clone();
            Box::pin(async move {
                match info.platform {
                    Platform::Newapi => {
                        // new-api 的 Key 直接记分组名；空分组就是用户自己的分组。
                        let wanted = group_id.clone().unwrap_or_default();
                        let existing = newapi::list_keys(http, info, credential)
                            .await?
                            .into_iter()
                            .filter(|key| {
                                key.enabled
                                    && key.name == name
                                    && key.group.clone().unwrap_or_default() == wanted
                            })
                            .max_by_key(|key| key.id.parse::<i64>().unwrap_or(0));
                        match existing {
                            Some(key) => {
                                Ok((newapi::unmask(http, info, credential, key).await?, false))
                            }
                            None => Ok((
                                newapi::create_key(
                                    http,
                                    info,
                                    credential,
                                    &name,
                                    group_id.as_deref(),
                                )
                                .await?,
                                true,
                            )),
                        }
                    }
                    Platform::Sub2api => {
                        // sub2api 的 Key 列表里分组显示为名字，按 ID 找到名字再比较。
                        let groups = sub2api::groups(http, info, credential).await?;
                        let wanted = group_id.as_ref().and_then(|id| {
                            groups
                                .iter()
                                .find(|group| &group.id == id)
                                .map(|group| group.name.clone())
                        });
                        let existing = sub2api::list_keys(http, info, credential)
                            .await?
                            .into_iter()
                            .filter(|key| key.enabled && key.name == name && key.group == wanted)
                            .max_by_key(|key| key.id.parse::<i64>().unwrap_or(0));
                        match existing {
                            Some(key) => Ok((key, false)),
                            None => Ok((
                                sub2api::create_key(
                                    http,
                                    info,
                                    credential,
                                    &name,
                                    group_id.as_deref(),
                                )
                                .await?,
                                true,
                            )),
                        }
                    }
                }
            })
        })
        .await
    }

    pub fn sites(&self) -> Result<Vec<SiteView>, SiteError> {
        Ok(self
            .store
            .list()
            .map_err(storage)?
            .iter()
            .map(SiteView::from)
            .collect())
    }

    pub fn remove(&self, id: &str) -> Result<(), SiteError> {
        self.secrets.delete(id).map_err(storage)?;
        self.store.remove(id).map_err(storage)?;
        Ok(())
    }

    /// 用保存的凭证执行一次站点操作；sub2api 访问令牌过期时续期后重试一次。
    async fn call<T>(
        &self,
        id: &str,
        op: impl for<'a> Fn(&'a Http, &'a SiteInfo, &'a Credential) -> SiteFuture<'a, T>,
    ) -> Result<T, SiteError> {
        let (record, credential) = self.load(id)?;
        // 在登录窗口里登录的站点，用那个窗口的 User-Agent。
        let site_http = record.info.user_agent.as_deref().map(Http::with_user_agent);
        let http = site_http.as_ref().unwrap_or(&self.http);
        match op(http, &record.info, &credential).await {
            Err(SiteError::SessionExpired) if record.info.platform == Platform::Sub2api => {
                let refreshed = {
                    let _guard = self.refresh_lock.lock().await;
                    // 别的请求可能刚续期过：用钥匙串里最新的那一对。
                    let (_, latest) = self.load(id)?;
                    if latest != credential {
                        latest
                    } else {
                        let refreshed = sub2api::refresh(http, &record.info, &credential).await?;
                        self.store_credential(id, &refreshed)?;
                        refreshed
                    }
                };
                op(http, &record.info, &refreshed).await
            }
            result => result,
        }
    }

    pub async fn account(&self, id: &str) -> Result<AccountSummary, SiteError> {
        self.call(id, |http, info, credential| {
            Box::pin(async move {
                match info.platform {
                    Platform::Newapi => newapi::account(http, info, credential).await,
                    Platform::Sub2api => sub2api::account(http, info, credential).await,
                }
            })
        })
        .await
    }

    /// Key 列表。交给界面的都是打码的，完整 Key 只在需要时用 [`Self::reveal_key`] 取。
    pub async fn keys(&self, id: &str) -> Result<Vec<KeySummary>, SiteError> {
        let keys = self
            .call(id, |http, info, credential| {
                Box::pin(async move {
                    match info.platform {
                        Platform::Newapi => newapi::list_keys(http, info, credential).await,
                        Platform::Sub2api => sub2api::list_keys(http, info, credential).await,
                    }
                })
            })
            .await?;
        Ok(keys.into_iter().map(KeySummary::for_display).collect())
    }

    /// 某个 Key 的完整内容（用户点复制时才取）。
    pub async fn reveal_key(&self, id: &str, key_id: &str) -> Result<String, SiteError> {
        let key_id = key_id.to_string();
        self.call(id, |http, info, credential| {
            let key_id = key_id.clone();
            Box::pin(async move {
                match info.platform {
                    Platform::Newapi => newapi::full_key(http, info, credential, &key_id).await,
                    Platform::Sub2api => sub2api::full_key(http, info, credential, &key_id).await,
                }
            })
        })
        .await
    }

    pub async fn groups(&self, id: &str) -> Result<Vec<GroupSummary>, SiteError> {
        self.call(id, |http, info, credential| {
            Box::pin(async move {
                match info.platform {
                    Platform::Newapi => newapi::groups(http, info, credential).await,
                    Platform::Sub2api => sub2api::groups(http, info, credential).await,
                }
            })
        })
        .await
    }

    /// 新建 Key。和列表一样只返回打码的版本，要用时再取完整 Key。
    pub async fn create_key(
        &self,
        id: &str,
        name: &str,
        group: Option<&str>,
    ) -> Result<KeySummary, SiteError> {
        let name = name.trim().to_string();
        let group = group.map(str::to_string);
        let created = self
            .call(id, |http, info, credential| {
                let name = name.clone();
                let group = group.clone();
                Box::pin(async move {
                    match info.platform {
                        Platform::Newapi => {
                            newapi::create_key(http, info, credential, &name, group.as_deref())
                                .await
                        }
                        Platform::Sub2api => {
                            sub2api::create_key(http, info, credential, &name, group.as_deref())
                                .await
                        }
                    }
                })
            })
            .await?;
        Ok(created.for_display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;
    use serde_json::{Value, json};
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({ "code": 0, "message": "success", "data": data }))
    }

    fn accounts(dir: &std::path::Path) -> Accounts {
        Accounts::new(
            Http::new(),
            SiteStore::new(dir.join("sites.json")),
            Box::new(MemoryStore::default()),
        )
    }

    #[tokio::test]
    async fn sub2api_login_with_code_is_saved_and_refreshed_when_expired() {
        let server = MockServer::start().await;
        let dir = std::env::temp_dir().join(format!("transitlink-accounts-{}", std::process::id()));
        let accounts = accounts(&dir);
        Mock::given(method("GET"))
            .and(path("/api/v1/settings/public"))
            .respond_with(ok(
                json!({ "site_name": "Relay", "registration_enabled": true }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/login"))
            .respond_with(ok(json!({ "requires_2fa": true, "temp_token": "tmp" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/login/2fa"))
            .and(body_partial_json(json!({ "totp_code": "000000" })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "code": 400, "message": "验证码错误" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/api/v1/auth/login/2fa")).and(body_partial_json(json!({ "totp_code": "123456" })))
            .respond_with(ok(json!({ "access_token": "old", "refresh_token": "rt-1", "user": { "email": "a@example.com" } }))).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/v1/auth/me"))
            .and(header("Authorization", "Bearer old"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(json!({ "code": 401, "message": "TOKEN_EXPIRED" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/refresh"))
            .and(body_partial_json(json!({ "refresh_token": "rt-1" })))
            .respond_with(ok(
                json!({ "access_token": "new", "refresh_token": "rt-2" }),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/auth/me"))
            .and(header("Authorization", "Bearer new"))
            .respond_with(ok(json!({ "email": "a@example.com", "balance": 3.5 })))
            .mount(&server)
            .await;

        let LoginOutcome::NeedCode { flow_id, stage } = accounts
            .login(&server.uri(), "a@example.com", "pw")
            .await
            .unwrap()
        else {
            panic!("code expected")
        };
        assert_eq!(stage, CodeStage::Login);
        // 输错验证码可以重试。
        assert!(matches!(
            accounts.submit_code(&flow_id, "000000").await,
            Err(SiteError::InvalidCode(_))
        ));
        let LoginOutcome::Done { site } = accounts.submit_code(&flow_id, "123456").await.unwrap()
        else {
            panic!("done expected")
        };
        assert_eq!(site.account, "a@example.com");
        assert_eq!(accounts.sites().unwrap().len(), 1);
        assert!(
            matches!(
                accounts.submit_code(&flow_id, "123456").await,
                Err(SiteError::SessionExpired)
            ),
            "flows are single-use"
        );

        // 访问令牌过期：自动续期，保存新令牌，再重试。
        let summary = accounts.account(&site.id).await.unwrap();
        assert_eq!(summary.balance_usd, 3.5);
        let (_, credential) = accounts.load(&site.id).unwrap();
        assert_eq!(
            credential,
            Credential::Sub2api {
                access_token: "new".into(),
                refresh_token: "rt-2".into()
            }
        );

        // 交给界面的 Key 列表是打码的；复制时才取完整 Key。
        Mock::given(method("GET"))
            .and(path("/api/v1/groups/available"))
            .respond_with(ok(json!([])))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/v1/keys")).and(header("Authorization", "Bearer new"))
            .respond_with(ok(json!({ "items": [{ "id": 4, "name": "cc", "key": "sk-0123456789abcdef", "status": "active" }] })))
            .mount(&server).await;
        let keys = accounts.keys(&site.id).await.unwrap();
        assert_eq!(keys[0].key, "sk-0123**********cdef");
        assert!(keys[0].masked);
        assert_eq!(
            accounts.reveal_key(&site.id, "4").await.unwrap(),
            "sk-0123456789abcdef"
        );

        // 同一账号重新登录不会多出一条记录。
        accounts
            .save(site_info(&server.uri()), "A@example.com", &credential)
            .unwrap();
        assert_eq!(accounts.sites().unwrap().len(), 1);

        accounts.remove(&site.id).unwrap();
        assert!(accounts.sites().unwrap().is_empty());
        assert!(matches!(
            accounts.account(&site.id).await,
            Err(SiteError::SessionExpired)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn web_login_keeps_the_login_window_user_agent() {
        let server = MockServer::start().await;
        let dir = std::env::temp_dir().join(format!("transitlink-web-{}", std::process::id()));
        let accounts = accounts(&dir);
        // 站点把会话绑定到登录时的 User-Agent：以后的请求必须用登录窗口的 User-Agent。
        Mock::given(method("GET"))
            .and(path("/api/v1/auth/me"))
            .and(header("Authorization", "Bearer web-at"))
            .and(header("User-Agent", "Mozilla/5.0 WebView"))
            .respond_with(ok(json!({ "email": "a@example.com", "balance": 1.5 })))
            .mount(&server)
            .await;

        let outcome = accounts
            .finish_web_login(
                site_info(&server.uri()),
                WebSession::Sub2api {
                    access_token: "web-at".into(),
                    refresh_token: "web-rt".into(),
                    account: "a@example.com".into(),
                    user_agent: "Mozilla/5.0 WebView".into(),
                },
            )
            .await
            .unwrap();
        let LoginOutcome::Done { site } = outcome else {
            panic!("done expected")
        };
        assert_eq!(site.account, "a@example.com");
        assert_eq!(accounts.account(&site.id).await.unwrap().balance_usd, 1.5);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn site_info(base_url: &str) -> SiteInfo {
        SiteInfo {
            platform: Platform::Sub2api,
            base_url: base_url.into(),
            name: "Relay".into(),
            logo_url: None,
            captcha_required: false,
            password_login_disabled: false,
            email_only: true,
            quota_per_unit: None,
            password_encryption: false,
            user_agent: None,
        }
    }
}
