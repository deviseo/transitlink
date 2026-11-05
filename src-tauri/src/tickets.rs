//! 工单：用户在 TransitLink 里向站长提问题，站长在 TransitHub 里回复。
//!
//! 只有用专属链接接入的站点才有工单：TransitHub 的地址是配置地址去掉末尾的
//! `/api/client/v1/sites/<代码>`。先用站点的登录凭证换一个工单会话（TransitHub 拿它去站点核实一次身份，
//! 不保存），之后的请求都带这个会话。会话只放在内存里，过期（401）了重新换一个再试一次。
//! sub2api 只发访问令牌，刷新令牌从不离开本机；站点不认访问令牌时，按原来的流程续期后再试一次。

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use reqwest::RequestBuilder;
use reqwest::multipart::{Form, Part};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::accounts::Accounts;
use crate::siteconfig;
use crate::sites::http::{Http, text};
use crate::sites::{Credential, SiteError};
use crate::store::SiteRecord;

/// 每条消息最多几张图片、每张最大多少（和 TransitHub 一致）。
const MAX_IMAGES: usize = 5;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_TITLE_CHARS: usize = 100;
const MAX_BODY_CHARS: usize = 5000;
const MAX_CONTACT_CHARS: usize = 100;
/// 响应最大 16 MB（图片最大 5 MB，工单列表和详情一般只有几十 KB）。
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// 上传、下载图片比普通请求慢，给更长的超时。
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// 站点拒绝了某个登录（sub2api 续期后也不行）以后，这么久之内不再拿同一个登录去换会话：
/// 后台每几分钟检查一次未读，不然会反复续期（每次都换一对新令牌）。重新登录站点后马上就能再试。
const REJECTION_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// 工单出错。`code()` 对应界面文案（errors.*）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketError {
    /// 站点不是用专属链接接入的，没有工单。
    NotLinked,
    /// 站长没开工单，或者关掉了专属链接。
    Disabled,
    /// 站点不认这个登录了（sub2api 续期后也不行），要重新登录站点。
    SiteLoginExpired,
    /// TransitHub 连不上站点，没法核实身份。
    SiteUnavailable,
    TooManyRequests,
    Invalid,
    NotFound,
    Closed,
    TooManyImages,
    ImageTooLarge,
    InvalidImage,
    UploadsDisabled,
    /// 整个请求太大，被 TransitHub 前面的服务器拒绝了（例如 nginx 的上传大小限制）。
    UploadTooLarge,
    /// 工单会话过期了，换了新会话也不行。
    SessionExpired,
    /// 连不上 TransitHub。
    Unreachable,
    /// TransitHub 返回了别的错误（附上 HTTP 状态之类的说明）。
    Failed(Option<String>),
    /// 读取站点凭证、续期时出的错。
    Site(SiteError),
    /// 专属链接里的 TransitHub 还没经过用户同意：打开工单要把站点的登录交给它核实身份。附上它的地址。
    ConsentRequired(String),
}

impl TicketError {
    pub fn code(&self) -> &'static str {
        match self {
            TicketError::NotLinked => "ticketsNotLinked",
            TicketError::Disabled => "ticketsDisabled",
            TicketError::SiteLoginExpired => "ticketsSiteLoginExpired",
            TicketError::SiteUnavailable => "ticketsSiteUnavailable",
            TicketError::TooManyRequests => "ticketsTooManyRequests",
            TicketError::Invalid => "ticketsInvalid",
            TicketError::NotFound => "ticketsNotFound",
            TicketError::Closed => "ticketsClosed",
            TicketError::TooManyImages => "ticketsTooManyImages",
            TicketError::ImageTooLarge => "ticketsImageTooLarge",
            TicketError::InvalidImage => "ticketsInvalidImage",
            TicketError::UploadsDisabled => "ticketsUploadsDisabled",
            TicketError::UploadTooLarge => "ticketsUploadTooLarge",
            TicketError::SessionExpired => "ticketsSessionExpired",
            TicketError::Unreachable => "ticketsUnreachable",
            TicketError::Failed(_) => "ticketsFailed",
            TicketError::Site(error) => error.code(),
            TicketError::ConsentRequired(_) => "ticketsConsentRequired",
        }
    }

    pub fn message(&self) -> Option<String> {
        match self {
            TicketError::Failed(detail) => detail.clone(),
            TicketError::Site(error) => error.message(),
            TicketError::ConsentRequired(host) => Some(host.clone()),
            _ => None,
        }
    }

    /// TransitHub 的错误响应 `{"message": "tickets.errors.<名称>"}`。
    fn from_response(status: u16, key: &str) -> Self {
        match key.strip_prefix("tickets.errors.") {
            Some("disabled") => TicketError::Disabled,
            Some("sessionExpired") => TicketError::SessionExpired,
            Some("siteLoginExpired") => TicketError::SiteLoginExpired,
            Some("siteUnavailable") => TicketError::SiteUnavailable,
            Some("tooManyRequests") => TicketError::TooManyRequests,
            Some("invalid") => TicketError::Invalid,
            Some("notFound") => TicketError::NotFound,
            Some("closed") => TicketError::Closed,
            Some("tooManyImages") => TicketError::TooManyImages,
            Some("imageTooLarge") => TicketError::ImageTooLarge,
            Some("invalidImage") => TicketError::InvalidImage,
            Some("uploadsDisabled") => TicketError::UploadsDisabled,
            _ => match status {
                401 => TicketError::SessionExpired,
                413 => TicketError::UploadTooLarge,
                429 => TicketError::TooManyRequests,
                _ => TicketError::Failed(Some(format!("HTTP {status}"))),
            },
        }
    }
}

impl From<SiteError> for TicketError {
    fn from(error: SiteError) -> Self {
        match error {
            // 站点记录或凭证没了、sub2api 续期失败：都要重新登录站点。
            SiteError::SessionExpired => TicketError::SiteLoginExpired,
            other => TicketError::Site(other),
        }
    }
}

/// 换会话时告诉站长用的是哪个版本、什么系统，例如 `TransitLink 0.2.0 (macOS)`。
fn client_info() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("TransitLink {} ({os})", env!("CARGO_PKG_VERSION"))
}

/// 换会话时交给 TransitHub 的凭证：new-api 个人访问令牌和 sub2api 访问令牌都是 bearer，
/// 旧版 new-api 是会话 cookie 和用户 ID。sub2api 的刷新令牌不发出去。
fn credential_json(credential: &Credential) -> Value {
    match credential {
        Credential::NewapiToken { token } => json!({ "kind": "bearer", "token": token }),
        Credential::Sub2api { access_token, .. } => {
            json!({ "kind": "bearer", "token": access_token })
        }
        Credential::NewapiLegacy { cookie, user_id } => {
            json!({ "kind": "cookie", "cookie": cookie, "userId": user_id })
        }
    }
}

/// TransitHub 上的工单接口：配置地址是 `<TransitHub>/api/client/v1/sites/<代码>`
/// （TransitHub 可能部署在子路径下）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hub {
    base: String,
    code: String,
}

