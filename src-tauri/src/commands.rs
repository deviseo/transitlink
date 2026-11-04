//! 前端可以调用的命令。参数名在前端用驼峰写法（例如 flowId）。

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::accounts::{Accounts, LoginOutcome, SiteView};
use crate::configure::probe::TestResult;
use crate::configure::{ApplyOutcome, ConfigError, Configurator, SetupPlan, ToolOverview};
use crate::settings::{AppSettings, SettingsStore};
use crate::sites::{AccountSummary, GroupSummary, KeySummary, SiteError, SiteInfo};
use crate::tools::ToolKind;
use crate::{paths, tools};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    data_dir: String,
    platform: String,
}

/// 命令出错时返回给前端：`code` 对应界面文案（errors.*），`message` 是站点自己的说明或出错的文件。
#[derive(Debug, Clone, Serialize)]
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

impl From<ConfigError> for CommandError {
    fn from(error: ConfigError) -> Self {
        let (code, message) = match error {
            ConfigError::Invalid => ("configInvalid", None),
            ConfigError::InvalidFile(path) => ("configInvalid", Some(path)),
            ConfigError::Io(message) => ("configWrite", Some(message)),
            ConfigError::NotConfigured => ("notConfigured", None),
            ConfigError::Test(TestResult::NoFamilyModels { .. }) => ("noFamilyModels", None),
            ConfigError::Test(TestResult::BadKey { message }) => ("badKey", message),
            ConfigError::Test(TestResult::Failed { message }) => ("probeFailed", message),
            ConfigError::Test(TestResult::Ok { .. }) => ("unexpected", None),
            ConfigError::Site(error) => return error.into(),
        };
        CommandError {
            code: code.to_string(),
            message,
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// 站点、工具配置或语言变了：托盘菜单跟着更新。
fn refresh_tray(app: &AppHandle) {
    #[cfg(desktop)]
    crate::tray::refresh(app);
    #[cfg(not(desktop))]
    let _ = app;
}

fn settings_error(error: std::io::Error) -> CommandError {
    ConfigError::Io(error.to_string()).into()
}

/// 应用设置（关掉窗口后是否留在托盘等）。
#[tauri::command]
pub fn app_settings(settings: State<'_, SettingsStore>) -> AppSettings {
    settings.get()
}

#[tauri::command]
pub fn set_close_to_tray(
    settings: State<'_, SettingsStore>,
    enabled: bool,
) -> CommandResult<AppSettings> {
    settings
        .update(|settings| settings.close_to_tray = enabled)
        .map_err(settings_error)
}

/// 前端告诉我们界面语言，托盘菜单用同一种语言。
#[tauri::command]
pub fn set_language(
    app: AppHandle,
    settings: State<'_, SettingsStore>,
    language: String,
) -> CommandResult<()> {
    settings
        .update(|settings| settings.language = language)
        .map_err(settings_error)?;
    refresh_tray(&app);
    Ok(())
}

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

/// 检测本机的编程工具（第一期：Claude Code、Codex）和它们现在的配置。要运行 `--version`，
/// 放到阻塞线程里执行，不卡界面。
#[tauri::command]
pub async fn detect_tools(
    configurator: State<'_, Configurator>,
) -> CommandResult<Vec<ToolOverview>> {
    let statuses = tauri::async_runtime::spawn_blocking(tools::detect_all)
        .await
        .unwrap_or_default();
    Ok(configurator.overview(statuses))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

#[cfg(desktop)]
fn update_error(error: tauri_plugin_updater::Error) -> CommandError {
    CommandError {
        code: "updateFailed".to_string(),
        message: Some(error.to_string()),
    }
}

/// 检查有没有新版本：读 GitHub Releases 上最新正式版的 `latest.json`，签名用内置的公钥校验。
#[tauri::command]
pub async fn check_update(app: AppHandle) -> CommandResult<Option<UpdateInfo>> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        let update = app
            .updater()
            .map_err(update_error)?
            .check()
            .await
            .map_err(update_error)?;
        Ok(update.map(|update| UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
        }))
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Ok(None)
    }
}

/// 下载并安装新版本，装好后重启应用。
#[tauri::command]
pub async fn install_update(app: AppHandle) -> CommandResult<()> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        let update = app
            .updater()
            .map_err(update_error)?
            .check()
            .await
            .map_err(update_error)?;
        if let Some(update) = update {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(update_error)?;
            app.restart();
        }
        Ok(())
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Ok(())
    }
}

