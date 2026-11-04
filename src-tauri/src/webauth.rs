//! 开了人机验证（Turnstile 等）的站点，不能直接用账号密码接口登录。这里在应用里打开站点自己的
//! 登录页（无痕窗口，关掉就什么都不留），用户在里面正常登录；登录完成后取走登录状态，关掉窗口。
//!
//! - new-api 新版：读 `new_api_refresh` cookie（HttpOnly，网页脚本读不到，只能用原生接口读）。
//! - new-api 旧版：读 `session` cookie，再从网页的 localStorage 里读用户 ID。
//! - sub2api：从网页的 localStorage 读令牌（`auth_token`、`refresh_token`、`auth_user`），
//!   顺便记下窗口的 User-Agent（站点可能把会话绑定到它）。
//!
//! 登录窗口不能调用应用的任何命令（权限只给了主窗口），这里只是从外面读它的 cookie 和 localStorage。

use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::accounts::WebSession;
use crate::sites::http::text;
use crate::sites::{Platform, SiteError, SiteInfo, newapi};

const LABEL: &str = "site-login";
const POLL: Duration = Duration::from_millis(800);
/// 十分钟还没登录完就放弃。
const TIMEOUT: Duration = Duration::from_secs(600);

/// 打开登录窗口，等用户登录完成。用户关掉窗口时返回 LoginCancelled。
pub async fn capture(
    app: &AppHandle,
    info: &SiteInfo,
    title: &str,
) -> Result<WebSession, SiteError> {
    close(app);
    let url =
        url::Url::parse(&format!("{}/login", info.base_url)).map_err(|_| SiteError::InvalidUrl)?;
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title(title)
        .inner_size(480.0, 760.0)
        .center()
        .incognito(true)
        .build()
        .map_err(|error| SiteError::Unexpected(format!("login window: {error}")))?;

    let started = Instant::now();
    loop {
        tokio::time::sleep(POLL).await;
        let Some(window) = app.get_webview_window(LABEL) else {
            return Err(SiteError::LoginCancelled);
        };
        if started.elapsed() > TIMEOUT {
            let _ = window.destroy();
            return Err(SiteError::LoginCancelled);
        }
        if let Some(session) = read_session(&window, info).await {
            // 先关窗口再用：刷新令牌用一次就换新，不能让网页再拿旧的去刷新。
            let _ = window.destroy();
            return Ok(session);
        }
    }
}

/// 关掉登录窗口（用户在应用里点了取消）。
pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}

async fn read_session(window: &WebviewWindow, info: &SiteInfo) -> Option<WebSession> {
    let origin = serde_json::to_string(&info.base_url).ok()?;
    match info.platform {
        Platform::Newapi => {
            // 刷新令牌 cookie 的 Path 是 /api/user/auth，要按这个地址取。
            let refresh_url =
                url::Url::parse(&format!("{}/api/user/auth/refresh", info.base_url)).ok()?;
            let cookies = window.cookies_for_url(refresh_url).ok()?;
            if let Some(cookie) = cookies.iter().find(|cookie| {
                cookie.name() == newapi::REFRESH_COOKIE && !cookie.value().is_empty()
            }) {
                return Some(WebSession::NewapiRefresh(cookie.value().to_string()));
            }
            let site_url = url::Url::parse(&format!("{}/", info.base_url)).ok()?;
            let session = window
                .cookies_for_url(site_url)
                .ok()?
                .into_iter()
                .find(|cookie| cookie.name() == "session" && !cookie.value().is_empty())?;
            // 有些站点登录前也会发 session cookie，所以还要等网页存下用户信息才算登录完成。
            let script = format!(
                "(() => {{ try {{ if (location.origin !== {origin}) return null; \
                 const user = JSON.parse(localStorage.getItem('user') || 'null'); \
                 return user && user.id ? String(user.id) : null; }} catch (e) {{ return null; }} }})()"
            );
            let user_id = eval(window, &script).await?.as_str()?.to_string();
            Some(WebSession::NewapiLegacy {
                cookie: format!("session={}", session.value()),
                user_id,
            })
        }
        Platform::Sub2api => {
            let script = format!(
                "(() => {{ try {{ if (location.origin !== {origin}) return null; \
                 return {{ token: localStorage.getItem('auth_token'), refresh: localStorage.getItem('refresh_token'), \
                 user: localStorage.getItem('auth_user'), ua: navigator.userAgent }}; }} catch (e) {{ return null; }} }})()"
            );
            let value = eval(window, &script).await?;
            let access_token = value
                .get("token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())?
                .to_string();
            let account = value
                .get("user")
                .and_then(Value::as_str)
                .and_then(|user| serde_json::from_str::<Value>(user).ok())
                .and_then(|user| text(&user, "email").or_else(|| text(&user, "username")))
                .unwrap_or_default();
            Some(WebSession::Sub2api {
                access_token,
                refresh_token: value
                    .get("refresh")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                account,
                user_agent: value.get("ua").and_then(Value::as_str)?.to_string(),
            })
        }
    }
}

/// 在登录窗口里执行一段脚本，取回结果（JSON）。页面还在加载、跳到了别的网站时返回 None。
async fn eval(window: &WebviewWindow, script: &str) -> Option<Value> {
    let (sender, receiver) = tokio::sync::oneshot::channel::<String>();
    let sender = std::sync::Mutex::new(Some(sender));
    window
        .eval_with_callback(script, move |result| {
            if let Some(sender) = sender.lock().ok().and_then(|mut slot| slot.take()) {
                let _ = sender.send(result);
            }
        })
        .ok()?;
    let result = tokio::time::timeout(Duration::from_secs(3), receiver)
        .await
        .ok()?
        .ok()?;
    serde_json::from_str::<Value>(&result)
        .ok()
        .filter(|value| !value.is_null())
}