impl Hub {
    fn parse(config_url: &str) -> Option<Hub> {
        let url = url::Url::parse(config_url).ok()?;
        // 换会话要带上站点的登录凭证，只发往 https（本机测试除外）。
        if !siteconfig::allowed(&url) {
            return None;
        }
        let (prefix, code) = url.path().trim_end_matches('/').rsplit_once('/')?;
        let root = prefix.strip_suffix("/api/client/v1/sites")?;
        if code.is_empty() {
            return None;
        }
        Some(Hub {
            base: format!("{}{root}", url.origin().ascii_serialization()),
            code: code.to_string(),
        })
    }

    fn session_url(&self) -> String {
        format!(
            "{}/api/client/v1/sites/{}/tickets/session",
            self.base, self.code
        )
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/client/v1/{path}", self.base)
    }
}

/// 专属链接里 TransitHub 的地址（协议、主机和路径前缀），用来记下用户同意过哪个地址。
pub fn hub_base(config_url: &str) -> Option<String> {
    Hub::parse(config_url).map(|hub| hub.base)
}

/// 给用户看的 TransitHub 地址（去掉协议），例如 `hub.example.com/transithub`。
pub fn hub_host(config_url: &str) -> Option<String> {
    hub_base(config_url).map(|base| hub_display(&base))
}

/// 两个网址是不是同一个站长的：主机相同（IP 地址只认完全相同），或者属于同一个可注册域名
/// （按公共后缀列表：api.example.com 和 hub.example.com 算；a.github.io 和 b.github.io 不算）。
/// 同一个域名只有站长能控制，所以这时工单服务不用用户确认。
pub fn same_owner(site_url: &str, other_url: &str) -> bool {
    let (Ok(site), Ok(other)) = (url::Url::parse(site_url), url::Url::parse(other_url)) else {
        return false;
    };
    match (site.host(), other.host()) {
        (Some(url::Host::Domain(a)), Some(url::Host::Domain(b))) => {
            let (a, b) = (a.trim_end_matches('.'), b.trim_end_matches('.'));
            a.eq_ignore_ascii_case(b)
                || matches!((registrable(a), registrable(b)), (Some(x), Some(y)) if x == y)
        }
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// 主机名的可注册域名（例如 api.example.com.cn → example.com.cn）。
fn registrable(host: &str) -> Option<String> {
    psl::domain_str(&host.to_ascii_lowercase()).map(str::to_string)
}

/// 能不能用站点的登录换工单会话：专属链接里的工单服务和站点是同一个域名，或者用户确认过这个工单服务
/// （打开不同域名的专属链接时在接入窗口里确认，老版本接入的站点在工单页确认）。链接换了地址要重新确认。
pub fn hub_allowed(record: &SiteRecord) -> bool {
    let Some(hub) = record.config_url.as_deref().and_then(hub_base) else {
        return false;
    };
    same_owner(&record.info.base_url, &hub) || record.ticket_hub.as_deref() == Some(hub.as_str())
}

/// 给用户看的 TransitHub 地址（去掉协议）。
fn hub_display(base: &str) -> String {
    base.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

/// 凭证的指纹（只在内存里比较是不是同一个登录，不保存）。
fn fingerprint(credential: &Credential) -> u64 {
    let mut hasher = DefaultHasher::new();
    credential_json(credential).to_string().hash(&mut hasher);
    hasher.finish()
}

/// 工单和图片的 ID 是 TransitHub 生成的十六进制串；拼进地址前先检查，免得拼出别的路径。
fn checked_id(id: &str) -> Result<&str, TicketError> {
    let id = id.trim();
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(id)
    } else {
        Err(TicketError::NotFound)
    }
}

/// 给界面的会话信息：站点上的用户、问题分类和图片限制。会话令牌本身留在 Rust 侧。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketSession {
    pub user: TicketUser,
    pub categories: Vec<String>,
    pub limits: UploadLimits,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketUser {
    pub id: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UploadLimits {
    pub enabled: bool,
    pub max_images: usize,
    pub max_image_bytes: usize,
}

impl Default for UploadLimits {
    fn default() -> Self {
        UploadLimits {
            enabled: false,
            max_images: MAX_IMAGES,
            max_image_bytes: MAX_IMAGE_BYTES,
        }
    }
}

impl UploadLimits {
    /// 不超过 5 张、每张 5 MB；TransitHub 没给（0）时就用这个上限。
    fn normalized(self) -> Self {
        let clamp = |value: usize, max: usize| if value == 0 { max } else { value.min(max) };
        UploadLimits {
            enabled: self.enabled,
            max_images: clamp(self.max_images, MAX_IMAGES),
            max_image_bytes: clamp(self.max_image_bytes, MAX_IMAGE_BYTES),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    #[serde(default)]
    session_token: String,
    #[serde(flatten)]
    session: TicketSession,
}

/// 列表里的一个工单。状态：open 等客服回复；pending 追问后等客服回复；replied 客服回复了；closed 已关闭。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketSummary {
    pub id: String,
    pub title: String,
    pub category: String,
    pub status: String,
    pub contact: String,
    /// 客服回复了、用户还没看。
    pub unread: bool,
    pub last_message_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketList {
    pub items: Vec<TicketSummary>,
    /// 有新回复的工单数。
    pub unread: u32,
}

/// 工单详情：列表里的字段加上全部消息。打开详情时 TransitHub 记下客服的回复已读。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketDetail {
    #[serde(flatten)]
    pub ticket: TicketSummary,
    pub messages: Vec<TicketMessage>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketMessage {
    pub id: String,
    /// customer（用户）或 admin（客服）。
    pub author_type: String,
    pub author_name: String,
    pub body: String,
    pub created_at: String,
    pub attachments: Vec<TicketAttachment>,
}

/// 消息里的图片。内容要另外用 [`Tickets::attachment`] 取。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TicketAttachment {
    pub id: String,
    pub original_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub created_at: String,
}

/// 新建工单时填的内容。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewTicket {
    pub title: String,
    pub category: String,
    pub body: String,
    pub contact: String,
}

/// 界面选的图片：文件名和 base64 内容（也可以是 `data:` 地址）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpload {
    pub name: String,
    pub data_base64: String,
}

/// 检查过的图片。
struct Image {
    name: String,
    mime: &'static str,
    data: Vec<u8>,
}

/// 按文件头判断图片格式。和 TransitHub 一样只收 PNG、JPEG、WebP、GIF。
fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 14 && data.starts_with(b"RIFF") && &data[8..14] == b"WEBPVP" {
        Some("image/webp")
    } else {
        None
    }
}

/// 上传时用的文件名：去掉路径和控制字符，太长的截断；没有时按格式起一个。
fn image_name(name: &str, mime: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .take(100)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() || clean.chars().all(|c| c == '.') {
        format!("image.{}", mime.trim_start_matches("image/"))
    } else {
        clean.to_string()
    }
}

/// 发送前检查图片：数量、大小和格式（按文件内容判断）。TransitHub 还会再检查一遍。
fn prepare_images(
    images: Vec<ImageUpload>,
    limits: &UploadLimits,
) -> Result<Vec<Image>, TicketError> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    if !limits.enabled {
        return Err(TicketError::UploadsDisabled);
    }
    if images.len() > limits.max_images {
        return Err(TicketError::TooManyImages);
    }
    images
        .into_iter()
        .map(|image| {
            let encoded = image.data_base64.trim();
            let encoded = match encoded.split_once(',') {
                Some((head, rest)) if head.starts_with("data:") => rest,
                _ => encoded,
            };
            // 先按长度估算，太大的不用解码。
            if encoded.len() / 4 * 3 > limits.max_image_bytes + 2 {
                return Err(TicketError::ImageTooLarge);
            }
            let data = STANDARD
                .decode(encoded)
                .map_err(|_| TicketError::InvalidImage)?;
            if data.len() > limits.max_image_bytes {
                return Err(TicketError::ImageTooLarge);
            }
            let mime = sniff_image(&data).ok_or(TicketError::InvalidImage)?;
            Ok(Image {
                name: image_name(&image.name, mime),
                mime,
                data,
            })
        })
        .collect()
}