/// 一键配置前的选项：站点的分组和每个工具推荐的分组。
#[tauri::command]
pub async fn setup_plan(
    accounts: State<'_, Accounts>,
    configurator: State<'_, Configurator>,
    site_id: String,
) -> CommandResult<SetupPlan> {
    Ok(configurator.plan(&accounts, &site_id).await?)
}

/// 一键配置一个工具：专用 Key → 测试 → 备份 → 写配置。
#[tauri::command]
pub async fn apply_tool(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    configurator: State<'_, Configurator>,
    kind: ToolKind,
    site_id: String,
    group_id: Option<String>,
    model: Option<String>,
) -> CommandResult<ApplyOutcome> {
    let outcome = configurator
        .apply(
            &accounts,
            kind,
            &site_id,
            group_id.as_deref(),
            model.as_deref(),
        )
        .await?;
    refresh_tray(&app);
    Ok(outcome)
}

/// 用工具配置里现在的地址和 Key 测试连通。
#[tauri::command]
pub async fn test_tool(
    configurator: State<'_, Configurator>,
    kind: ToolKind,
) -> CommandResult<TestResult> {
    Ok(configurator.test(kind).await?)
}

/// 工具可选的模型。
#[tauri::command]
pub async fn tool_models(
    configurator: State<'_, Configurator>,
    kind: ToolKind,
) -> CommandResult<Vec<String>> {
    Ok(configurator.models(kind).await?)
}

/// 换模型（Claude Code 传空表示用它自己的默认模型）。
#[tauri::command]
pub async fn set_tool_model(
    configurator: State<'_, Configurator>,
    kind: ToolKind,
    model: Option<String>,
) -> CommandResult<()> {
    Ok(configurator.set_model(kind, model.as_deref()).await?)
}

/// 还原到第一次用 TransitLink 配置之前。
#[tauri::command]
pub async fn restore_tool(
    app: AppHandle,
    configurator: State<'_, Configurator>,
    kind: ToolKind,
) -> CommandResult<()> {
    configurator.restore(kind).await?;
    refresh_tray(&app);
    Ok(())
}

/// 识别站点类型和登录要求。
#[tauri::command]
pub async fn detect_site(accounts: State<'_, Accounts>, url: String) -> CommandResult<SiteInfo> {
    Ok(accounts.detect(&url).await?)
}

/// 账号密码登录。
#[tauri::command]
pub async fn login_site(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    url: String,
    account: String,
    password: String,
) -> CommandResult<LoginOutcome> {
    let outcome = accounts.login(&url, &account, &password).await?;
    refresh_tray(&app);
    Ok(outcome)
}

/// 提交两步验证码。
#[tauri::command]
pub async fn submit_login_code(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    flow_id: String,
    code: String,
) -> CommandResult<LoginOutcome> {
    let outcome = accounts.submit_code(&flow_id, &code).await?;
    refresh_tray(&app);
    Ok(outcome)
}

/// 开了人机验证的站点：打开站点自己的登录页，等用户登录完成。
#[tauri::command]
pub async fn web_login(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    settings: State<'_, SettingsStore>,
    url: String,
) -> CommandResult<LoginOutcome> {
    #[cfg(desktop)]
    {
        let info = accounts.detect(&url).await?;
        let title = if settings.get().language.starts_with("zh") {
            format!("登录 {}", info.name)
        } else {
            format!("Sign in to {}", info.name)
        };
        let session = crate::webauth::capture(&app, &info, &title).await?;
        let outcome = accounts.finish_web_login(info, session).await?;
        refresh_tray(&app);
        Ok(outcome)
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, accounts, settings, url);
        Err(SiteError::CaptchaRequired.into())
    }
}

/// 关掉登录窗口（在应用里点了取消）。
#[tauri::command]
pub fn cancel_web_login(app: AppHandle) {
    #[cfg(desktop)]
    crate::webauth::close(&app);
    #[cfg(not(desktop))]
    let _ = app;
}

/// 提交密码：在登录窗口里登录的 new-api 站点，创建个人访问令牌时要再验证一次。
#[tauri::command]
pub async fn submit_login_password(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    flow_id: String,
    password: String,
) -> CommandResult<LoginOutcome> {
    let outcome = accounts.submit_password(&flow_id, &password).await?;
    refresh_tray(&app);
    Ok(outcome)
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
pub fn remove_site(app: AppHandle, accounts: State<'_, Accounts>, id: String) -> CommandResult<()> {
    accounts.remove(&id)?;
    refresh_tray(&app);
    Ok(())
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
