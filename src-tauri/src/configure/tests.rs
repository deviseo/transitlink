//! 一键配置的端到端测试：模拟一个 sub2api 站点，工具配置写到临时目录。

use std::path::PathBuf;

use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::secrets::MemoryStore;
use crate::sites::{Credential, SiteInfo};
use crate::store::SiteStore;

fn ok(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({ "code": 0, "message": "success", "data": data }))
}

fn models(ids: &[&str]) -> ResponseTemplate {
    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "id": id, "object": "model" }))
        .collect();
    ResponseTemplate::new(200).set_body_json(json!({ "object": "list", "data": data }))
}

struct Fixture {
    dir: PathBuf,
    home: PathBuf,
    accounts: Accounts,
    configurator: Configurator,
    site_id: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(server: &MockServer, name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "transitlink-configure-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let accounts = Accounts::new(
        Http::new(),
        SiteStore::new(dir.join("data/sites.json")),
        Box::new(MemoryStore::default()),
    );
    let info = SiteInfo {
        platform: Platform::Sub2api,
        base_url: server.uri(),
        name: "Relay".into(),
        logo_url: None,
        captcha_required: false,
        password_login_disabled: false,
        email_only: true,
        quota_per_unit: None,
        password_encryption: false,
        user_agent: None,
    };
    let credential = Credential::Sub2api {
        access_token: "at".into(),
        refresh_token: "rt".into(),
    };
    let site = accounts.save(info, "a@example.com", &credential).unwrap();
    let configurator = Configurator::new(
        Http::new(),
        home.clone(),
        dir.join("data"),
        Box::new(|_| None),
    );
    Fixture {
        dir,
        home,
        accounts,
        configurator,
        site_id: site.id,
    }
}

fn status(kind: ToolKind, fixture: &Fixture) -> ToolStatus {
    let path = kind.config_path(&fixture.home, &|_| None);
    ToolStatus {
        kind,
        installed: true,
        binary_path: None,
        version: None,
        config_exists: path.is_file(),
        config_path: path.display().to_string(),
        install_command: String::new(),
        install_npm: false,
        configurable: kind.configurable(),
    }
}

fn overview(kind: ToolKind, fixture: &Fixture) -> ToolOverview {
    fixture
        .configurator
        .overview(vec![status(kind, fixture)])
        .remove(0)
}

