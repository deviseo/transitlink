//! 工具升级：查 npm 上的最新版本，再按工具是怎么装的选升级办法（做法参考 cc-switch）：
//!
//! - DeepSeek Harness 桌面版带的命令 → 请用户在桌面版里更新（先看这个，桌面版里也有 node_modules）；
//! - Homebrew 装的（真实位置在 `Cellar`、`Caskroom` 里）→ 用那一处的 `brew upgrade`；
//! - npm 装的（真实位置在 `node_modules` 里；Windows 上命令是 `.cmd` 启动脚本，包在同目录的
//!   `node_modules` 里）→ 用同一处的 npm `install -g <包>@<版本>`（npm 12 默认拦截安装脚本，
//!   靠脚本放程序的包要明确允许，否则装完是空壳）；
//! - pnpm、bun、Yarn、Volta 装的 → 给出对应的命令请用户在终端里运行（用 npm 会再装出一份）；
//! - 官方安装脚本装的 → Claude Code、Grok Build 用自带的 `update`，OpenCode 用 `upgrade`，
//!   Codex 重新运行官方安装脚本（`codex update` 断网时也报成功）。
//!
//! npm 升级装查到的那个版本号，不用 `@latest`：国内镜像有时落后很多，它的 `latest` 可能比本机的还旧。
//! 升级命令都用绝对路径运行，命令所在的目录放在 PATH 最前面（从桌面启动的应用拿不到终端的 PATH，
//! `#!/usr/bin/env node` 这类脚本要找到同一处的 node）。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::install::{self, Method, NPM_MIRROR};
use crate::sites::http::Http;
use crate::tools::ToolKind;

/// 工具在 npm 上的包名（官方安装脚本装的版本号和 npm 上的一致）。OpenCode 2 是另一个包。
pub fn npm_package(kind: ToolKind, local: Option<&str>) -> &'static str {
    match kind {
        ToolKind::ClaudeCode => "@anthropic-ai/claude-code",
        ToolKind::Codex => "@openai/codex",
        ToolKind::GeminiCli => "@google/gemini-cli",
        ToolKind::GrokBuild => "@xai-official/grok",
        ToolKind::OpenCode if local.and_then(major).is_some_and(|major| major >= 2) => {
            "@opencode/cli"
        }
        ToolKind::OpenCode => "opencode-ai",
        ToolKind::DeepseekHarness => "@deepseek-ai/dsh",
        ToolKind::Pi => "@earendil-works/pi-coding-agent",
        ToolKind::OhMyPi => "@oh-my-pi/pi-coding-agent",
        ToolKind::KimiCode => "@moonshot-ai/kimi-code",
        ToolKind::MinimaxCode => "@minimax-ai/code",
        ToolKind::OpenClaw => "openclaw",
        // 没有 npm 包（最新版本另外查，见 `latest`）。
        ToolKind::Antigravity => "",
    }
}

/// 这个工具可能的 npm 包名（OpenCode 1 和 2 是两个包）：看命令是谁装的时候用。
pub fn npm_packages(kind: ToolKind) -> &'static [&'static str] {
    match kind {
        ToolKind::ClaudeCode => &["@anthropic-ai/claude-code"],
        ToolKind::Codex => &["@openai/codex"],
        ToolKind::GeminiCli => &["@google/gemini-cli"],
        ToolKind::GrokBuild => &["@xai-official/grok"],
        ToolKind::OpenCode => &["opencode-ai", "@opencode/cli"],
        ToolKind::DeepseekHarness => &["@deepseek-ai/dsh"],
        ToolKind::Pi => &["@earendil-works/pi-coding-agent"],
        ToolKind::OhMyPi => &["@oh-my-pi/pi-coding-agent"],
        ToolKind::KimiCode => &["@moonshot-ai/kimi-code"],
        ToolKind::MinimaxCode => &["@minimax-ai/code"],
        ToolKind::OpenClaw => &["openclaw"],
        ToolKind::Antigravity => &[],
    }
}

/// Homebrew 卸载、清理时默认会顺手删掉「不再被需要」的依赖（例如别的工具还在用的 node）：关掉它。
pub const HOMEBREW_NO_AUTOREMOVE: (&str, &str) = ("HOMEBREW_NO_AUTOREMOVE", "1");

