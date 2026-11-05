//! 前端可以调用的命令。参数名在前端用驼峰写法（例如 flowId）。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::accounts::{Accounts, LoginOutcome, SiteView};
use crate::configure::probe::TestResult;
use crate::configure::{ApplyOutcome, ConfigError, Configurator, SetupPlan, ToolOverview};
use crate::install::{InstallOutcome, Installer};
use crate::settings::{AppSettings, SettingsStore};
use crate::siteconfig::{self, SiteConfig};
use crate::sites::http::Http;
use crate::sites::{AccountSummary, GroupSummary, KeySummary, SiteError, SiteInfo};
use crate::tickets::{
    ImageUpload, NewTicket, TicketDetail, TicketError, TicketList, TicketSession, Tickets,
};
use crate::tools::ToolKind;
use crate::upgrade::{self, ToolLatest};
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
            ConfigError::Unsupported(what) => ("configUnsupported", Some(what)),
            ConfigError::Busy => ("configBusy", None),
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

impl From<TicketError> for CommandError {
    fn from(error: TicketError) -> Self {
        CommandError {
            code: error.code().to_string(),
            message: error.message(),
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// 冷启动时由专属链接打开：链接先存在这里，前端准备好后来取（运行中点的链接也走这里）。
#[derive(Default)]
pub struct PendingLink(pub std::sync::Mutex<Option<String>>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteLink {
    config_url: String,
    config: SiteConfig,
    /// 专属链接的地址（不带协议），和站点不是同一个域名时在接入窗口里给用户看。
    link_host: String,
    /// 专属链接和站点是不是同一个域名（同一个站长）。不是的话接入窗口里提醒用户确认链接是站长发的。
    same_owner: bool,
}

/// 读取专属链接（或者直接是配置地址）指向的站长配置。
#[tauri::command]
pub async fn load_site_link(link: String) -> CommandResult<SiteLink> {
    let config_url = siteconfig::config_url_from(&link).ok_or(SiteError::LinkInvalid)?;
    let config = siteconfig::fetch(&Http::new(), &config_url).await?;
    let link_host = crate::tickets::hub_host(&config_url)
        .or_else(|| {
            url::Url::parse(&config_url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
        })
        .unwrap_or_default();
    let same_owner = crate::tickets::same_owner(&config.site.base_url, &config_url);
    Ok(SiteLink {
        config_url,
        config,
        link_host,
        same_owner,
    })
}

/// 用户在接入窗口里接受了专属链接：记下站长配置（品牌、推荐、公告、客服），也算确认了链接里的工单服务。
#[tauri::command]
pub async fn attach_site_link(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    id: String,
    config_url: String,
) -> CommandResult<SiteView> {
    let config = siteconfig::fetch(&Http::new(), &config_url).await?;
    let view = accounts.accept_link(&id, &config_url, config)?;
    refresh_tray(&app);
    Ok(view)
}

/// 更新站长配置：前端启动时和每半小时调用，超过 10 分钟没更新的才去读。读取失败的保留旧配置。
/// 返回有没有更新。
#[tauri::command]
pub async fn refresh_site_links(accounts: State<'_, Accounts>) -> CommandResult<bool> {
    let http = Http::new();
    let mut changed = false;
    for (id, config_url) in accounts.stale_configs(10 * 60) {
        if let Ok(config) = siteconfig::fetch(&http, &config_url).await {
            changed |= accounts.attach_config(&id, &config_url, config).is_ok();
        }
    }
    Ok(changed)
}

/// 取走等着处理的专属链接（配置地址）。
#[tauri::command]
pub fn take_pending_link(pending: State<'_, PendingLink>) -> Option<String> {
    pending.0.lock().ok().and_then(|mut slot| slot.take())
}

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

/// 记下界面上的当前站点（`None`：不指定，界面按工具在用的站点或最近接入的站点显示）。
#[tauri::command]
pub fn set_current_site(
    settings: State<'_, SettingsStore>,
    accounts: State<'_, Accounts>,
    site_id: Option<String>,
) -> CommandResult<AppSettings> {
    if let Some(id) = &site_id {
        accounts.site(id)?;
    }
    settings
        .update(|settings| settings.current_site = site_id)
        .map_err(settings_error)
}

/// 切换站点的结果：哪些工具切过去了，哪些没切成功（应用里切换和托盘切换共用）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchReport {
    pub site_id: String,
    pub switched: Vec<ToolKind>,
    pub failures: Vec<SwitchFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchFailure {
    pub kind: ToolKind,
    pub error: CommandError,
}

/// 把这些工具一个一个切到这个站点（沿用在那个站点上选过的分组和模型）。
pub async fn switch_tools_to(
    configurator: &Configurator,
    accounts: &Accounts,
    site_id: &str,
    kinds: &[ToolKind],
) -> SwitchReport {
    let mut report = SwitchReport {
        site_id: site_id.to_string(),
        switched: Vec::new(),
        failures: Vec::new(),
    };
    for &kind in kinds {
        match configurator.switch(accounts, kind, site_id).await {
            Ok(_) => report.switched.push(kind),
            Err(error) => report.failures.push(SwitchFailure {
                kind,
                error: error.into(),
            }),
        }
    }
    report
}

/// 应用里切换站点时，把用户勾选的工具也切过去。
#[tauri::command]
pub async fn switch_tools(
    app: AppHandle,
    configurator: State<'_, Configurator>,
    accounts: State<'_, Accounts>,
    site_id: String,
    kinds: Vec<ToolKind>,
) -> CommandResult<SwitchReport> {
    accounts.site(&site_id)?;
    let report = switch_tools_to(&configurator, &accounts, &site_id, &kinds).await;
    refresh_tray(&app);
    Ok(report)
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

/// 一键安装工具（官方安装脚本或 npm）。`mirror`：npm 用国内镜像。安装过程的输出通过
/// `tool-install` 事件发给界面。已经在装别的工具时返回 installBusy。
#[tauri::command]
pub async fn install_tool(
    app: AppHandle,
    installer: State<'_, Installer>,
    kind: ToolKind,
    mirror: bool,
) -> CommandResult<InstallOutcome> {
    let outcome = installer
        .install(&app, kind, mirror)
        .await
        .map_err(|_| CommandError {
            code: "installBusy".to_string(),
            message: None,
        })?;
    refresh_tray(&app);
    Ok(outcome)
}

/// 升级工具（按工具是怎么装的选办法）。`version`：要升到的版本（界面上显示的最新版）。输出同样通过
/// `tool-install` 事件发给界面。
#[tauri::command]
pub async fn upgrade_tool(
    app: AppHandle,
    installer: State<'_, Installer>,
    kind: ToolKind,
    mirror: bool,
    version: Option<String>,
) -> CommandResult<InstallOutcome> {
    let outcome = installer
        .upgrade(&app, kind, mirror, version.as_deref())
        .await
        .map_err(|_| CommandError {
            code: "installBusy".to_string(),
            message: None,
        })?;
    refresh_tray(&app);
    Ok(outcome)
}

/// 界面上检测到的一个工具和它的版本。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolVersionQuery {
    kind: ToolKind,
    version: Option<String>,
}

/// 各工具 npm 上的最新版本，和本机的版本比较（同时查，每个最多等 10 秒）。
#[tauri::command]
pub async fn tool_versions(tools: Vec<ToolVersionQuery>) -> Vec<ToolLatest> {
    let http = Http::new();
    let tasks: Vec<_> = tools
        .into_iter()
        .map(|query| {
            let http = http.clone();
            tauri::async_runtime::spawn(async move {
                let latest = upgrade::latest(&http, query.kind, query.version.as_deref()).await;
                let compare = |check: fn(&str, &str) -> bool| {
                    matches!(
                        (&latest, &query.version),
                        (Some(latest), Some(local)) if check(latest, local)
                    )
                };
                ToolLatest {
                    kind: query.kind,
                    update_available: compare(upgrade::is_newer),
                    up_to_date: compare(upgrade::same_version),
                    latest,
                }
            })
        })
        .collect();
    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(result) = task.await {
            results.push(result);
        }
    }
    results
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

/// 删除站点：同时删掉钥匙串里的凭证和内存里的工单会话；删的是当前站点时，界面换回自动挑选。
#[tauri::command]
pub fn remove_site(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    settings: State<'_, SettingsStore>,
    id: String,
) -> CommandResult<()> {
    accounts.remove(&id)?;
    tickets.forget(&id);
    let _ = settings.update(|settings| {
        if settings.current_site.as_deref() == Some(id.as_str()) {
            settings.current_site = None;
        }
    });
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

/// 用户确认专属链接里的 TransitHub 是站长的：以后打开工单时，才把站点的登录交给它核实身份。
#[tauri::command]
pub fn ticket_allow(accounts: State<'_, Accounts>, site_id: String) -> CommandResult<SiteView> {
    Ok(accounts.allow_ticket_hub(&site_id)?)
}

/// 工单会话：站点上的用户、问题分类和图片限制。第一次调用时用站点的登录换一个工单会话
/// （会话令牌留在 Rust 侧，不交给网页）。
#[tauri::command]
pub async fn ticket_session(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
) -> CommandResult<TicketSession> {
    Ok(tickets.session_info(&accounts, &site_id).await?)
}

/// 我在这个站点提过的工单，和有新回复的工单数。
#[tauri::command]
pub async fn tickets_list(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
) -> CommandResult<TicketList> {
    Ok(tickets.list(&accounts, &site_id).await?)
}

/// 工单详情（打开后客服的回复记为已读）。
#[tauri::command]
pub async fn ticket_detail(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
    ticket_id: String,
) -> CommandResult<TicketDetail> {
    Ok(tickets.detail(&accounts, &site_id, &ticket_id).await?)
}

/// 新建工单。图片是 base64，最多 5 张、每张 5 MB。
#[tauri::command]
pub async fn ticket_create(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
    draft: NewTicket,
    images: Vec<ImageUpload>,
) -> CommandResult<TicketDetail> {
    Ok(tickets.create(&accounts, &site_id, draft, images).await?)
}

/// 追问（可以只发图片）。
#[tauri::command]
pub async fn ticket_reply(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
    ticket_id: String,
    body: String,
    images: Vec<ImageUpload>,
) -> CommandResult<TicketDetail> {
    Ok(tickets
        .reply(&accounts, &site_id, &ticket_id, &body, images)
        .await?)
}

/// 关闭工单。
#[tauri::command]
pub async fn ticket_close(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
    ticket_id: String,
) -> CommandResult<TicketDetail> {
    Ok(tickets.close(&accounts, &site_id, &ticket_id).await?)
}

/// 消息里的一张图片，返回 `data:` 地址。
#[tauri::command]
pub async fn ticket_attachment(
    accounts: State<'_, Accounts>,
    tickets: State<'_, Tickets>,
    site_id: String,
    attachment_id: String,
    thumb: bool,
) -> CommandResult<String> {
    Ok(tickets
        .attachment(&accounts, &site_id, &attachment_id, thumb)
        .await?)
}