/// `codex_creations`：这个测试里应该新建几次 Codex 专用 Key。
async fn mount_site(server: &MockServer, codex_creations: u64) {
    Mock::given(method("GET"))
        .and(path("/api/v1/groups/available"))
        .respond_with(ok(json!([
            { "id": 1, "name": "Claude Max", "platform": "anthropic", "rate_multiplier": 1 },
            { "id": 2, "name": "Codex", "platform": "openai", "rate_multiplier": 1 }
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/auth/me"))
        .respond_with(ok(json!({ "email": "a@example.com", "balance": 5 })))
        .mount(server)
        .await;
    // 第一次列 Key 时还没有专用 Key，之后就有了。
    Mock::given(method("GET"))
        .and(path("/api/v1/keys"))
        .respond_with(ok(json!({ "items": [] })))
        .up_to_n_times(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/keys"))
        .respond_with(ok(json!({ "items": [
            { "id": 7, "name": "TransitLink Codex", "key": "sk-codex", "group_id": 2, "status": "active" }
        ] })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/keys"))
        .and(body_partial_json(json!({ "name": "TransitLink Codex", "group_id": 2 })))
        .respond_with(ok(json!(
            { "id": 7, "name": "TransitLink Codex", "key": "sk-codex", "group_id": 2, "status": "active" }
        )))
        .expect(codex_creations)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/keys"))
        .and(body_partial_json(json!({ "name": "TransitLink Claude Code", "group_id": 1 })))
        .respond_with(ok(json!(
            { "id": 8, "name": "TransitLink Claude Code", "key": "sk-claude", "group_id": 1, "status": "active" }
        )))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/keys"))
        .and(body_partial_json(json!({ "name": "TransitLink Claude Code", "group_id": 2 })))
        .respond_with(ok(json!(
            { "id": 9, "name": "TransitLink Claude Code", "key": "sk-wrong-group", "group_id": 2, "status": "active" }
        )))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("Authorization", "Bearer sk-codex"))
        .respond_with(models(&["gpt-5.5", "gpt-6.1-mini", "gpt-6.1-sol"]))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("Authorization", "Bearer sk-claude"))
        .respond_with(models(&["claude-opus-5-5", "claude-sonnet-5-5"]))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("Authorization", "Bearer sk-wrong-group"))
        .respond_with(models(&["gpt-6.1-sol"]))
        .mount(server)
        .await;
}

const CODEX_BEFORE: &str = "# 自己的设置\nmodel = \"o3\"\napproval_policy = \"never\"\n";

#[tokio::test]
async fn setup_plan_reuses_the_last_choice_on_a_site() {
    let server = MockServer::start().await;
    mount_site(&server, 1).await;
    let fixture = fixture(&server, "plan-choice");
    fixture
        .configurator
        .apply(
            &fixture.accounts,
            ToolKind::Codex,
            &fixture.site_id,
            Some("2"),
            Some("gpt-5.5"),
        )
        .await
        .unwrap();
    let plan = fixture
        .configurator
        .plan(&fixture.accounts, &fixture.site_id)
        .await
        .unwrap();
    let codex = plan
        .tools
        .iter()
        .find(|tool| tool.kind == ToolKind::Codex)
        .unwrap();
    assert!(codex.remembered);
    assert_eq!(codex.recommended_group.as_deref(), Some("2"));
    assert_eq!(codex.recommended_model.as_deref(), Some("gpt-5.5"));
    let claude = plan
        .tools
        .iter()
        .find(|tool| tool.kind == ToolKind::ClaudeCode)
        .unwrap();
    assert!(!claude.remembered);
    assert_eq!(claude.recommended_group.as_deref(), Some("1"));
}

#[tokio::test]
async fn configures_tests_switches_models_and_restores_codex() {
    let server = MockServer::start().await;
    mount_site(&server, 1).await;
    let fixture = fixture(&server, "codex");
    let config = fixture.home.join(".codex/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, CODEX_BEFORE).unwrap();

    let plan = fixture
        .configurator
        .plan(&fixture.accounts, &fixture.site_id)
        .await
        .unwrap();
    assert_eq!(plan.balance_usd, Some(5.0));
    let codex_plan = plan
        .tools
        .iter()
        .find(|tool| tool.kind == ToolKind::Codex)
        .unwrap();
    assert_eq!(codex_plan.recommended_group.as_deref(), Some("2"));
    assert!(codex_plan.matched);
    assert_eq!(
        overview(ToolKind::Codex, &fixture).state,
        ConfigState::NotConfigured
    );

    let outcome = fixture
        .configurator
        .apply(
            &fixture.accounts,
            ToolKind::Codex,
            &fixture.site_id,
            Some("2"),
            None,
        )
        .await
        .unwrap();
    assert!(outcome.key_created);
    assert_eq!(outcome.model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(outcome.group.as_deref(), Some("Codex"));
    assert_eq!(
        outcome.test,
        TestResult::Ok {
            models: 3,
            family: 3
        }
    );

    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.contains("experimental_bearer_token = \"sk-codex\""));
    assert!(written.contains(&format!("base_url = \"{}/v1\"", server.uri())));
    assert!(written.contains("approval_policy = \"never\""));
    let view = overview(ToolKind::Codex, &fixture);
    assert_eq!(view.state, ConfigState::Configured);
    let applied = view.applied.unwrap();
    assert_eq!(applied.site_name, "Relay");
    assert_eq!(applied.key_name, "TransitLink Codex");

    // 再配一次：沿用已有的专用 Key（POST 只调用过一次）。
    let again = fixture
        .configurator
        .apply(
            &fixture.accounts,
            ToolKind::Codex,
            &fixture.site_id,
            Some("2"),
            Some("gpt-5.5"),
        )
        .await
        .unwrap();
    assert!(!again.key_created);
    assert_eq!(again.model.as_deref(), Some("gpt-5.5"));

    assert_eq!(
        fixture.configurator.test(ToolKind::Codex).await.unwrap(),
        TestResult::Ok {
            models: 3,
            family: 3
        }
    );
    let listed = fixture.configurator.models(ToolKind::Codex).await.unwrap();
    assert_eq!(listed.len(), 3);
    fixture
        .configurator
        .set_model(ToolKind::Codex, Some("gpt-6.1-sol"))
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("model = \"gpt-6.1-sol\"")
    );
    assert_eq!(
        overview(ToolKind::Codex, &fixture)
            .applied
            .unwrap()
            .model
            .as_deref(),
        Some("gpt-6.1-sol")
    );

    // 别的程序改了配置：显示「已被改动」。
    let changed = std::fs::read_to_string(&config).unwrap().replace(
        "model_provider = \"transitlink\"",
        "model_provider = \"openai\"",
    );
    std::fs::write(&config, changed).unwrap();
    assert_eq!(
        overview(ToolKind::Codex, &fixture).state,
        ConfigState::Changed
    );

    // 还原：回到第一次配置前的样子，记录删掉，原文件备份还在。
    fixture.configurator.restore(ToolKind::Codex).await.unwrap();
    let restored = std::fs::read_to_string(&config).unwrap();
    assert!(
        restored.starts_with("# 自己的设置\nmodel = \"o3\"\n"),
        "{restored}"
    );
    assert!(!restored.contains("transitlink"));
    assert!(!restored.contains("model_provider"));
    assert_eq!(
        overview(ToolKind::Codex, &fixture).state,
        ConfigState::NotConfigured
    );
    assert_eq!(
        std::fs::read_to_string(fixture.dir.join("data/backups/codex/original-config.toml"))
            .unwrap(),
        CODEX_BEFORE
    );
    assert_eq!(
        fixture
            .configurator
            .restore(ToolKind::Codex)
            .await
            .unwrap_err(),
        ConfigError::NotConfigured
    );
}

#[tokio::test]
async fn configures_claude_code_and_skips_onboarding() {
    let server = MockServer::start().await;
    mount_site(&server, 0).await;
    let fixture = fixture(&server, "claude");
    let settings = fixture.home.join(".claude/settings.json");

    // 选错分组（没有 Claude 模型）：报错，什么都不写。
    let error = fixture
        .configurator
        .apply(
            &fixture.accounts,
            ToolKind::ClaudeCode,
            &fixture.site_id,
            Some("2"),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ConfigError::Test(TestResult::NoFamilyModels { models: 1 })
    );
    assert!(!settings.exists());
    assert!(!fixture.home.join(".claude.json").exists());

    let outcome = fixture
        .configurator
        .apply(
            &fixture.accounts,
            ToolKind::ClaudeCode,
            &fixture.site_id,
            Some("1"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        outcome.model, None,
        "Claude Code keeps its own default model"
    );
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(written["env"]["ANTHROPIC_BASE_URL"], json!(server.uri()));
    assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], json!("sk-claude"));
    let user: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.home.join(".claude.json")).unwrap())
            .unwrap();
    assert_eq!(user["hasCompletedOnboarding"], json!(true));
    assert_eq!(
        overview(ToolKind::ClaudeCode, &fixture).state,
        ConfigState::Configured
    );

    // 原来没有 settings.json：还原就删掉它。
    fixture
        .configurator
        .restore(ToolKind::ClaudeCode)
        .await
        .unwrap();
    assert!(!settings.exists());
}

#[test]
fn recommends_groups_by_platform_or_name() {
    let group = |id: &str, name: &str, description: &str, platform: Option<&str>| GroupSummary {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        ratio: None,
        platform: platform.map(str::to_string),
        subscription: false,
    };
    let newapi = [
        group("default", "default", "默认分组", None),
        group("cc", "cc", "", None),
        group("plus", "plus", "GPT-6 专线", None),
    ];
    assert_eq!(
        recommend(
            ToolKind::ClaudeCode,
            Platform::Newapi,
            &newapi,
            Some("default"),
            None
        ),
        (Some("cc".into()), true)
    );
    assert_eq!(
        recommend(
            ToolKind::Codex,
            Platform::Newapi,
            &newapi,
            Some("default"),
            None
        ),
        (Some("plus".into()), true)
    );
    let plain = [
        group("vip", "vip", "", None),
        group("default", "default", "", None),
    ];
    assert_eq!(
        recommend(ToolKind::Codex, Platform::Newapi, &plain, Some("vip"), None),
        (Some("vip".into()), false),
        "falls back to the user's own group"
    );
    let sub2api = [
        group("3", "Gemini", "", Some("gemini")),
        group("4", "Claude", "", Some("anthropic")),
    ];
    assert_eq!(
        recommend(
            ToolKind::ClaudeCode,
            Platform::Sub2api,
            &sub2api,
            None,
            None
        ),
        (Some("4".into()), true)
    );
    assert_eq!(
        recommend(ToolKind::Codex, Platform::Sub2api, &sub2api, None, None),
        (Some("3".into()), false)
    );
    assert_eq!(
        recommend(ToolKind::Codex, Platform::Sub2api, &[], None, None),
        (None, false)
    );

    // 站长指定的分组优先（按 ID 或名称找）；站点里已经没有这个分组时照常推荐。
    let operator = |group: &str| crate::siteconfig::ToolRecommend {
        group: group.into(),
        model: String::new(),
    };
    assert_eq!(
        recommend(
            ToolKind::ClaudeCode,
            Platform::Sub2api,
            &sub2api,
            None,
            Some(&operator("3"))
        ),
        (Some("3".into()), true)
    );
    assert_eq!(
        recommend(
            ToolKind::Codex,
            Platform::Newapi,
            &newapi,
            None,
            Some(&operator("default"))
        ),
        (Some("default".into()), true)
    );
    assert_eq!(
        recommend(
            ToolKind::ClaudeCode,
            Platform::Sub2api,
            &sub2api,
            None,
            Some(&operator("99"))
        ),
        (Some("4".into()), true)
    );
}

#[tokio::test]
async fn switches_configured_tools_between_sites() {
    let a = MockServer::start().await;
    let b = MockServer::start().await;
    // 每个站点只新建一次 Codex 专用 Key：切回来时沿用。
    mount_site(&a, 1).await;
    mount_site(&b, 1).await;
    let fixture = fixture(&a, "switch");
    let site_b = fixture
        .accounts
        .save(
            SiteInfo {
                platform: Platform::Sub2api,
                base_url: b.uri(),
                name: "Backup".into(),
                logo_url: None,
                captcha_required: false,
                password_login_disabled: false,
                email_only: true,
                quota_per_unit: None,
                password_encryption: false,
                user_agent: None,
            },
            "a@example.com",
            &Credential::Sub2api {
                access_token: "at".into(),
                refresh_token: "rt".into(),
            },
        )
        .unwrap()
        .id;
    let config = fixture.home.join(".codex/config.toml");

    fixture
        .configurator
        .apply(
            &fixture.accounts,
            ToolKind::Codex,
            &fixture.site_id,
            Some("2"),
            None,
        )
        .await
        .unwrap();
    fixture
        .configurator
        .set_model(ToolKind::Codex, Some("gpt-5.5"))
        .await
        .unwrap();

    // 第一次切到 B：用推荐的分组，模型沿用现在的。
    let outcome = fixture
        .configurator
        .switch(&fixture.accounts, ToolKind::Codex, &site_b)
        .await
        .unwrap();
    assert_eq!(outcome.model.as_deref(), Some("gpt-5.5"));
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains(&format!("base_url = \"{}/v1\"", b.uri()))
    );
    let configured = fixture.configurator.configured();
    assert_eq!(configured.len(), 1);
    assert_eq!(configured[0].1.site_name, "Backup");

    // 切回 A：沿用在 A 上选过的分组和模型，不再新建 Key。
    let back = fixture
        .configurator
        .switch(&fixture.accounts, ToolKind::Codex, &fixture.site_id)
        .await
        .unwrap();
    assert!(!back.key_created);
    assert_eq!(back.model.as_deref(), Some("gpt-5.5"));
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains(&format!("base_url = \"{}/v1\"", a.uri()))
    );
}