/// npm 升级时要加的参数（和一键安装一致）。
fn npm_extra(kind: ToolKind, package: &str) -> Vec<String> {
    match kind {
        ToolKind::ClaudeCode | ToolKind::GrokBuild | ToolKind::OpenCode => vec![
            "--ignore-scripts=false".into(),
            "--include=optional".into(),
            format!("--allow-scripts={package}"),
        ],
        ToolKind::DeepseekHarness => vec!["--engine-strict".into()],
        ToolKind::Pi => vec!["--ignore-scripts".into()],
        // 它的安装脚本要放好 SQLite 的原生模块。
        ToolKind::MinimaxCode => vec![
            "--ignore-scripts=false".into(),
            "--include=optional".into(),
            "--allow-scripts=@minimax-ai/code,better-sqlite3".into(),
        ],
        ToolKind::OpenClaw => vec![
            "--ignore-scripts=false".into(),
            "--allow-scripts=openclaw".into(),
        ],
        ToolKind::Codex
        | ToolKind::GeminiCli
        | ToolKind::OhMyPi
        | ToolKind::KimiCode
        | ToolKind::Antigravity => Vec::new(),
    }
}

fn major(version: &str) -> Option<u64> {
    version
        .split('.')
        .next()?
        .trim_start_matches('v')
        .parse()
        .ok()
}

/// 某个工具的最新版本。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLatest {
    pub kind: ToolKind,
    pub latest: Option<String>,
    /// 最新版比本机装的新。
    pub update_available: bool,
    /// 本机装的就是最新版（查到的比本机的旧时——例如镜像落后——两个都是 false）。
    pub up_to_date: bool,
}

/// Antigravity CLI 没有 npm 包：问它自己的更新服务（和它的安装脚本、`agy update` 用的一样）。
async fn antigravity_latest(http: &Http) -> Option<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        _ => return None,
    };
    let url = format!(
        "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/{os}_{arch}.json"
    );
    let request = http.get(&url).timeout(std::time::Duration::from_secs(10));
    let response = http.send(request).await.ok()?;
    (response.status == 200).then_some(()).and_then(|()| {
        response
            .body
            .as_ref()?
            .get("version")?
            .as_str()
            .map(str::to_string)
    })
}

/// npm 上的最新版本：先问官方源，连不上再问国内镜像。
pub async fn latest(http: &Http, kind: ToolKind, local: Option<&str>) -> Option<String> {
    if kind == ToolKind::Antigravity {
        return antigravity_latest(http).await;
    }
    let package = npm_package(kind, local);
    let encoded = package.replacen('/', "%2F", 1);
    for registry in ["https://registry.npmjs.org", NPM_MIRROR] {
        let request = http
            .get(&format!("{registry}/{encoded}/latest"))
            .timeout(std::time::Duration::from_secs(10));
        if let Ok(response) = http.send(request).await
            && response.status == 200
            && let Some(version) = response
                .body
                .as_ref()
                .and_then(|body| body.get("version"))
                .and_then(|version| version.as_str())
        {
            return Some(version.to_string());
        }
    }
    None
}

/// 一个版本号：主、次、修订号和预发布标识（`1.2.3-rc.4` → `[1,2,3]`、`["rc","4"]`）。
fn parse(version: &str) -> Option<(Vec<u64>, Vec<String>)> {
    let version = version.trim().trim_start_matches('v');
    let version = version.split('+').next()?;
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, pre.split('.').map(str::to_string).collect()),
        None => (version, Vec::new()),
    };
    let numbers: Vec<u64> = core
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    (!numbers.is_empty()).then_some((numbers, pre))
}

/// `latest` 比 `local` 新（按语义化版本比较；正式版比同号的预发布版新）。看不懂的版本号不算新。
pub fn is_newer(latest: &str, local: &str) -> bool {
    let (Some((latest_core, latest_pre)), Some((local_core, local_pre))) =
        (parse(latest), parse(local))
    else {
        return false;
    };
    let width = latest_core.len().max(local_core.len());
    let pad = |core: &[u64]| {
        let mut core = core.to_vec();
        core.resize(width, 0);
        core
    };
    match pad(&latest_core).cmp(&pad(&local_core)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (latest_pre.is_empty(), local_pre.is_empty()) {
            (true, true) => false,
            (true, false) => true,
            (false, true) => false,
            (false, false) => pre_newer(&latest_pre, &local_pre),
        },
    }
}

