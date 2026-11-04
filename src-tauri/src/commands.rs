//! 前端可以调用的命令。参数名在前端用驼峰写法（例如 flowId）。

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::accounts::{Accounts, LoginOutcome, SiteView};
use crate::sites::{AccountSummary, GroupSummary, KeySummary, SiteError, SiteInfo};
use crate::{paths, tools};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    data_dir: String,
    platform: String,
}

/// 命令出错时返回给前端：`code` 对应界面文案（sites.errors.*），`message` 是站点自己的说明。
#[derive(Debug, Serialize)]
pub struct CommandError {
    code: String,
    message: Option<String>,
}

impl From<SiteError> for CommandError {
    fn from(error: SiteError) -> Self {
        CommandError {
            code: error.code().to_string(),
            message: error.message(),
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// 应用名称、版本、数据目录和操作系统。
#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    let package = app.package_info();
    AppInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        data_dir: paths::app_data_dir().display().to_string(),
        platform: std::env::consts::OS.to_string(),
    }
}

/// 检测本机的编程工具（第一期：Claude Code、Codex）。要运行 `--version`，放到阻塞线程里执行，不卡界面。
#[tauri::command]
pub async fn detect_tools() -> Vec<tools::ToolStatus> {
    tauri::async_runtime::spawn_blocking(tools::detect_all)
        .await
        .unwrap_or_default()
}

/// 识别站点类型和登录要求。
#[tauri::command]
pub async fn detect_site(accounts: State<'_, Accounts>, url: String) -> CommandResult<SiteInfo> {
    Ok(accounts.detect(&url).await?)
}

/// 账号密码登录。
#[tauri::command]
pub async fn login_site(
    accounts: State<'_, Accounts>,
    url: String,
    account: String,
    password: String,
) -> CommandResult<LoginOutcome> {
    Ok(accounts.login(&url, &account, &password).await?)
}

/// 提交两步验证码。
#[tauri::command]
pub async fn submit_login_code(
    accounts: State<'_, Accounts>,
    flow_id: String,
    code: String,
) -> CommandResult<LoginOutcome> {
    Ok(accounts.submit_code(&flow_id, &code).await?)
}

#[tauri::command]
pub fn cancel_login(accounts: State<'_, Accounts>, flow_id: String) {
    accounts.cancel(&flow_id);
}

#[tauri::command]
pub fn list_sites(accounts: State<'_, Accounts>) -> CommandResult<Vec<SiteView>> {
    Ok(accounts.sites()?)
}

/// 删除站点：同时删掉钥匙串里的凭证。
#[tauri::command]
pub fn remove_site(accounts: State<'_, Accounts>, id: String) -> CommandResult<()> {
    Ok(accounts.remove(&id)?)
}

#[tauri::command]
pub async fn site_account(
    accounts: State<'_, Accounts>,
    id: String,
) -> CommandResult<AccountSummary> {
    Ok(accounts.account(&id).await?)
}

#[tauri::command]
pub async fn site_keys(
    accounts: State<'_, Accounts>,
    id: String,
) -> CommandResult<Vec<KeySummary>> {
    Ok(accounts.keys(&id).await?)
}

/// 把完整 Key 直接写进系统剪贴板，完整 Key 不经过网页层。
#[tauri::command]
pub async fn copy_site_key(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    id: String,
    key_id: String,
) -> CommandResult<()> {
    let key = accounts.reveal_key(&id, &key_id).await?;
    app.clipboard()
        .write_text(key)
        .map_err(|error| SiteError::Unexpected(format!("clipboard: {error}")))?;
    Ok(())
}

#[tauri::command]
pub async fn site_groups(
    accounts: State<'_, Accounts>,
    id: String,
) -> CommandResult<Vec<GroupSummary>> {
    Ok(accounts.groups(&id).await?)
}

#[tauri::command]
pub async fn create_site_key(
    accounts: State<'_, Accounts>,
    id: String,
    name: String,
    group: Option<String>,
) -> CommandResult<KeySummary> {
    Ok(accounts.create_key(&id, &name, group.as_deref()).await?)
}
