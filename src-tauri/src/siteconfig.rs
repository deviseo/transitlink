//! 站长在 TransitHub 里设置的站点配置：品牌、推荐的分组和模型、公告、客服。
//! 用户点专属链接 `transitlink://connect?config=<配置地址>` 接入时读取，之后定期刷新。
//! 配置地址是 TransitHub 的公开接口 `…/api/client/v1/sites/<代码>`。

use serde::{Deserialize, Serialize};

use crate::sites::http::Http;
use crate::sites::{SiteError, normalize_url};

/// 配置最大 1 MB（正常只有几 KB）。
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SiteConfig {
    pub site: ConfigSite,
    pub brand: ConfigBrand,
    pub recommend: ConfigRecommend,
    pub notice: ConfigNotice,
    pub support: Vec<ConfigSupport>,
    pub client: ConfigClient,
    /// 工单开关（以前保存的配置里没有这一项，读出来是关着的）。
    pub tickets: ConfigTickets,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigSite {
    pub name: String,
    pub base_url: String,
    pub platform: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigBrand {
    pub name: String,
    /// 绝对地址（https 或 data:）；没有时为空。
    pub logo_url: String,
    /// `#rrggbb`；不合法时为空。
    pub color: String,
    pub welcome: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolRecommend {
    /// new-api 是分组名，sub2api 是分组 ID；空表示站长没指定。
    pub group: String,
    /// 空表示没指定。
    pub model: String,
}

/// 各工具的推荐设置。TransitHub 1.2 起才有 Gemini CLI 到 DeepSeek Harness，1.3 起才有 Pi 到 OpenClaw；
/// 旧版本的配置里没有（按没指定处理）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigRecommend {
    pub claude_code: ToolRecommend,
    pub codex: ToolRecommend,
    pub gemini_cli: ToolRecommend,
    pub grok_build: ToolRecommend,
    pub opencode: ToolRecommend,
    pub deepseek_harness: ToolRecommend,
    pub pi: ToolRecommend,
    pub oh_my_pi: ToolRecommend,
    pub kimi_code: ToolRecommend,
    pub minimax_code: ToolRecommend,
    pub openclaw: ToolRecommend,
}

static NO_RECOMMEND: ToolRecommend = ToolRecommend {
    group: String::new(),
    model: String::new(),
};

impl ConfigRecommend {
    /// 某个工具的推荐设置。
    pub fn tool(&self, kind: crate::tools::ToolKind) -> &ToolRecommend {
        use crate::tools::ToolKind;
        match kind {
            ToolKind::ClaudeCode => &self.claude_code,
            ToolKind::Codex => &self.codex,
            ToolKind::GeminiCli => &self.gemini_cli,
            ToolKind::GrokBuild => &self.grok_build,
            ToolKind::OpenCode => &self.opencode,
            ToolKind::DeepseekHarness => &self.deepseek_harness,
            ToolKind::Pi => &self.pi,
            ToolKind::OhMyPi => &self.oh_my_pi,
            ToolKind::KimiCode => &self.kimi_code,
            ToolKind::MinimaxCode => &self.minimax_code,
            ToolKind::OpenClaw => &self.openclaw,
            // 没法一键配置，没有推荐设置。
            ToolKind::Antigravity => &NO_RECOMMEND,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigNotice {
    pub content: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigSupport {
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub value: String,
}

/// 站长对客户端本身的要求。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigClient {
    /// 最低版本（例如 0.2.0），低于它时提示用户更新；空表示不限制。
    pub min_version: String,
}

/// 站长有没有开工单。问题分类等细节在换取工单会话时才给。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigTickets {
    pub enabled: bool,
}

/// 是不是 X.Y.Z 形式的版本号。
fn is_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 4 && part.chars().all(|c| c.is_ascii_digit())
        })
}

/// 从用户给的内容里取出配置地址：专属链接 `transitlink://connect?config=…`，或者直接是配置地址。
pub fn config_url_from(input: &str) -> Option<String> {
    let input = input.trim();
    let url = url::Url::parse(input).ok()?;
    let candidate = if url.scheme() == "transitlink" {
        if url.host_str() != Some("connect") {
            return None;
        }
        url.query_pairs()
            .find(|(key, _)| key == "config")
            .map(|(_, value)| value.into_owned())?
    } else {
        input.to_string()
    };
    let config = url::Url::parse(&candidate).ok()?;
    allowed(&config).then(|| config.to_string())
}

/// 配置地址只接受 https；本机测试（localhost、127.0.0.1）可以用 http。
pub(crate) fn allowed(url: &url::Url) -> bool {
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}

/// 读取并校验配置。链接失效（TransitHub 返回 404）时返回 ConfigNotFound。
pub async fn fetch(http: &Http, config_url: &str) -> Result<SiteConfig, SiteError> {
    let url = url::Url::parse(config_url).map_err(|_| SiteError::InvalidUrl)?;
    if !allowed(&url) {
        return Err(SiteError::InvalidUrl);
    }
    let response = http
        .get(url.as_str())
        .send()
        .await
        .map_err(|_| SiteError::Unreachable)?;
    let status = response.status().as_u16();
    if status == 404 {
        return Err(SiteError::LinkNotFound);
    }
    let bytes = response.bytes().await.map_err(|_| SiteError::Unreachable)?;
    if status >= 400 || bytes.len() > MAX_CONFIG_BYTES {
        return Err(SiteError::LinkInvalid);
    }
    let config: SiteConfig = serde_json::from_slice(&bytes).map_err(|_| SiteError::LinkInvalid)?;
    sanitize(config, &url)
}

/// 规范化地址、限制长度、去掉不合法的颜色和图片地址。站点地址和平台不对时整份配置不用。
/// 从备份文件导入的配置也走这一遍。
pub(crate) fn sanitize(
    mut config: SiteConfig,
    config_url: &url::Url,
) -> Result<SiteConfig, SiteError> {
    config.site.base_url =
        normalize_url(&config.site.base_url).map_err(|_| SiteError::LinkInvalid)?;
    if !matches!(config.site.platform.as_str(), "newapi" | "sub2api") {
        return Err(SiteError::LinkInvalid);
    }
    let clip = |value: &mut String, limit: usize| {
        *value = value.trim().chars().take(limit).collect();
    };
    clip(&mut config.site.name, 60);
    clip(&mut config.brand.name, 40);
    clip(&mut config.brand.welcome, 200);
    clip(&mut config.notice.content, 2000);
    for tool in [
        &mut config.recommend.claude_code,
        &mut config.recommend.codex,
        &mut config.recommend.gemini_cli,
        &mut config.recommend.grok_build,
        &mut config.recommend.opencode,
        &mut config.recommend.deepseek_harness,
        &mut config.recommend.pi,
        &mut config.recommend.oh_my_pi,
        &mut config.recommend.kimi_code,
        &mut config.recommend.minimax_code,
        &mut config.recommend.openclaw,
    ] {
        clip(&mut tool.group, 100);
        clip(&mut tool.model, 100);
    }
    let color = config.brand.color.trim().to_ascii_lowercase();
    config.brand.color = if color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        color
    } else {
        String::new()
    };
    // logo 可以是相对地址（TransitHub 上传的图片），按配置地址解析；最后只留 https。
    let logo = config.brand.logo_url.trim();
    config.brand.logo_url = if logo.is_empty() {
        String::new()
    } else {
        config_url
            .join(logo)
            .ok()
            .filter(|url| url.scheme() == "https" || allowed(url))
            .map(|url| url.to_string())
            .unwrap_or_default()
    };
    let version = config
        .client
        .min_version
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    config.client.min_version = if is_version(&version) {
        version
    } else {
        String::new()
    };
    config.support.retain(|item| !item.value.trim().is_empty());
    config.support.truncate(8);
    for item in &mut config.support {
        clip(&mut item.kind, 20);
        clip(&mut item.label, 30);
        clip(&mut item.value, 300);
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn reads_config_urls_from_links() {
        let config = "https://hub.example.com/api/client/v1/sites/abcdefghij";
        let link = format!(
            "transitlink://connect?config={}",
            url::form_urlencoded::byte_serialize(config.as_bytes()).collect::<String>()
        );
        assert_eq!(config_url_from(&link).as_deref(), Some(config));
        assert_eq!(
            config_url_from(&format!("  {config} ")).as_deref(),
            Some(config)
        );
        assert_eq!(
            config_url_from("http://127.0.0.1:5555/api/client/v1/sites/abcdefghij").as_deref(),
            Some("http://127.0.0.1:5555/api/client/v1/sites/abcdefghij")
        );
        // 只接受 https（本机测试除外）和 connect 链接。
        assert_eq!(
            config_url_from("http://hub.example.com/api/client/v1/sites/x"),
            None
        );
        assert_eq!(
            config_url_from("transitlink://other?config=https%3A%2F%2Fa.example.com"),
            None
        );
        assert_eq!(
            config_url_from("transitlink://connect?config=javascript%3Aalert(1)"),
            None
        );
        assert_eq!(config_url_from("relay.example.com"), None);
    }

    #[tokio::test]
    async fn fetches_and_sanitizes_configs() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/sites/abcdefghij"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "version": 1,
                "site": { "name": "示例中转", "baseUrl": "https://relay.example.com/", "platform": "newapi" },
                "brand": { "name": "极速中转", "logoUrl": "/api/client/v1/sites/abcdefghij/logo", "color": "#2563EB", "welcome": "欢迎" },
                "recommend": {
                    "claudeCode": { "group": "claude", "model": "" },
                    "codex": { "group": "codex", "model": "gpt-6.1-sol" },
                    "deepseekHarness": { "group": " deepseek ", "model": "deepseek-v4" },
                    "ohMyPi": { "group": "claude", "model": "claude-sonnet-5-5" },
                    "kimiCode": { "group": "kimi", "model": " kimi-k2.6 " }
                },
                "notice": { "content": "周六维护", "updatedAt": "2026-11-04T08:00:00Z" },
                "support": [{ "type": "telegram", "label": "TG", "value": "https://t.me/x" }, { "type": "qq", "label": "空", "value": " " }],
                "client": { "minVersion": "v0.2.0" },
                "tickets": { "enabled": true },
                "future": { "ignored": true }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/sites/gone"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(json!({ "message": "client.errors.notFound" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/client/v1/sites/bad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({ "site": { "baseUrl": "https://x.example.com", "platform": "other" } }),
            ))
            .mount(&server)
            .await;

        let http = Http::new();
        let base = server.uri();
        let config = fetch(&http, &format!("{base}/api/client/v1/sites/abcdefghij"))
            .await
            .unwrap();
        assert_eq!(config.site.base_url, "https://relay.example.com");
        assert_eq!(config.brand.color, "#2563eb");
        assert_eq!(
            config.brand.logo_url,
            format!("{base}/api/client/v1/sites/abcdefghij/logo"),
            "relative logos resolve against the config address"
        );
        assert_eq!(config.recommend.codex.model, "gpt-6.1-sol");
        let dsh = config
            .recommend
            .tool(crate::tools::ToolKind::DeepseekHarness);
        assert_eq!(
            (dsh.group.as_str(), dsh.model.as_str()),
            ("deepseek", "deepseek-v4")
        );
        let kimi = config.recommend.tool(crate::tools::ToolKind::KimiCode);
        assert_eq!(
            (kimi.group.as_str(), kimi.model.as_str()),
            ("kimi", "kimi-k2.6")
        );
        assert_eq!(
            config.recommend.tool(crate::tools::ToolKind::OhMyPi).model,
            "claude-sonnet-5-5"
        );
        // 旧版 TransitHub 没有的工具：当作没指定。
        assert_eq!(
            config.recommend.tool(crate::tools::ToolKind::GrokBuild),
            &ToolRecommend::default()
        );
        assert_eq!(config.support.len(), 1);
        assert_eq!(config.client.min_version, "0.2.0");
        assert!(config.tickets.enabled);
        assert_eq!(
            fetch(&http, &format!("{base}/api/client/v1/sites/gone"))
                .await
                .unwrap_err(),
            SiteError::LinkNotFound
        );
        assert_eq!(
            fetch(&http, &format!("{base}/api/client/v1/sites/bad"))
                .await
                .unwrap_err(),
            SiteError::LinkInvalid
        );
        assert_eq!(
            fetch(&http, "ftp://hub.example.com/config")
                .await
                .unwrap_err(),
            SiteError::InvalidUrl
        );
    }

    #[test]
    fn drops_unsafe_logos_and_colors() {
        let config_url =
            url::Url::parse("https://hub.example.com/api/client/v1/sites/abcdefghij").unwrap();
        let config = SiteConfig {
            site: ConfigSite {
                name: "S".into(),
                base_url: "relay.example.com".into(),
                platform: "sub2api".into(),
            },
            brand: ConfigBrand {
                logo_url: "javascript:alert(1)".into(),
                color: "red".into(),
                ..ConfigBrand::default()
            },
            client: ConfigClient {
                min_version: "latest".into(),
            },
            ..SiteConfig::default()
        };
        let clean = sanitize(config, &config_url).unwrap();
        assert_eq!(clean.site.base_url, "https://relay.example.com");
        assert_eq!(clean.brand.logo_url, "");
        assert_eq!(clean.brand.color, "");
        assert_eq!(clean.client.min_version, "", "only X.Y.Z versions are kept");
    }

    #[test]
    fn configs_saved_before_tickets_still_load() {
        // 旧版本存下的配置没有 tickets：照样能读，工单是关着的。
        let saved = json!({
            "site": { "name": "S", "baseUrl": "https://relay.example.com", "platform": "newapi" },
            "brand": { "name": "B", "logoUrl": "", "color": "", "welcome": "" },
            "client": { "minVersion": "" }
        });
        let config: SiteConfig = serde_json::from_value(saved).unwrap();
        assert!(!config.tickets.enabled);
        assert_eq!(config.brand.name, "B");
    }
}