/// 两个版本号一样（`1.2` 和 `1.2.0` 算一样）。看不懂的不算一样。
pub fn same_version(a: &str, b: &str) -> bool {
    parse(a).is_some() && parse(b).is_some() && !is_newer(a, b) && !is_newer(b, a)
}

/// npm 要装的版本：看起来是正常的版本号才用（它来自 npm 源的回复，会拼进 npm 的参数），否则 `latest`。
fn version_spec(target: Option<&str>) -> String {
    target
        .filter(|version| {
            parse(version).is_some()
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        })
        .unwrap_or("latest")
        .to_string()
}

/// 预发布标识逐段比：数字按大小，文字按字母序，数字比文字小，段数多的新。
fn pre_newer(latest: &[String], local: &[String]) -> bool {
    for (a, b) in latest.iter().zip(local) {
        let ordering = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering == std::cmp::Ordering::Greater;
        }
    }
    latest.len() > local.len()
}

/// 怎么升级。
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// 运行这个命令（`npm`：是 npm 升级，失败时按 npm 的错误分析；`env`：额外的环境变量）。
    Run {
        program: PathBuf,
        args: Vec<String>,
        display: String,
        npm: bool,
        env: Vec<(String, String)>,
    },
    /// 重新运行官方安装（Codex）。
    Reinstall(Method),
    /// 这里没法升级：`command` 是请用户在终端里运行的命令；为空时是桌面版带的，请在桌面版里更新。
    Manual { command: Option<String> },
}

/// 装工具用的包管理器。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Manager {
    Npm,
    Pnpm,
    Bun,
    Yarn,
    Volta,
}

/// 启动脚本这么小以内才读（真正的程序动辄几十 MB，不读）。
const MAX_SCRIPT_BYTES: u64 = 64 * 1024;