/// 新加的三个工具：各自的分组、专用 Key 和模型。
async fn mount_more_tools(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/groups/available"))
        .respond_with(ok(json!([
            { "id": 1, "name": "Claude", "platform": "anthropic", "rate_multiplier": 1 },
            { "id": 2, "name": "Codex", "platform": "openai", "rate_multiplier": 1 },
            { "id": 3, "name": "Gemini", "platform": "gemini", "rate_multiplier": 1 },
            { "id": 4, "name": "Grok", "platform": "grok", "rate_multiplier": 1 },
            { "id": 5, "name": "DeepSeek", "platform": "deepseek", "rate_multiplier": 1 }
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/auth/me"))
        .respond_with(ok(json!({ "email": "a@example.com", "balance": 5 })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/keys"))
        .respond_with(ok(json!({ "items": [] })))
        .mount(server)
        .await;
    for (name, group, key, ids) in [
        (
            "TransitLink Gemini CLI",
            3,
            "sk-gemini",
            vec!["gemini-3.8-flash", "gemini-3.1-pro-preview", "imagen-4"],
        ),
        (
            "TransitLink Grok Build",
            4,
            "sk-grok",
            vec!["grok-code-fast-1", "grok-4.7", "grok-imagine-1"],
        ),
        (
            "TransitLink OpenCode",
            1,
            "sk-opencode",
            vec!["claude-haiku-5-5", "claude-opus-5-5", "claude-sonnet-5-5"],
        ),
        (
            "TransitLink DeepSeek Harness",
            5,
            "sk-dsh",
            vec!["deepseek-v4-flash", "deepseek-v4", "claude-sonnet-5-5"],
        ),
    ] {
        Mock::given(method("POST"))
            .and(path("/api/v1/keys"))
            .and(body_partial_json(json!({ "name": name, "group_id": group })))
            .respond_with(ok(json!(
                { "id": group * 10, "name": name, "key": key, "group_id": group, "status": "active" }
            )))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", format!("Bearer {key}").as_str()))
            .respond_with(models(&ids))
            .mount(server)
            .await;
    }
}

#[tokio::test]
async fn configures_and_restores_gemini_grok_and_opencode() {
    let server = MockServer::start().await;
    mount_more_tools(&server).await;
    let fixture = fixture(&server, "more-tools");
    let apply = |kind: ToolKind, group: &'static str| {
        fixture
            .configurator
            .apply(&fixture.accounts, kind, &fixture.site_id, Some(group), None)
    };

    // Gemini CLI：.env 写地址、Key、模型；settings.json 改登录方式，注释和原来的登录方式都保留。
    let gemini_dir = fixture.home.join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    let settings_before = "{\n  // 我的 Gemini 设置\n  \"security\": { \"auth\": { \"selectedType\": \"oauth-personal\" } }\n}\n";
    std::fs::write(gemini_dir.join("settings.json"), settings_before).unwrap();
    let outcome = apply(ToolKind::GeminiCli, "3").await.unwrap();
    assert_eq!(outcome.model.as_deref(), Some("gemini-3.1-pro-preview"));
    let env = std::fs::read_to_string(gemini_dir.join(".env")).unwrap();
    assert!(env.contains(&format!("GOOGLE_GEMINI_BASE_URL={}", server.uri())));
    assert!(env.contains("GEMINI_API_KEY=sk-gemini"));
    let settings = std::fs::read_to_string(gemini_dir.join("settings.json")).unwrap();
    assert!(settings.contains("// 我的 Gemini 设置"));
    assert!(settings.contains("\"gemini-api-key\""));
    assert_eq!(
        overview(ToolKind::GeminiCli, &fixture).state,
        ConfigState::Configured
    );
    fixture
        .configurator
        .restore(ToolKind::GeminiCli)
        .await
        .unwrap();
    assert!(!gemini_dir.join(".env").exists());
    assert_eq!(
        std::fs::read_to_string(gemini_dir.join("settings.json")).unwrap(),
        settings_before
    );

    // Grok Build：config.toml 里我们的模型表，默认模型指向它；可以换模型；原来没有文件，还原就删掉。
    let outcome = apply(ToolKind::GrokBuild, "4").await.unwrap();
    assert_eq!(outcome.model.as_deref(), Some("grok-4.7"));
    let grok = fixture.home.join(".grok/config.toml");
    let text = std::fs::read_to_string(&grok).unwrap();
    assert!(text.contains("api_key = \"sk-grok\""));
    assert!(text.contains(&format!("base_url = \"{}/v1\"", server.uri())));
    assert_eq!(
        overview(ToolKind::GrokBuild, &fixture).state,
        ConfigState::Configured
    );
    fixture
        .configurator
        .set_model(ToolKind::GrokBuild, Some("grok-code-fast-1"))
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&grok)
            .unwrap()
            .contains("model = \"grok-code-fast-1\"")
    );
    fixture
        .configurator
        .restore(ToolKind::GrokBuild)
        .await
        .unwrap();
    assert!(!grok.exists());

    // OpenCode：Claude 分组走 Anthropic 接口，默认用 sonnet，起标题用 haiku；换模型只列 Claude 模型。
    let outcome = apply(ToolKind::OpenCode, "1").await.unwrap();
    assert_eq!(outcome.model.as_deref(), Some("claude-sonnet-5-5"));
    let opencode = fixture.home.join(".config/opencode/opencode.json");
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&opencode).unwrap()).unwrap();
    assert_eq!(written["model"], json!("transitlink/claude-sonnet-5-5"));
    assert_eq!(
        written["small_model"],
        json!("transitlink/claude-haiku-5-5")
    );
    assert_eq!(
        written["provider"]["transitlink"]["npm"],
        json!("@ai-sdk/anthropic")
    );
    assert_eq!(
        written["provider"]["transitlink"]["options"]["apiKey"],
        json!("sk-opencode")
    );
    assert_eq!(
        written["provider"]["transitlink"]["models"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        overview(ToolKind::OpenCode, &fixture).state,
        ConfigState::Configured
    );
    let choices = fixture
        .configurator
        .models(ToolKind::OpenCode)
        .await
        .unwrap();
    assert!(choices.iter().all(|id| id.contains("claude")));
    fixture
        .configurator
        .restore(ToolKind::OpenCode)
        .await
        .unwrap();
    assert!(!opencode.exists());

    // DeepSeek Harness：web、desktop 配置档写服务商和默认模型，Key 放 .credentials.yaml；
    // DeepSeek 模型走 OpenAI 兼容接口，只列 DeepSeek 模型。
    let outcome = apply(ToolKind::DeepseekHarness, "5").await.unwrap();
    assert_eq!(outcome.model.as_deref(), Some("deepseek-v4"));
    let dsh_home = fixture.home.join(".dsh");
    let web = std::fs::read_to_string(dsh_home.join("profiles/web/cordis.patch.yml")).unwrap();
    assert!(web.contains("api: openai-completions"));
    assert!(web.contains("thinkingFormat: deepseek"));
    assert!(web.contains("deepseek-v4-flash"));
    assert!(!web.contains("claude-sonnet-5-5"));
    assert!(dsh_home.join("profiles/desktop/cordis.patch.yml").is_file());
    let credentials = std::fs::read_to_string(dsh_home.join(".credentials.yaml")).unwrap();
    assert!(credentials.contains("TRANSITLINK_API_KEY: sk-dsh"));
    assert_eq!(
        overview(ToolKind::DeepseekHarness, &fixture).state,
        ConfigState::Configured
    );
    let choices = fixture
        .configurator
        .models(ToolKind::DeepseekHarness)
        .await
        .unwrap();
    assert!(choices.iter().all(|id| id.contains("deepseek")));
    fixture
        .configurator
        .set_model(ToolKind::DeepseekHarness, Some("deepseek-v4-flash"))
        .await
        .unwrap();
    assert_eq!(
        overview(ToolKind::DeepseekHarness, &fixture)
            .current_model
            .as_deref(),
        Some("deepseek-v4-flash")
    );
    fixture
        .configurator
        .restore(ToolKind::DeepseekHarness)
        .await
        .unwrap();
    assert!(!dsh_home.join("profiles/web/cordis.patch.yml").exists());
    assert!(!dsh_home.join(".credentials.yaml").exists());
}

#[test]
fn recommends_groups_for_the_new_tools() {
    let group = |id: &str, name: &str, platform: Option<&str>| GroupSummary {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        ratio: None,
        platform: platform.map(str::to_string),
        subscription: false,
    };
    let sub2api = vec![
        group("2", "Codex", Some("openai")),
        group("1", "Claude", Some("anthropic")),
        group("3", "Gemini", Some("gemini")),
        group("4", "Grok", Some("grok")),
    ];
    let pick =
        |kind, groups: &[GroupSummary], platform| recommend(kind, platform, groups, None, None);
    assert_eq!(
        pick(ToolKind::GeminiCli, &sub2api, Platform::Sub2api),
        (Some("3".into()), true)
    );
    assert_eq!(
        pick(ToolKind::GrokBuild, &sub2api, Platform::Sub2api),
        (Some("4".into()), true)
    );
    // OpenCode 优先 Claude 分组（即使 GPT 分组排在前面）。
    assert_eq!(
        pick(ToolKind::OpenCode, &sub2api, Platform::Sub2api),
        (Some("1".into()), true)
    );

    let newapi = vec![
        group("default", "default", None),
        group("g", "Google Gemini", None),
        group("x", "xAI 专线", None),
    ];
    assert_eq!(
        pick(ToolKind::GeminiCli, &newapi, Platform::Newapi),
        (Some("g".into()), true)
    );
    assert_eq!(
        pick(ToolKind::GrokBuild, &newapi, Platform::Newapi),
        (Some("x".into()), true)
    );
    assert_eq!(
        pick(ToolKind::OpenCode, &newapi, Platform::Newapi),
        (Some("default".into()), false)
    );
}