/// 去掉首尾空白，超过长度时报错。
fn limited(value: &str, max_chars: usize) -> Result<String, TicketError> {
    let value = value.trim();
    if value.chars().count() > max_chars {
        return Err(TicketError::Invalid);
    }
    Ok(value.to_string())
}

/// 文字字段和图片：没有图片时发 JSON；有图片时发 multipart 表单（图片的字段名是 images）。
fn with_fields(
    request: RequestBuilder,
    fields: &[(&'static str, String)],
    images: &[Image],
) -> Result<RequestBuilder, TicketError> {
    if images.is_empty() {
        let body: Map<String, Value> = fields
            .iter()
            .map(|(key, value)| (key.to_string(), Value::String(value.clone())))
            .collect();
        return Ok(request.json(&body));
    }
    let mut form = Form::new();
    for (key, value) in fields {
        form = form.text(*key, value.clone());
    }
    for image in images {
        let part = Part::bytes(image.data.clone())
            .file_name(image.name.clone())
            .mime_str(image.mime)
            .map_err(|_| TicketError::InvalidImage)?;
        form = form.part("images", part);
    }
    Ok(request.multipart(form).timeout(UPLOAD_TIMEOUT))
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, TicketError> {
    serde_json::from_slice(bytes)
        .map_err(|error| TicketError::Failed(Some(format!("response: {error}"))))
}

/// 换到的工单会话。
#[derive(Clone)]
struct Session {
    /// 换会话时站点的配置地址：站点换了专属链接，旧会话就不用了。
    config_url: String,
    hub: Hub,
    token: String,
    info: TicketSession,
}

pub struct Tickets {
    http: Http,
    client_info: String,
    /// 每个站点的工单会话（只在内存里，不保存）。
    sessions: Mutex<HashMap<String, Session>>,
    /// 同一时间只换一次会话：并发的请求等着用同一个新会话
    /// （换会话要去站点核实身份，TransitHub 也限制了频率）。
    renew_lock: tauri::async_runtime::Mutex<()>,
    /// 站点拒绝过的登录（时间和凭证指纹），见 REJECTION_BACKOFF。
    rejected: Mutex<HashMap<String, (Instant, u64)>>,
}

impl Tickets {
    pub fn new(http: Http) -> Self {
        Self {
            http,
            client_info: client_info(),
            sessions: Mutex::new(HashMap::new()),
            renew_lock: tauri::async_runtime::Mutex::new(()),
            rejected: Mutex::new(HashMap::new()),
        }
    }

    /// 删除站点时丢掉它的工单会话。
    pub fn forget(&self, site_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(site_id);
        }
    }

    /// 会话信息（用户、问题分类、图片限制）。第一次调用时用站点的登录换一个工单会话。
    pub async fn session_info(
        &self,
        accounts: &Accounts,
        site_id: &str,
    ) -> Result<TicketSession, TicketError> {
        Ok(self.session(accounts, site_id, None).await?.info)
    }

    pub async fn list(
        &self,
        accounts: &Accounts,
        site_id: &str,
    ) -> Result<TicketList, TicketError> {
        let bytes = self
            .authorized(accounts, site_id, |http, hub| {
                Ok(http.get(&hub.url("tickets")))
            })
            .await?;
        parse(&bytes)
    }

    /// 工单详情。打开详情时 TransitHub 把客服的回复记为已读。
    pub async fn detail(
        &self,
        accounts: &Accounts,
        site_id: &str,
        ticket_id: &str,
    ) -> Result<TicketDetail, TicketError> {
        let path = format!("tickets/{}", checked_id(ticket_id)?);
        let bytes = self
            .authorized(accounts, site_id, |http, hub| Ok(http.get(&hub.url(&path))))
            .await?;
        parse(&bytes)
    }

    pub async fn create(
        &self,
        accounts: &Accounts,
        site_id: &str,
        ticket: NewTicket,
        images: Vec<ImageUpload>,
    ) -> Result<TicketDetail, TicketError> {
        let title = limited(&ticket.title, MAX_TITLE_CHARS)?;
        let body = limited(&ticket.body, MAX_BODY_CHARS)?;
        let contact = limited(&ticket.contact, MAX_CONTACT_CHARS)?;
        if title.is_empty() || body.is_empty() {
            return Err(TicketError::Invalid);
        }
        let limits = self.session(accounts, site_id, None).await?.info.limits;
        let images = prepare_images(images, &limits)?;
        let fields = [
            ("title", title),
            ("category", ticket.category.trim().to_string()),
            ("body", body),
            ("contact", contact),
        ];
        let bytes = self
            .authorized(accounts, site_id, |http, hub| {
                with_fields(http.post(&hub.url("tickets")), &fields, &images)
            })
            .await?;
        parse(&bytes)
    }

    /// 追问。只发图片也可以；已关闭的工单不能再回复（Closed）。
    pub async fn reply(
        &self,
        accounts: &Accounts,
        site_id: &str,
        ticket_id: &str,
        body: &str,
        images: Vec<ImageUpload>,
    ) -> Result<TicketDetail, TicketError> {
        let path = format!("tickets/{}/messages", checked_id(ticket_id)?);
        let body = limited(body, MAX_BODY_CHARS)?;
        if body.is_empty() && images.is_empty() {
            return Err(TicketError::Invalid);
        }
        let limits = self.session(accounts, site_id, None).await?.info.limits;
        let images = prepare_images(images, &limits)?;
        let fields = [("body", body)];
        let bytes = self
            .authorized(accounts, site_id, |http, hub| {
                with_fields(http.post(&hub.url(&path)), &fields, &images)
            })
            .await?;
        parse(&bytes)
    }

    /// 关闭工单（问题解决了）。
    pub async fn close(
        &self,
        accounts: &Accounts,
        site_id: &str,
        ticket_id: &str,
    ) -> Result<TicketDetail, TicketError> {
        let path = format!("tickets/{}/close", checked_id(ticket_id)?);
        let bytes = self
            .authorized(
                accounts,
                site_id,
                |http, hub| Ok(http.post(&hub.url(&path))),
            )
            .await?;
        parse(&bytes)
    }

    /// 消息里的一张图片，转成 `data:` 地址给界面显示。格式按内容核对，只认那几种图片。
    /// `thumb`：要缩略图（TransitHub 1.1.2 起有，几十 KB 的 JPEG；更早的版本不认这个参数，给原图）。
    pub async fn attachment(
        &self,
        accounts: &Accounts,
        site_id: &str,
        attachment_id: &str,
        thumb: bool,
    ) -> Result<String, TicketError> {
        let mut path = format!("tickets/attachments/{}", checked_id(attachment_id)?);
        if thumb {
            path.push_str("?size=thumb");
        }
        let bytes = self
            .authorized(accounts, site_id, |http, hub| {
                Ok(http.get(&hub.url(&path)).timeout(DOWNLOAD_TIMEOUT))
            })
            .await?;
        let mime = sniff_image(&bytes).ok_or(TicketError::InvalidImage)?;
        Ok(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)))
    }

    /// 带工单会话发请求；会话过期（401）时换一个新会话再试一次。
    async fn authorized(
        &self,
        accounts: &Accounts,
        site_id: &str,
        build: impl Fn(&Http, &Hub) -> Result<RequestBuilder, TicketError>,
    ) -> Result<Vec<u8>, TicketError> {
        let session = self.session(accounts, site_id, None).await?;
        let request = build(&self.http, &session.hub)?.bearer_auth(&session.token);
        match self.send(request).await {
            Err(TicketError::SessionExpired) => {
                let renewed = self
                    .session(accounts, site_id, Some(&session.token))
                    .await?;
                let request = build(&self.http, &renewed.hub)?.bearer_auth(&renewed.token);
                self.send(request).await
            }
            result => result,
        }
    }

    /// 取工单会话：内存里有就用；没有，或者 `stale`（被 TransitHub 拒绝的令牌）就是内存里那个时，换一个新的。
    async fn session(
        &self,
        accounts: &Accounts,
        site_id: &str,
        stale: Option<&str>,
    ) -> Result<Session, TicketError> {
        let record = accounts.site(site_id)?;
        let config_url = record.config_url.clone().ok_or(TicketError::NotLinked)?;
        let hub = Hub::parse(&config_url).ok_or(TicketError::NotLinked)?;
        // 和站点不是同一个域名、用户也没确认过的工单服务，拿不到站点的登录（专属链接可能不是站长发的）。
        if !hub_allowed(&record) {
            return Err(TicketError::ConsentRequired(hub_display(&hub.base)));
        }
        if let Some(session) = self.cached(site_id, &config_url, stale) {
            return Ok(session);
        }
        let _guard = self.renew_lock.lock().await;
        // 等锁的时候，别的请求可能已经换好了。
        if let Some(session) = self.cached(site_id, &config_url, stale) {
            return Ok(session);
        }
        self.forget(site_id);
        let session = self.create_session(accounts, site_id, &hub).await?;
        let session = Session {
            config_url,
            hub,
            token: session.session_token,
            info: TicketSession {
                limits: session.session.limits.normalized(),
                ..session.session
            },
        };
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(site_id.to_string(), session.clone());
        }
        Ok(session)
    }

    fn cached(&self, site_id: &str, config_url: &str, stale: Option<&str>) -> Option<Session> {
        let sessions = self.sessions.lock().ok()?;
        let session = sessions.get(site_id)?;
        (session.config_url == config_url && stale != Some(session.token.as_str()))
            .then(|| session.clone())
    }

    /// 用站点的登录换工单会话。sub2api 的访问令牌被站点拒绝时，续期（保存新令牌）后再试一次；
    /// 其他站点只能重新登录。
    async fn create_session(
        &self,
        accounts: &Accounts,
        site_id: &str,
        hub: &Hub,
    ) -> Result<SessionResponse, TicketError> {
        let (record, credential) = accounts.load(site_id)?;
        if self.recently_rejected(site_id, &credential) {
            return Err(TicketError::SiteLoginExpired);
        }
        match self.request_session(hub, &credential).await {
            Err(TicketError::SiteLoginExpired)
                if matches!(credential, Credential::Sub2api { .. }) =>
            {
                let renewed = match accounts.renew(site_id, &record.info, &credential).await {
                    Ok(renewed) => renewed,
                    Err(error) => {
                        // 刷新令牌也失效了才记下来；连不上站点只是这次不行。
                        if matches!(error, SiteError::SessionExpired) {
                            self.remember_rejection(site_id, &credential);
                        }
                        return Err(error.into());
                    }
                };
                let result = self.request_session(hub, &renewed).await;
                if matches!(result, Err(TicketError::SiteLoginExpired)) {
                    self.remember_rejection(site_id, &renewed);
                }
                result
            }
            Err(TicketError::SiteLoginExpired) => {
                self.remember_rejection(site_id, &credential);
                Err(TicketError::SiteLoginExpired)
            }
            result => result,
        }
    }

    fn recently_rejected(&self, site_id: &str, credential: &Credential) -> bool {
        let Ok(rejected) = self.rejected.lock() else {
            return false;
        };
        rejected.get(site_id).is_some_and(|(at, print)| {
            *print == fingerprint(credential) && at.elapsed() < REJECTION_BACKOFF
        })
    }

    fn remember_rejection(&self, site_id: &str, credential: &Credential) {
        if let Ok(mut rejected) = self.rejected.lock() {
            rejected.insert(
                site_id.to_string(),
                (Instant::now(), fingerprint(credential)),
            );
        }
    }

    async fn request_session(
        &self,
        hub: &Hub,
        credential: &Credential,
    ) -> Result<SessionResponse, TicketError> {
        let body = json!({
            "credential": credential_json(credential),
            "clientInfo": self.client_info,
        });
        let bytes = self
            .send(self.http.post(&hub.session_url()).json(&body))
            .await?;
        let response: SessionResponse = parse(&bytes)?;
        if response.session_token.is_empty() {
            return Err(TicketError::Failed(Some("no session token".into())));
        }
        Ok(response)
    }

    /// 发请求，读出响应内容（最多 16 MB）。不成功时按 TransitHub 的错误 key 转成 [`TicketError`]。
    async fn send(&self, request: RequestBuilder) -> Result<Vec<u8>, TicketError> {
        let mut response = request.send().await.map_err(|_| TicketError::Unreachable)?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| TicketError::Unreachable)?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(TicketError::Failed(Some("response too large".into())));
            }
            bytes.extend_from_slice(&chunk);
        }
        if (200..300).contains(&status) {
            return Ok(bytes);
        }
        let key = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|body| text(&body, "message"))
            .unwrap_or_default();
        Err(TicketError::from_response(status, &key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::SiteView;
    use crate::secrets::MemoryStore;
    use crate::siteconfig::{ConfigSite, ConfigTickets, SiteConfig};
    use crate::sites::{Platform, SiteInfo};
    use crate::store::SiteStore;
    use wiremock::matchers::{
        body_json, body_partial_json, header, header_regex, method, path, query_param,
    };
    use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

    const CODE: &str = "abcdefghij";
    /// 最小的 PNG 文件头（够判断格式）。
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";

    /// 一个用专属链接接入的站点：站点本身和 TransitHub 用同一个模拟服务器（同一个主机，工单自动放行）。
    fn linked_site(
        server: &MockServer,
        name: &str,
        platform: Platform,
        credential: Credential,
    ) -> (Accounts, String, std::path::PathBuf) {
        link_site(server, name, platform, credential, &server.uri())
    }

    /// 工单服务和站点不是同一个域名的站点：站点记成 localhost，TransitHub 还在 127.0.0.1，要用户确认。
    fn linked_elsewhere(
        server: &MockServer,
        name: &str,
        platform: Platform,
        credential: Credential,
    ) -> (Accounts, String, std::path::PathBuf) {
        let site_url = server.uri().replace("127.0.0.1", "localhost");
        link_site(server, name, platform, credential, &site_url)
    }

    /// 像后台更新配置那样记下专属链接（不算用户确认）。
    fn link_site(
        server: &MockServer,
        name: &str,
        platform: Platform,
        credential: Credential,
        site_url: &str,
    ) -> (Accounts, String, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("transitlink-tickets-{name}-{}", std::process::id()));
        let accounts = Accounts::new(
            Http::new(),
            SiteStore::new(dir.join("sites.json")),
            Box::new(MemoryStore::default()),
        );
        let info = SiteInfo {
            platform,
            base_url: site_url.to_string(),
            name: "Relay".into(),
            logo_url: None,
            captcha_required: false,
            password_login_disabled: false,
            email_only: platform == Platform::Sub2api,
            quota_per_unit: None,
            password_encryption: false,
            user_agent: None,
        };
        let site = accounts.save(info, "a@example.com", &credential).unwrap();
        let config = SiteConfig {
            site: ConfigSite {
                name: "Relay".into(),
                base_url: site_url.to_string(),
                platform: if platform == Platform::Sub2api {
                    "sub2api".into()
                } else {
                    "newapi".into()
                },
            },
            tickets: ConfigTickets { enabled: true },
            ..Default::default()
        };
        let config_url = format!("{}/api/client/v1/sites/{CODE}", server.uri());
        accounts
            .attach_config(&site.id, &config_url, config)
            .unwrap();
        (accounts, site.id, dir)
    }

    fn session_path() -> String {
        format!("/api/client/v1/sites/{CODE}/tickets/session")
    }

    fn session_request(credential: Value) -> Value {
        json!({ "credential": credential, "clientInfo": client_info() })
    }

    fn session_ok(token: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "sessionToken": token,
            "expiresAt": "2026-11-11T08:00:00Z",
            "user": { "id": "7", "name": "alice", "email": "a@example.com" },
            "categories": ["通用问题", "余额/计费"],
            "limits": { "enabled": true, "maxImages": 5, "maxImageBytes": 5242880 }
        }))
    }

    fn hub_error(status: u16, key: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(json!({ "message": key }))
    }

    fn detail_json(id: &str, status: &str) -> Value {
        json!({
            "id": id, "title": "Claude Code 报 401", "category": "接口调用", "status": status,
            "contact": "", "unread": false,
            "lastMessageAt": "2026-11-04T08:00:00Z", "createdAt": "2026-11-04T07:00:00Z",
            "messages": [{
                "id": "m1", "authorType": "customer", "authorName": "alice", "body": "报错了",
                "createdAt": "2026-11-04T07:00:00Z",
                "attachments": [{ "id": "a1", "originalName": "shot.png", "contentType": "image/png", "sizeBytes": 20, "createdAt": "2026-11-04T07:00:00Z" }]
            }]
        })
    }

    /// 请求体里包含这些字节（multipart 表单里有二进制的图片，不能当字符串比较）。
    struct BodyContains(Vec<Vec<u8>>);

    impl Match for BodyContains {
        fn matches(&self, request: &Request) -> bool {
            self.0.iter().all(|needle| {
                request
                    .body
                    .windows(needle.len())
                    .any(|window| window == needle.as_slice())
            })
        }
    }

    fn png_upload(name: &str) -> ImageUpload {
        ImageUpload {
            name: name.into(),
            data_base64: STANDARD.encode(PNG),
        }
    }

    #[test]
    fn credentials_are_sent_as_bearer_or_cookie() {
        assert_eq!(
            credential_json(&Credential::NewapiToken {
                token: "nap_1".into()
            }),
            json!({ "kind": "bearer", "token": "nap_1" })
        );
        assert_eq!(
            credential_json(&Credential::NewapiLegacy {
                cookie: "session=abc".into(),
                user_id: "7".into()
            }),
            json!({ "kind": "cookie", "cookie": "session=abc", "userId": "7" })
        );
        // sub2api 只发访问令牌，刷新令牌不离开本机。
        assert_eq!(
            credential_json(&Credential::Sub2api {
                access_token: "at".into(),
                refresh_token: "rt".into()
            }),
            json!({ "kind": "bearer", "token": "at" })
        );
    }

    #[test]
    fn hub_addresses_come_from_the_config_url() {
        let hub = Hub::parse("https://hub.example.com/api/client/v1/sites/abcdefghij").unwrap();
        assert_eq!(hub.base, "https://hub.example.com");
        assert_eq!(
            hub.session_url(),
            "https://hub.example.com/api/client/v1/sites/abcdefghij/tickets/session"
        );
        assert_eq!(
            hub.url("tickets/t1"),
            "https://hub.example.com/api/client/v1/tickets/t1"
        );
        // 部署在子路径下的 TransitHub。
        let hub = Hub::parse("https://example.com/hub/api/client/v1/sites/abc/").unwrap();
        assert_eq!(hub.base, "https://example.com/hub");
        assert_eq!(hub.code, "abc");
        assert!(Hub::parse("http://127.0.0.1:5555/api/client/v1/sites/abc").is_some());
        // 凭证只发往 https。
        assert_eq!(
            Hub::parse("http://hub.example.com/api/client/v1/sites/abc"),
            None
        );
        assert_eq!(Hub::parse("https://hub.example.com/config/abc"), None);
        assert_eq!(
            Hub::parse("https://hub.example.com/api/client/v1/sites/"),
            None
        );
        assert_eq!(checked_id("../config").unwrap_err(), TicketError::NotFound);
        assert_eq!(checked_id(" 0a1b-2c_3 ").unwrap(), "0a1b-2c_3");
    }

    #[test]
    fn images_are_checked_before_sending() {
        let limits = UploadLimits {
            enabled: true,
            max_images: 2,
            max_image_bytes: 64,
        };
        let images = prepare_images(
            vec![
                png_upload("C:\\Users\\a\\截图 1.png"),
                ImageUpload {
                    name: "".into(),
                    data_base64: format!("data:image/png;base64,{}", STANDARD.encode(PNG)),
                },
            ],
            &limits,
        )
        .unwrap();
        assert_eq!(images[0].name, "截图 1.png");
        assert_eq!(images[0].mime, "image/png");
        assert_eq!(images[1].name, "image.png");
        assert_eq!(images[1].data, PNG);

        let three = vec![
            png_upload("a.png"),
            png_upload("b.png"),
            png_upload("c.png"),
        ];
        assert!(matches!(
            prepare_images(three, &limits),
            Err(TicketError::TooManyImages)
        ));
        let big = ImageUpload {
            name: "big.png".into(),
            data_base64: STANDARD.encode([PNG.to_vec(), vec![0u8; 64]].concat()),
        };
        assert!(matches!(
            prepare_images(vec![big], &limits),
            Err(TicketError::ImageTooLarge)
        ));
        let svg = ImageUpload {
            name: "x.svg".into(),
            data_base64: STANDARD.encode("<svg/>"),
        };
        assert!(matches!(
            prepare_images(vec![svg], &limits),
            Err(TicketError::InvalidImage)
        ));
        let garbled = ImageUpload {
            name: "x.png".into(),
            data_base64: "not base64!".into(),
        };
        assert!(matches!(
            prepare_images(vec![garbled], &limits),
            Err(TicketError::InvalidImage)
        ));
        let off = UploadLimits {
            enabled: false,
            ..limits.clone()
        };
        assert!(matches!(
            prepare_images(vec![png_upload("a.png")], &off),
            Err(TicketError::UploadsDisabled)
        ));
        assert!(prepare_images(Vec::new(), &off).unwrap().is_empty());
        // TransitHub 给的限制不会超过 5 张、5 MB。
        let generous = UploadLimits {
            enabled: true,
            max_images: 50,
            max_image_bytes: 0,
        }
        .normalized();
        assert_eq!(
            (generous.max_images, generous.max_image_bytes),
            (5, MAX_IMAGE_BYTES)
        );
    }

    #[test]
    fn hub_errors_map_to_codes() {
        assert_eq!(
            TicketError::from_response(403, "tickets.errors.disabled"),
            TicketError::Disabled
        );
        assert_eq!(
            TicketError::from_response(409, "tickets.errors.closed").code(),
            "ticketsClosed"
        );
        assert_eq!(
            TicketError::from_response(413, ""),
            TicketError::UploadTooLarge
        );
        let failed = TicketError::from_response(500, "tickets.errors.request");
        assert_eq!(failed.code(), "ticketsFailed");
        assert_eq!(failed.message().as_deref(), Some("HTTP 500"));
        assert_eq!(
            TicketError::from(SiteError::SessionExpired),
            TicketError::SiteLoginExpired
        );
        assert_eq!(
            TicketError::from(SiteError::Unreachable).code(),
            "unreachable"
        );
    }

    #[tokio::test]
    async fn sessions_are_cached_and_renewed_once_when_expired() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "cache",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_1".into(),
            },
        );
        // 每次换会话都带同样的凭证和客户端信息，依次换到 s1、s2、s3。
        let request = session_request(json!({ "kind": "bearer", "token": "nap_1" }));
        for token in ["s1", "s2", "s3"] {
            Mock::given(method("POST"))
                .and(path(session_path()))
                .and(body_json(&request))
                .respond_with(session_ok(token))
                .up_to_n_times(1)
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets"))
            .and(header("Authorization", "Bearer s1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{ "id": "t1", "title": "Claude Code 报 401", "category": "接口调用", "status": "replied", "contact": "", "unread": true, "lastMessageAt": "2026-11-04T08:00:00Z", "createdAt": "2026-11-04T07:00:00Z" }],
                "unread": 1
            })))
            .expect(2)
            .mount(&server)
            .await;

        let tickets = Tickets::new(Http::new());
        // 列表查两次、再取会话信息：只换一次会话。
        let list = tickets.list(&accounts, &site).await.unwrap();
        assert_eq!(list.unread, 1);
        assert_eq!(list.items[0].status, "replied");
        assert!(list.items[0].unread);
        tickets.list(&accounts, &site).await.unwrap();
        let info = tickets.session_info(&accounts, &site).await.unwrap();
        assert_eq!(info.categories, vec!["通用问题", "余额/计费"]);
        assert_eq!(info.user.name, "alice");
        assert_eq!(info.limits.max_images, 5);

        // 会话过期（401）：换一个新会话再试一次。
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/t1"))
            .and(header("Authorization", "Bearer s1"))
            .respond_with(hub_error(401, "tickets.errors.sessionExpired"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/t1"))
            .and(header("Authorization", "Bearer s2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_json("t1", "replied")))
            .expect(1)
            .mount(&server)
            .await;
        let detail = tickets.detail(&accounts, &site, "t1").await.unwrap();
        assert_eq!(detail.ticket.title, "Claude Code 报 401");
        assert_eq!(detail.messages[0].attachments[0].original_name, "shot.png");

        // 新会话也被拒绝：只重试一次，不会一直换下去。
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/t2"))
            .respond_with(hub_error(401, "tickets.errors.sessionExpired"))
            .expect(2)
            .mount(&server)
            .await;
        assert_eq!(
            tickets.detail(&accounts, &site, "t2").await.unwrap_err(),
            TicketError::SessionExpired
        );
        // 不合法的 ID 不发请求。
        assert_eq!(
            tickets.detail(&accounts, &site, "../t1").await.unwrap_err(),
            TicketError::NotFound
        );
        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn sub2api_logins_are_renewed_once_when_the_site_rejects_them() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "sub2api",
            Platform::Sub2api,
            Credential::Sub2api {
                access_token: "old".into(),
                refresh_token: "rt-1".into(),
            },
        );
        Mock::given(method("POST"))
            .and(path(session_path()))
            .and(body_json(session_request(
                json!({ "kind": "bearer", "token": "old" }),
            )))
            .respond_with(hub_error(403, "tickets.errors.siteLoginExpired"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/refresh"))
            .and(body_partial_json(json!({ "refresh_token": "rt-1" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "message": "success",
                "data": { "access_token": "new", "refresh_token": "rt-2" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(session_path()))
            .and(body_json(session_request(
                json!({ "kind": "bearer", "token": "new" }),
            )))
            .respond_with(session_ok("s1"))
            .expect(1)
            .mount(&server)
            .await;

        let tickets = Tickets::new(Http::new());
        let info = tickets.session_info(&accounts, &site).await.unwrap();
        assert_eq!(info.user.email, "a@example.com");
        // 续期得到的新令牌存回钥匙串。
        let (_, credential) = accounts.load(&site).unwrap();
        assert_eq!(
            credential,
            Credential::Sub2api {
                access_token: "new".into(),
                refresh_token: "rt-2".into()
            }
        );
        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn other_rejected_logins_need_signing_in_again() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "legacy",
            Platform::Newapi,
            Credential::NewapiLegacy {
                cookie: "session=abc".into(),
                user_id: "7".into(),
            },
        );
        // 旧版 new-api 发会话 cookie；站点不认时不重试，提示重新登录。
        Mock::given(method("POST"))
            .and(path(session_path()))
            .and(body_json(session_request(
                json!({ "kind": "cookie", "cookie": "session=abc", "userId": "7" }),
            )))
            .respond_with(hub_error(403, "tickets.errors.siteLoginExpired"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let tickets = Tickets::new(Http::new());
        let error = tickets.list(&accounts, &site).await.unwrap_err();
        assert_eq!(error, TicketError::SiteLoginExpired);
        assert_eq!(error.code(), "ticketsSiteLoginExpired");
        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn creates_tickets_with_images_as_multipart() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "create",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_1".into(),
            },
        );
        Mock::given(method("POST"))
            .and(path(session_path()))
            .respond_with(session_ok("s1"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/client/v1/tickets"))
            .and(header("Authorization", "Bearer s1"))
            .and(header_regex(
                "Content-Type",
                "^multipart/form-data; boundary=",
            ))
            .and(BodyContains(vec![
                "name=\"title\"\r\n\r\nClaude Code 报 401\r\n"
                    .as_bytes()
                    .to_vec(),
                "name=\"category\"\r\n\r\n接口调用\r\n".as_bytes().to_vec(),
                "name=\"body\"\r\n\r\n报错了\r\n".as_bytes().to_vec(),
                b"name=\"contact\"\r\n\r\ntg: @alice\r\n".to_vec(),
                b"name=\"images\"; filename=\"shot.png\"\r\nContent-Type: image/png\r\n\r\n"
                    .to_vec(),
                PNG.to_vec(),
            ]))
            .respond_with(ResponseTemplate::new(201).set_body_json(detail_json("t9", "open")))
            .expect(1)
            .mount(&server)
            .await;
        // 没有图片时发 JSON。
        Mock::given(method("POST"))
            .and(path("/api/client/v1/tickets"))
            .and(header("Authorization", "Bearer s1"))
            .and(body_json(json!({ "title": "充值没到账", "category": "余额/计费", "body": "昨天充的", "contact": "" })))
            .respond_with(ResponseTemplate::new(201).set_body_json(detail_json("t10", "open")))
            .expect(1)
            .mount(&server)
            .await;

        let tickets = Tickets::new(Http::new());
        let ticket = NewTicket {
            title: " Claude Code 报 401 ".into(),
            category: "接口调用".into(),
            body: "报错了".into(),
            contact: "tg: @alice".into(),
        };
        let detail = tickets
            .create(&accounts, &site, ticket, vec![png_upload("/tmp/shot.png")])
            .await
            .unwrap();
        assert_eq!(detail.ticket.id, "t9");
        assert_eq!(detail.ticket.status, "open");

        let ticket = NewTicket {
            title: "充值没到账".into(),
            category: "余额/计费".into(),
            body: "昨天充的".into(),
            contact: String::new(),
        };
        let detail = tickets
            .create(&accounts, &site, ticket, Vec::new())
            .await
            .unwrap();
        assert_eq!(detail.ticket.id, "t10");

        // 标题太长、没有内容、图片太多：不发请求。
        let too_long = NewTicket {
            title: "长".repeat(101),
            body: "x".into(),
            ..Default::default()
        };
        assert_eq!(
            tickets
                .create(&accounts, &site, too_long, Vec::new())
                .await
                .unwrap_err(),
            TicketError::Invalid
        );
        let empty = NewTicket {
            title: "t".into(),
            body: "  ".into(),
            ..Default::default()
        };
        assert_eq!(
            tickets
                .create(&accounts, &site, empty, Vec::new())
                .await
                .unwrap_err(),
            TicketError::Invalid
        );
        let six = (0..6).map(|i| png_upload(&format!("{i}.png"))).collect();
        let ticket = NewTicket {
            title: "t".into(),
            body: "b".into(),
            ..Default::default()
        };
        assert_eq!(
            tickets
                .create(&accounts, &site, ticket, six)
                .await
                .unwrap_err(),
            TicketError::TooManyImages
        );
        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn replies_closes_and_downloads_images() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "reply",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_1".into(),
            },
        );
        Mock::given(method("POST"))
            .and(path(session_path()))
            .respond_with(session_ok("s1"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/client/v1/tickets/t1/messages"))
            .and(body_json(json!({ "body": "还是不行" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_json("t1", "pending")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/client/v1/tickets/t2/messages"))
            .respond_with(hub_error(409, "tickets.errors.closed"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/client/v1/tickets/t1/close"))
            .and(header("Authorization", "Bearer s1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_json("t1", "closed")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/missing"))
            .respond_with(hub_error(404, "tickets.errors.notFound"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/attachments/a1"))
            .and(header("Authorization", "Bearer s1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "image/png")
                    .set_body_bytes(PNG),
            )
            .mount(&server)
            .await;
        // 缩略图：同一张图片带 ?size=thumb，TransitHub 给小的 JPEG。
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/attachments/a1"))
            .and(query_param("size", "thumb"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "image/jpeg")
                    .set_body_bytes(&b"\xFF\xD8\xFF\xE0thumb"[..]),
            )
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets/attachments/a2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "image/svg+xml")
                    .set_body_string("<svg/>"),
            )
            .mount(&server)
            .await;

        let tickets = Tickets::new(Http::new());
        let detail = tickets
            .reply(&accounts, &site, "t1", " 还是不行 ", Vec::new())
            .await
            .unwrap();
        assert_eq!(detail.ticket.status, "pending");
        assert_eq!(
            tickets
                .reply(&accounts, &site, "t2", "在吗", Vec::new())
                .await
                .unwrap_err(),
            TicketError::Closed
        );
        assert_eq!(
            tickets
                .reply(&accounts, &site, "t1", " ", Vec::new())
                .await
                .unwrap_err(),
            TicketError::Invalid,
            "a reply needs text or images"
        );
        let closed = tickets.close(&accounts, &site, "t1").await.unwrap();
        assert_eq!(closed.ticket.status, "closed");
        assert_eq!(
            tickets
                .detail(&accounts, &site, "missing")
                .await
                .unwrap_err(),
            TicketError::NotFound
        );
        assert_eq!(
            tickets
                .attachment(&accounts, &site, "a1", false)
                .await
                .unwrap(),
            format!("data:image/png;base64,{}", STANDARD.encode(PNG))
        );
        assert_eq!(
            tickets
                .attachment(&accounts, &site, "a1", true)
                .await
                .unwrap(),
            format!(
                "data:image/jpeg;base64,{}",
                STANDARD.encode(b"\xFF\xD8\xFF\xE0thumb")
            )
        );
        // 只认那几种图片，别的内容不交给界面。
        assert_eq!(
            tickets
                .attachment(&accounts, &site, "a2", false)
                .await
                .unwrap_err(),
            TicketError::InvalidImage
        );
        drop(server);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn sites_without_a_link_have_no_tickets() {
        let dir = std::env::temp_dir().join(format!(
            "transitlink-tickets-unlinked-{}",
            std::process::id()
        ));
        let accounts = Accounts::new(
            Http::new(),
            SiteStore::new(dir.join("sites.json")),
            Box::new(MemoryStore::default()),
        );
        let info = SiteInfo {
            platform: Platform::Newapi,
            base_url: "https://relay.example.com".into(),
            name: "Relay".into(),
            logo_url: None,
            captcha_required: false,
            password_login_disabled: false,
            email_only: false,
            quota_per_unit: None,
            password_encryption: false,
            user_agent: None,
        };
        let site = accounts
            .save(
                info,
                "alice",
                &Credential::NewapiToken {
                    token: "nap_1".into(),
                },
            )
            .unwrap();
        let tickets = Tickets::new(Http::new());
        assert_eq!(
            tickets.list(&accounts, &site.id).await.unwrap_err(),
            TicketError::NotLinked
        );
        // 站点删掉了：要重新接入、登录。
        assert_eq!(
            tickets.list(&accounts, "gone").await.unwrap_err(),
            TicketError::SiteLoginExpired
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn site_logins_stay_home_until_the_user_allows_a_hub_on_another_domain() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_elsewhere(
            &server,
            "consent",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_secret".into(),
            },
        );
        let host = server.uri().trim_start_matches("http://").to_string();
        let tickets = Tickets::new(Http::new());

        // 没同意：不换会话，站点的登录一个字节也不发出去。
        let guard = Mock::given(method("POST"))
            .and(path(session_path()))
            .respond_with(session_ok("never"))
            .expect(0)
            .mount_as_scoped(&server)
            .await;
        assert_eq!(
            tickets.list(&accounts, &site).await.unwrap_err(),
            TicketError::ConsentRequired(host.clone())
        );
        let view = SiteView::from(&accounts.site(&site).unwrap());
        assert!(!view.tickets_allowed);
        assert_eq!(view.tickets_host.as_deref(), Some(host.as_str()));
        drop(guard);

        // 同意以后才换会话。
        Mock::given(method("POST"))
            .and(path(session_path()))
            .respond_with(session_ok("sess-1"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "items": [], "unread": 0 })),
            )
            .mount(&server)
            .await;
        assert!(accounts.allow_ticket_hub(&site).unwrap().tickets_allowed);
        assert_eq!(tickets.list(&accounts, &site).await.unwrap().unread, 0);

        // 专属链接换成了别的 TransitHub 地址：要重新同意，不能沿用。
        let config = accounts.site(&site).unwrap().config.unwrap();
        let moved = format!("{}/elsewhere/api/client/v1/sites/{CODE}", server.uri());
        accounts.attach_config(&site, &moved, config).unwrap();
        assert!(!SiteView::from(&accounts.site(&site).unwrap()).tickets_allowed);
        assert_eq!(
            tickets.list(&accounts, &site).await.unwrap_err(),
            TicketError::ConsentRequired(format!("{host}/elsewhere"))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rejected_logins_are_not_retried_for_a_while() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "backoff",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_old".into(),
            },
        );
        // 站点不认这个令牌：只问一次，之后（后台检查未读时）不再拿同一个令牌去试。
        Mock::given(method("POST"))
            .and(path(session_path()))
            .and(body_json(session_request(
                json!({ "kind": "bearer", "token": "nap_old" }),
            )))
            .respond_with(hub_error(403, "tickets.errors.siteLoginExpired"))
            .expect(1)
            .mount(&server)
            .await;
        let tickets = Tickets::new(Http::new());
        for _ in 0..3 {
            assert_eq!(
                tickets.list(&accounts, &site).await.unwrap_err(),
                TicketError::SiteLoginExpired
            );
        }

        // 重新登录（换了令牌）以后马上可以再试；重新登录沿用原来的记录，同意过的也还在。
        let info = accounts.site(&site).unwrap().info;
        let again = accounts
            .save(
                info,
                "a@example.com",
                &Credential::NewapiToken {
                    token: "nap_new".into(),
                },
            )
            .unwrap();
        assert_eq!(again.id, site);
        assert!(again.tickets_allowed);
        Mock::given(method("POST"))
            .and(path(session_path()))
            .and(body_json(session_request(
                json!({ "kind": "bearer", "token": "nap_new" }),
            )))
            .respond_with(session_ok("sess-2"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "items": [], "unread": 0 })),
            )
            .mount(&server)
            .await;
        assert!(tickets.list(&accounts, &site).await.is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hubs_on_the_site_domain_belong_to_the_same_owner() {
        assert!(same_owner(
            "https://api.example.com",
            "https://hub.example.com/transithub"
        ));
        assert!(same_owner("https://example.com", "https://hub.example.com"));
        assert!(same_owner(
            "https://API.example.com",
            "https://hub.Example.com"
        ));
        assert!(same_owner(
            "https://api.example.com.cn",
            "https://hub.example.com.cn"
        ));
        assert!(same_owner("http://1.2.3.4:3000", "http://1.2.3.4:8080"));
        assert!(same_owner("http://localhost:3000", "http://localhost:5555"));
        assert!(!same_owner(
            "https://api.example.com",
            "https://hub.other.com"
        ));
        assert!(!same_owner(
            "https://a.example.com.cn",
            "https://b.other.com.cn"
        ));
        // 共享托管的域名：同一个后缀下的不同用户不是同一个站长。
        assert!(!same_owner(
            "https://relay.github.io",
            "https://evil.github.io"
        ));
        assert!(!same_owner("http://1.2.3.4", "http://1.2.3.5"));
        assert!(!same_owner(
            "http://localhost:3000",
            "http://127.0.0.1:3000"
        ));
        assert!(!same_owner("https://api.example.com", "not a url"));
    }

    #[tokio::test]
    async fn hubs_on_the_site_domain_are_trusted_without_asking() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_site(
            &server,
            "same-owner",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_secret".into(),
            },
        );
        assert!(SiteView::from(&accounts.site(&site).unwrap()).tickets_allowed);
        Mock::given(method("POST"))
            .and(path(session_path()))
            .respond_with(session_ok("sess-1"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/tickets"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "items": [], "unread": 0 })),
            )
            .mount(&server)
            .await;
        let tickets = Tickets::new(Http::new());
        assert_eq!(tickets.list(&accounts, &site).await.unwrap().unread, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn accepting_a_link_counts_as_consent_but_refreshing_it_does_not() {
        let server = MockServer::start().await;
        let (accounts, site, dir) = linked_elsewhere(
            &server,
            "accept",
            Platform::Newapi,
            Credential::NewapiToken {
                token: "nap_secret".into(),
            },
        );
        let record = accounts.site(&site).unwrap();
        let config_url = record.config_url.clone().unwrap();
        let config = record.config.clone().unwrap();
        assert!(!SiteView::from(&record).tickets_allowed);

        // 后台定时更新配置：不算确认。
        accounts
            .attach_config(&site, &config_url, config.clone())
            .unwrap();
        assert!(!SiteView::from(&accounts.site(&site).unwrap()).tickets_allowed);

        // 用户在接入窗口里接受了这个链接（不同域名时窗口里提醒过）：算确认。
        assert!(
            accounts
                .accept_link(&site, &config_url, config.clone())
                .unwrap()
                .tickets_allowed
        );
        // 之后后台更新同一个链接，确认还在。
        accounts.attach_config(&site, &config_url, config).unwrap();
        assert!(SiteView::from(&accounts.site(&site).unwrap()).tickets_allowed);
        let _ = std::fs::remove_dir_all(dir);
    }
}