pub(crate) fn small_text(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_SCRIPT_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// 看命令的真实位置（和它旁边的文件、它的启动脚本）判断是哪个包管理器装的。
pub(crate) fn manager(binary: &Path, real: &Path, packages: &[&str]) -> Option<Manager> {
    let lower = path_text(real).to_ascii_lowercase();
    if lower.contains("/.volta/") || lower.contains("/volta/bin/") {
        return Some(Manager::Volta);
    }
    if lower.contains("/.bun/") {
        return Some(Manager::Bun);
    }
    if lower.contains("/pnpm/global/") || lower.contains("/node_modules/.pnpm/") {
        return Some(Manager::Pnpm);
    }
    if lower.contains("/yarn/") && lower.contains("/global/node_modules/") {
        return Some(Manager::Yarn);
    }
    if lower.contains("/node_modules/") {
        return Some(Manager::Npm);
    }
    // 命令是启动脚本（Windows 上 npm 的 `.cmd`，pnpm、Yarn 的脚本）：包在同目录的 node_modules 里
    // 就是 npm 装的；否则看脚本里写的位置。
    let dir = binary.parent()?;
    if packages
        .iter()
        .any(|package| dir.join("node_modules").join(package).is_dir())
    {
        return Some(Manager::Npm);
    }
    let script = small_text(binary)?.replace('\\', "/").to_ascii_lowercase();
    if script.contains("/pnpm/global/") || script.contains("/.pnpm/") {
        return Some(Manager::Pnpm);
    }
    if script.contains("/yarn/") && script.contains("/global/node_modules/") {
        return Some(Manager::Yarn);
    }
    None
}

pub(crate) fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Homebrew：真实位置里 `Cellar/<formula>/`、`Caskroom/<cask>/` 前面的部分是 Homebrew 的目录。
fn homebrew(real: &Path) -> Option<(PathBuf, bool, String)> {
    let text = path_text(real);
    for (marker, cask) in [("/Cellar/", false), ("/Caskroom/", true)] {
        if let Some(index) = text.find(marker) {
            let prefix = PathBuf::from(&text[..index]);
            let name = text[index + marker.len()..].split('/').next()?.to_string();
            return Some((prefix.join("bin").join("brew"), cask, name));
        }
    }
    None
}

/// 选升级办法。`binary`：检测到的命令位置；`local`：本机的版本；`target`：要升到的版本（查到的
/// 最新版）；`search`：找 npm 用的 PATH。
pub fn plan(
    kind: ToolKind,
    binary: &Path,
    local: Option<&str>,
    target: Option<&str>,
    mirror: bool,
    search: &OsString,
    home: &Path,
) -> Option<Plan> {
    let real = std::fs::canonicalize(binary).unwrap_or_else(|_| binary.to_path_buf());
    let lower = path_text(&real).to_ascii_lowercase();
    let package = npm_package(kind, local);
    let spec = version_spec(target);
    let own = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        let name = binary
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        Some(Plan::Run {
            program: binary.to_path_buf(),
            display: format!("{name} {}", args.join(" ")),
            args,
            npm: false,
            env: Vec::new(),
        })
    };

    // Antigravity CLI：怎么装的都用它自己的 `agy update`（Homebrew 的 cask 标了自动更新，
    // `brew upgrade` 会跳过或者装回 cask 记录的版本）。
    if kind == ToolKind::Antigravity {
        return own(&["update"]);
    }
    // 桌面版带的命令：macOS 在 `.app` 里，Windows、Linux 在安装目录的 `resources` 里。
    if kind == ToolKind::DeepseekHarness
        && (lower.contains(".app/") || lower.contains("/resources/"))
    {
        return Some(Plan::Manual { command: None });
    }
    if let Some((brew, cask, name)) = homebrew(&real) {
        let mut args = vec!["upgrade".to_string()];
        if cask {
            args.push("--cask".into());
        }
        args.push(name);
        let display = format!("brew {}", args.join(" "));
        let (key, value) = HOMEBREW_NO_AUTOREMOVE;
        return Some(Plan::Run {
            program: brew,
            args,
            display: format!("{key}={value} {display}"),
            npm: false,
            env: vec![(key.to_string(), value.to_string())],
        });
    }
    // Oh My Pi、OpenClaw 自己认得是用 npm、bun 还是编好的程序装的，按原来的办法升级
    // （OpenClaw 还会顺便重启它的后台服务）。
    match kind {
        ToolKind::OhMyPi => return own(&["update"]),
        ToolKind::OpenClaw => return own(&["update", "--yes"]),
        _ => {}
    }
    let command = |prefix: &str| {
        Some(Plan::Manual {
            command: Some(format!("{prefix} {package}@{spec}")),
        })
    };
    match manager(binary, &real, npm_packages(kind)) {
        Some(Manager::Npm) => return npm_plan(kind, package, &spec, binary, mirror, search, home),
        Some(Manager::Pnpm) => return command("pnpm add -g"),
        Some(Manager::Bun) => return command("bun add -g"),
        Some(Manager::Yarn) => return command("yarn global add"),
        Some(Manager::Volta) => return command("volta install"),
        None => {}
    }
    match kind {
        ToolKind::ClaudeCode | ToolKind::GrokBuild | ToolKind::Pi => own(&["update"]),
        ToolKind::OpenCode => own(&["upgrade"]),
        // 只下载好新版本，下次运行时换上（检测版本时就会运行一次）。
        ToolKind::KimiCode => own(&["upgrade", "--yes"]),
        ToolKind::Codex => Some(Plan::Reinstall(install::method(kind))),
        // `mcode update` 不在终端里运行时什么都不装：重新运行官方安装脚本（已经是最新时很快就结束）。
        ToolKind::MinimaxCode => Some(Plan::Reinstall(install::method(kind))),
        ToolKind::OhMyPi => own(&["update"]),
        ToolKind::OpenClaw => own(&["update", "--yes"]),
        ToolKind::Antigravity => own(&["update"]),
        // 只能用 npm 装的工具，装在别处时（例如单独下载的程序）用 npm 装一份新的。
        ToolKind::GeminiCli | ToolKind::DeepseekHarness => {
            npm_plan(kind, package, &spec, binary, mirror, search, home)
        }
    }
}

/// 用和工具同一处的 npm 升级（同目录里没有 npm 时用 PATH 里的）。
fn npm_plan(
    kind: ToolKind,
    package: &str,
    spec: &str,
    binary: &Path,
    mirror: bool,
    search: &OsString,
    home: &Path,
) -> Option<Plan> {
    let sibling = binary
        .parent()
        .map(|dir| dir.join(if cfg!(windows) { "npm.cmd" } else { "npm" }));
    let npm = sibling
        .filter(|path| path.is_file())
        .or_else(|| which::which_in("npm", Some(search), home).ok())?;
    let mut args = vec![
        "install".to_string(),
        "-g".to_string(),
        format!("{package}@{spec}"),
    ];
    args.extend(npm_extra(kind, package));
    if mirror {
        args.push(format!("--registry={NPM_MIRROR}"));
    }
    let display = format!("npm {}", args.join(" "));
    Some(Plan::Run {
        program: npm,
        args,
        display,
        npm: true,
        env: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_like_semver() {
        assert!(is_newer("2.1.301", "2.1.300"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(!is_newer("1.0.70", "1.0.70"));
        assert!(!is_newer("1.0.69", "1.0.70"));
        assert!(
            is_newer("0.2.1", "0.2.1-rc.5"),
            "a release is newer than its prereleases"
        );
        assert!(is_newer("0.2.1-rc.5", "0.2.1-rc.4"));
        assert!(is_newer("0.2.1-rc.10", "0.2.1-rc.9"));
        assert!(!is_newer("0.2.1-rc.5", "0.2.1"));
        assert!(is_newer("0.150.0-alpha.2", "0.149.0"));
        assert!(!is_newer("not a version", "1.0.0"));
        assert!(!is_newer("1.0.0", "unknown"));
    }

    #[test]
    fn opencode_two_uses_its_own_package() {
        assert_eq!(
            npm_package(ToolKind::OpenCode, Some("1.19.5")),
            "opencode-ai"
        );
        assert_eq!(
            npm_package(ToolKind::OpenCode, Some("2.2.5")),
            "@opencode/cli"
        );
        assert_eq!(npm_package(ToolKind::OpenCode, None), "opencode-ai");
    }

    #[test]
    fn homebrew_installs_upgrade_with_brew() {
        let (brew, cask, name) = homebrew(Path::new(
            "/opt/homebrew/Cellar/gemini-cli/0.66.0/bin/gemini",
        ))
        .unwrap();
        assert_eq!(brew, PathBuf::from("/opt/homebrew/bin/brew"));
        assert!(!cask);
        assert_eq!(name, "gemini-cli");
        let (_, cask, name) =
            homebrew(Path::new("/usr/local/Caskroom/grok-build/1.0.70/grok")).unwrap();
        assert!(cask);
        assert_eq!(name, "grok-build");
        assert!(homebrew(Path::new("/Users/a/.local/bin/claude")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn picks_the_upgrade_by_install_location() {
        let dir = std::env::temp_dir().join(format!("transitlink-upgrade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        // npm 装的 Gemini CLI：同目录有 npm。
        let npm_bin = dir.join("node/bin");
        let package = dir.join("node/lib/node_modules/@google/gemini-cli/dist");
        std::fs::create_dir_all(&npm_bin).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("index.js"), "").unwrap();
        std::fs::write(npm_bin.join("npm"), "").unwrap();
        std::os::unix::fs::symlink(package.join("index.js"), npm_bin.join("gemini")).unwrap();
        let search = OsString::new();
        let gemini = plan(
            ToolKind::GeminiCli,
            &npm_bin.join("gemini"),
            Some("0.66.0"),
            Some("0.67.0"),
            true,
            &search,
            &home,
        )
        .unwrap();
        assert_eq!(
            gemini,
            Plan::Run {
                program: npm_bin.join("npm"),
                args: vec![
                    "install".into(),
                    "-g".into(),
                    "@google/gemini-cli@0.67.0".into(),
                    format!("--registry={NPM_MIRROR}"),
                ],
                display: format!(
                    "npm install -g @google/gemini-cli@0.67.0 --registry={NPM_MIRROR}"
                ),
                npm: true,
                env: Vec::new(),
            }
        );

        // 官方安装脚本装的 Claude Code、OpenCode：用它们自己的升级命令；Codex 重新运行官方安装。
        let local_bin = home.join(".local/bin");
        std::fs::create_dir_all(&local_bin).unwrap();
        std::fs::write(local_bin.join("claude"), "").unwrap();
        let claude = plan(
            ToolKind::ClaudeCode,
            &local_bin.join("claude"),
            None,
            None,
            false,
            &search,
            &home,
        )
        .unwrap();
        assert!(matches!(claude, Plan::Run { ref args, npm: false, .. } if args == &["update"]));
        std::fs::write(local_bin.join("opencode"), "").unwrap();
        let opencode = plan(
            ToolKind::OpenCode,
            &local_bin.join("opencode"),
            None,
            None,
            false,
            &search,
            &home,
        )
        .unwrap();
        assert!(matches!(opencode, Plan::Run { ref args, .. } if args == &["upgrade"]));
        std::fs::write(local_bin.join("codex"), "").unwrap();
        assert!(matches!(
            plan(
                ToolKind::Codex,
                &local_bin.join("codex"),
                None,
                None,
                false,
                &search,
                &home
            ),
            Some(Plan::Reinstall(_))
        ));

        // DeepSeek Harness 桌面版带的命令（在桌面版的 node_modules 里也一样）：请用户在桌面版里更新。
        let app = dir.join("DeepSeek Harness.app/Contents/Resources/app/node_modules/.bin");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("dsh"), "").unwrap();
        assert_eq!(
            plan(
                ToolKind::DeepseekHarness,
                &app.join("dsh"),
                None,
                None,
                false,
                &search,
                &home
            ),
            Some(Plan::Manual { command: None })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn other_package_managers_get_their_own_command() {
        let dir = std::env::temp_dir().join(format!("transitlink-managers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let search = OsString::new();
        let plan_for = |kind: ToolKind, binary: &Path| {
            plan(
                kind,
                binary,
                Some("1.0.0"),
                Some("1.2.0"),
                false,
                &search,
                &home,
            )
        };

        // Windows 上 npm 的布局：命令是启动脚本，包在同目录的 node_modules 里。
        let npm_dir = dir.join("AppData/Roaming/npm");
        std::fs::create_dir_all(npm_dir.join("node_modules/@openai/codex")).unwrap();
        std::fs::write(
            npm_dir.join("codex"),
            "#!/bin/sh\nexec node \"$basedir/node_modules/@openai/codex/bin/codex.js\"\n",
        )
        .unwrap();
        std::fs::write(npm_dir.join("npm"), "").unwrap();
        assert!(matches!(
            plan_for(ToolKind::Codex, &npm_dir.join("codex")),
            Some(Plan::Run { ref args, npm: true, .. }) if args[2] == "@openai/codex@1.2.0"
        ));

        // pnpm 的启动脚本。
        let pnpm_dir = dir.join("Library/pnpm");
        std::fs::create_dir_all(&pnpm_dir).unwrap();
        std::fs::write(pnpm_dir.join("gemini"), "#!/bin/sh\nexec node \"$basedir/global/5/node_modules/@google/gemini-cli/dist/index.js\"\n# /Users/a/Library/pnpm/global/5/.pnpm\n").unwrap();
        assert_eq!(
            plan_for(ToolKind::GeminiCli, &pnpm_dir.join("gemini")),
            Some(Plan::Manual {
                command: Some("pnpm add -g @google/gemini-cli@1.2.0".into())
            })
        );

        // bun：链接到 ~/.bun/install/global/node_modules。
        let bun_package = dir.join(".bun/install/global/node_modules/opencode-ai/bin");
        let bun_bin = dir.join(".bun/bin");
        std::fs::create_dir_all(&bun_package).unwrap();
        std::fs::create_dir_all(&bun_bin).unwrap();
        std::fs::write(bun_package.join("opencode"), "").unwrap();
        std::os::unix::fs::symlink(bun_package.join("opencode"), bun_bin.join("opencode")).unwrap();
        assert_eq!(
            plan_for(ToolKind::OpenCode, &bun_bin.join("opencode")),
            Some(Plan::Manual {
                command: Some("bun add -g opencode-ai@1.2.0".into())
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_are_compared_and_sanitised() {
        assert!(same_version("1.2", "1.2.0"));
        assert!(same_version("0.2.1-rc.5", "0.2.1-rc.5"));
        assert!(!same_version("1.0.70", "0.1.4"));
        assert!(!same_version("x", "x"));
        assert_eq!(version_spec(Some("0.67.0")), "0.67.0");
        assert_eq!(version_spec(Some("0.2.1-rc.5")), "0.2.1-rc.5");
        assert_eq!(version_spec(Some("file:/tmp/evil")), "latest");
        assert_eq!(version_spec(Some("1.0.0 || 2")), "latest");
        assert_eq!(version_spec(None), "latest");
    }
}
