//! 本机编程工具的检测：Claude Code、Codex、Gemini CLI、Grok Build、OpenCode。写入配置在 `configure`
//! 模块，一键安装在 `install` 模块。

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    ClaudeCode,
    Codex,
    GeminiCli,
    GrokBuild,
    #[serde(rename = "opencode")]
    OpenCode,
    DeepseekHarness,
    Pi,
    OhMyPi,
    KimiCode,
    MinimaxCode,
    #[serde(rename = "openclaw")]
    OpenClaw,
    Antigravity,
}

impl ToolKind {
    pub const ALL: [ToolKind; 12] = [
        ToolKind::ClaudeCode,
        ToolKind::Codex,
        ToolKind::GeminiCli,
        ToolKind::GrokBuild,
        ToolKind::OpenCode,
        ToolKind::DeepseekHarness,
        ToolKind::Pi,
        ToolKind::OhMyPi,
        ToolKind::KimiCode,
        ToolKind::MinimaxCode,
        ToolKind::OpenClaw,
        ToolKind::Antigravity,
    ];

    /// 能一键连接站点。Antigravity CLI 只能用 Google 账号或 Gemini 官方格式的接口，Key 也只能放在
    /// 环境变量里，没法写进配置。
    pub fn configurable(self) -> bool {
        self != ToolKind::Antigravity
    }

    fn binary(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claude",
            ToolKind::Codex => "codex",
            ToolKind::GeminiCli => "gemini",
            ToolKind::GrokBuild => "grok",
            ToolKind::OpenCode => "opencode",
            ToolKind::DeepseekHarness => "dsh",
            ToolKind::Pi => "pi",
            ToolKind::OhMyPi => "omp",
            ToolKind::KimiCode => "kimi",
            ToolKind::MinimaxCode => "mcode",
            ToolKind::OpenClaw => "openclaw",
            ToolKind::Antigravity => "agy",
        }
    }

    /// 和前端一致的 ID，也用在 `~/.transitlink/tools.json` 里。
    pub fn id(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claudeCode",
            ToolKind::Codex => "codex",
            ToolKind::GeminiCli => "geminiCli",
            ToolKind::GrokBuild => "grokBuild",
            ToolKind::OpenCode => "opencode",
            ToolKind::DeepseekHarness => "deepseekHarness",
            ToolKind::Pi => "pi",
            ToolKind::OhMyPi => "ohMyPi",
            ToolKind::KimiCode => "kimiCode",
            ToolKind::MinimaxCode => "minimaxCode",
            ToolKind::OpenClaw => "openclaw",
            ToolKind::Antigravity => "antigravity",
        }
    }

    /// 备份目录名：`~/.transitlink/backups/<名字>`。
    pub fn backup_dir(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claude-code",
            ToolKind::Codex => "codex",
            ToolKind::GeminiCli => "gemini-cli",
            ToolKind::GrokBuild => "grok-build",
            ToolKind::OpenCode => "opencode",
            ToolKind::DeepseekHarness => "deepseek-harness",
            ToolKind::Pi => "pi",
            ToolKind::OhMyPi => "oh-my-pi",
            ToolKind::KimiCode => "kimi-code",
            ToolKind::MinimaxCode => "minimax-code",
            ToolKind::OpenClaw => "openclaw",
            ToolKind::Antigravity => "antigravity",
        }
    }

    /// 工具的名字（通知里用；界面上的名字在前端的语言包里）。
    pub fn display_name(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "Claude Code",
            ToolKind::Codex => "Codex",
            ToolKind::GeminiCli => "Gemini CLI",
            ToolKind::GrokBuild => "Grok Build",
            ToolKind::OpenCode => "OpenCode",
            ToolKind::DeepseekHarness => "DeepSeek Harness",
            ToolKind::Pi => "Pi",
            ToolKind::OhMyPi => "Oh My Pi",
            ToolKind::KimiCode => "Kimi Code",
            ToolKind::MinimaxCode => "MiniMax Code",
            ToolKind::OpenClaw => "OpenClaw",
            ToolKind::Antigravity => "Antigravity CLI",
        }
    }

    /// 一键配置时给这个工具建的专用 Key 的名字。
    pub fn key_name(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "TransitLink Claude Code",
            ToolKind::Codex => "TransitLink Codex",
            ToolKind::GeminiCli => "TransitLink Gemini CLI",
            ToolKind::GrokBuild => "TransitLink Grok Build",
            ToolKind::OpenCode => "TransitLink OpenCode",
            ToolKind::DeepseekHarness => "TransitLink DeepSeek Harness",
            ToolKind::Pi => "TransitLink Pi",
            ToolKind::OhMyPi => "TransitLink Oh My Pi",
            ToolKind::KimiCode => "TransitLink Kimi Code",
            ToolKind::MinimaxCode => "TransitLink MiniMax Code",
            ToolKind::OpenClaw => "TransitLink OpenClaw",
            ToolKind::Antigravity => "TransitLink Antigravity",
        }
    }

    /// 配置文件位置（都支持用环境变量换目录）：
    /// - Claude Code：`CLAUDE_CONFIG_DIR` 或 `~/.claude`，`settings.json`；
    /// - Codex：`CODEX_HOME` 或 `~/.codex`，`config.toml`；
    /// - Gemini CLI：`GEMINI_CLI_HOME`（代替主目录）或主目录下的 `.gemini/.env`；
    /// - Grok Build：`GROK_HOME` 或 `~/.grok`，`config.toml`；
    /// - OpenCode：`XDG_CONFIG_HOME` 或 `~/.config` 下的 `opencode/`，已有 `opencode.jsonc` 就用它
    ///   （OpenCode 第一次运行会建这个文件，而且它优先），否则 `opencode.json`；
    /// - DeepSeek Harness：`DSH_HOME` 或 `~/.dsh` 下 `web` 配置档的 `cordis.patch.yml`（还会改
    ///   `desktop` 配置档和 `.credentials.yaml`，见 `configure::dsh`）。
    pub fn config_path(self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
        let dir = |variable: &str, default: PathBuf| {
            env(variable)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or(default)
        };
        match self {
            ToolKind::ClaudeCode => {
                dir("CLAUDE_CONFIG_DIR", home.join(".claude")).join("settings.json")
            }
            ToolKind::Codex => dir("CODEX_HOME", home.join(".codex")).join("config.toml"),
            ToolKind::GeminiCli => dir("GEMINI_CLI_HOME", home.to_path_buf())
                .join(".gemini")
                .join(".env"),
            ToolKind::GrokBuild => dir("GROK_HOME", home.join(".grok")).join("config.toml"),
            ToolKind::OpenCode => {
                let base = dir("XDG_CONFIG_HOME", home.join(".config")).join("opencode");
                let jsonc = base.join("opencode.jsonc");
                if jsonc.is_file() {
                    jsonc
                } else {
                    base.join("opencode.json")
                }
            }
            ToolKind::DeepseekHarness => {
                crate::configure::dsh::profile_path(&crate::configure::dsh::home(home, env), "web")
            }
            // 只显示用：Antigravity CLI 没法一键连接站点。
            ToolKind::Antigravity => dir("GEMINI_CLI_HOME", home.to_path_buf())
                .join(".gemini")
                .join("antigravity-cli")
                .join("settings.json"),
            // 几个文件一起改的工具：显示第一个文件。
            _ => crate::configure::multi(self)
                .and_then(|adapter: &dyn crate::configure::multi::MultiFile| {
                    adapter.files(home, env).into_iter().next()
                })
                .map(|(path, _)| path)
                .unwrap_or_else(|| home.to_path_buf()),
        }
    }
}

/// Gemini CLI 的 `settings.json`（和 `.env` 在同一个目录）：一键配置时把登录方式设成 API Key。
pub fn gemini_settings_path(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let env_file = ToolKind::GeminiCli.config_path(home, env);
    env_file.parent().unwrap_or(home).join("settings.json")
}

/// Claude Code 的用户状态文件 `.claude.json`（记录是否完成首次引导等）：默认在主目录下；
/// 用 `CLAUDE_CONFIG_DIR` 换了目录时在那个目录里。
pub fn claude_user_config_path(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let settings = ToolKind::ClaudeCode.config_path(home, env);
    let dir = settings.parent().unwrap_or(home);
    if dir == home.join(".claude") {
        home.join(".claude.json")
    } else {
        dir.join(".claude.json")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub kind: ToolKind,
    pub installed: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub config_path: String,
    pub config_exists: bool,
    /// 官方安装命令（界面上一键安装旁边显示，也可以复制到终端里自己运行）。
    pub install_command: String,
    /// 用 npm 安装（需要 Node.js，可以换国内镜像）。
    pub install_npm: bool,
    /// 能一键连接站点（Antigravity CLI 不能）。
    pub configurable: bool,
}

/// 检测所有支持的工具。
pub fn detect_all() -> Vec<ToolStatus> {
    let home = crate::paths::home_dir();
    let env = |key: &str| std::env::var(key).ok();
    let search = search_path(&home, std::env::var_os("PATH"));
    ToolKind::ALL
        .iter()
        .map(|kind| detect(*kind, &home, &env, &search))
        .collect()
}

/// 检测一个工具（装完以后重新检测用）。
pub fn detect_one(kind: ToolKind) -> ToolStatus {
    let home = crate::paths::home_dir();
    let env = |key: &str| std::env::var(key).ok();
    let search = search_path(&home, std::env::var_os("PATH"));
    detect(kind, &home, &env, &search)
}

/// 用户在终端里能不能直接运行这个目录里的命令：看登录 shell 的 PATH（Windows 看用户和系统的
/// Path 环境变量）。读不到时当作能，免得误报。
pub fn on_shell_path(home: &Path, dir: &Path) -> bool {
    let _ = home;
    match shell_path() {
        Some(path) => std::env::split_paths(&path).any(|entry| entry == dir),
        None => true,
    }
}

/// 用户在终端里能看到的这几个环境变量（设成空字符串的也算设置了）。macOS 上从程序坞打开的应用拿不到终端里设置的
/// 变量，所以跑一次登录 shell 读；整份环境变量只在内存里挑出要的这几个，不保存、不记录。
/// Windows 上从桌面启动的程序本来就继承用户的环境变量，直接读本进程的。读不到时返回 None（宁可漏报）。
pub fn shell_env(names: &[&str]) -> Option<std::collections::HashMap<String, String>> {
    #[cfg(windows)]
    {
        Some(
            names
                .iter()
                .filter_map(|name| {
                    std::env::var(name)
                        .ok()
                        .map(|value| (name.to_string(), value))
                })
                .collect(),
        )
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/bin/zsh".into()
            } else {
                "/bin/bash".into()
            }
        });
        let mut command = Command::new(shell);
        // 只用 echo 和 env：bash、zsh、fish 都认。`env -0` 用 NUL 分隔，值里有换行也不会读错。
        command.args(["-ilc", "echo __TL_ENV__; env -0"]);
        let output = run_quietly(command, Duration::from_secs(5))?;
        parse_env_listing(&String::from_utf8_lossy(&output), names)
    }
}

/// 从 `echo __TL_ENV__; env -0` 的输出里挑出要的变量（标记前面是登录时 shell 自己打印的东西）。
#[cfg_attr(windows, allow(dead_code))]
fn parse_env_listing(
    text: &str,
    names: &[&str],
) -> Option<std::collections::HashMap<String, String>> {
    let (_, listing) = text.rsplit_once("__TL_ENV__")?;
    Some(
        listing
            .split('\0')
            .map(|entry| entry.trim_start_matches(['\r', '\n']))
            .filter_map(|entry| entry.split_once('='))
            .filter(|(name, _)| names.contains(name))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    )
}

/// 在常见的终端配置文件里找设置了这个变量的行（「~/.zshrc:12」），告诉用户去哪里改。
pub fn rc_locations(home: &Path, name: &str) -> Vec<String> {
    const FILES: [&str; 7] = [
        ".zshrc",
        ".zprofile",
        ".zshenv",
        ".bashrc",
        ".bash_profile",
        ".profile",
        ".config/fish/config.fish",
    ];
    let mut found = Vec::new();
    for file in FILES {
        let Ok(text) = std::fs::read_to_string(home.join(file)) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let line = line.trim_start();
            if line.starts_with('#') {
                continue;
            }
            let sets = line.starts_with(&format!("{name}="))
                || line.contains(&format!("export {name}="))
                || line.contains(&format!("set -gx {name} "))
                || line.contains(&format!("set -x {name} "));
            if sets {
                found.push(format!("~/{file}:{}", index + 1));
            }
        }
    }
    found
}

#[cfg(not(windows))]
fn shell_path() -> Option<OsString> {
    // 从桌面启动的应用可能没有 SHELL：macOS 默认 zsh。
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    });
    let mut command = Command::new(shell);
    // -i -l：和打开一个新终端一样读 .zshrc / .bash_profile。
    command.args(["-ilc", "printf '__TL_PATH__%s' \"$PATH\""]);
    let output = run_quietly(command, Duration::from_secs(5))?;
    let text = String::from_utf8_lossy(&output);
    let (_, path) = text.rsplit_once("__TL_PATH__")?;
    Some(OsString::from(path.trim()))
}

#[cfg(windows)]
fn shell_path() -> Option<OsString> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine')",
    ]);
    let output = run_quietly(command, Duration::from_secs(10))?;
    Some(OsString::from(String::from_utf8_lossy(&output).trim()))
}

/// 运行命令，最多等 `limit`，返回标准输出。
fn run_quietly(mut command: Command, limit: Duration) -> Option<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // 边读边存：shell 启动的后台程序可能一直占着输出（读不到结尾），这时用已经读到的。
    let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (done, finished) = std::sync::mpsc::channel::<()>();
    {
        let buffer = buffer.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => buffer
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .extend_from_slice(&chunk[..read]),
                }
            }
            let _ = done.send(());
        });
    }
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    // 命令结束了，再给输出一点时间读完。
    let _ = finished.recv_timeout(Duration::from_millis(500));
    let output = buffer
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    Some(output)
}

/// 找命令。老的 Python 版 kimi-cli 也叫 `kimi`：跳过它（和 Kimi 官方安装脚本一样，看真实文件开头
/// 4 KB 里有没有 `kimi_cli`；Windows 上 uv 生成的小 .exe 启动器把脚本路径写在末尾，整个看）。
fn find_binary(kind: ToolKind, search: &OsString, home: &Path) -> Option<PathBuf> {
    if kind != ToolKind::KimiCode {
        return which::which_in(kind.binary(), Some(search), home).ok();
    }
    which::which_in_all(kind.binary(), Some(search), home)
        .ok()?
        .find(|path| !legacy_kimi(path))
}

fn legacy_kimi(path: &Path) -> bool {
    use std::io::Read;
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let Ok(file) = std::fs::File::open(&real) else {
        return false;
    };
    let size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let limit = if cfg!(windows) && size <= 1 << 20 {
        size
    } else {
        4096
    };
    let mut head = Vec::new();
    if file.take(limit).read_to_end(&mut head).is_err() {
        return false;
    }
    head.windows(b"kimi_cli".len())
        .any(|window| window == b"kimi_cli")
}

fn detect(
    kind: ToolKind,
    home: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    search: &OsString,
) -> ToolStatus {
    let binary = find_binary(kind, search, home);
    let version = binary
        .as_deref()
        .and_then(|path| read_version(path, search));
    let config = kind.config_path(home, env);
    ToolStatus {
        kind,
        installed: binary.is_some(),
        binary_path: binary.map(|path| path.display().to_string()),
        version,
        config_exists: config.is_file(),
        config_path: config.display().to_string(),
        install_command: crate::install::display_command(crate::install::method(kind), false),
        install_npm: matches!(
            crate::install::method(kind),
            crate::install::Method::Npm { .. }
        ),
        configurable: kind.configurable(),
    }
}

/// 查找命令用的 PATH。从桌面图标启动的应用拿不到终端里的 PATH（macOS 上尤其如此），
/// 所以补上 npm 全局目录、Homebrew、Claude Code 安装脚本等常见位置；nvm 的每个 Node 版本也加上。
pub fn search_path(home: &Path, inherited: Option<OsString>) -> OsString {
    let mut dirs: Vec<PathBuf> = inherited
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    let mut extra = vec![
        home.join(".local/bin"),
        home.join(".claude/local"),
        // Grok Build、OpenCode、Pi、Kimi Code、MiniMax Code、OpenClaw 官方安装脚本的位置。
        home.join(".grok/bin"),
        home.join(".opencode/bin"),
        home.join(".pi/agent/bin"),
        home.join(".kimi-code/bin"),
        home.join(".minimax-code/bin"),
        home.join(".openclaw/bin"),
        home.join(".npm-global/bin"),
        home.join(".volta/bin"),
        home.join(".bun/bin"),
    ];
    #[cfg(unix)]
    extra.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            extra.push(PathBuf::from(appdata).join("npm"));
        }
        // Codex、Oh My Pi、Antigravity CLI 官方安装脚本在 Windows 上的安装位置。
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            extra.push(local.join("Programs\\OpenAI\\Codex\\bin"));
            extra.push(local.join("omp"));
            extra.push(local.join("agy\\bin"));
        }
        // MiniMax Code 在 Windows 上的启动脚本在安装目录本身。
        extra.push(home.join(".minimax-code"));
    }
    if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path().join("bin"))
            .collect();
        versions.sort();
        // 新版本在前。
        extra.extend(versions.into_iter().rev());
    }
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// 运行 `<命令> --version`，最多等 5 秒。子进程用同一份 PATH，保证 npm 安装的命令能找到 node。
fn read_version(binary: &Path, search: &OsString) -> Option<String> {
    let mut command = Command::new(binary);
    command
        .arg("--version")
        .env("PATH", search)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 不弹出控制台窗口。
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    parse_version(&output)
}

/// 取输出里的第一个版本号，例如 "2.0.14 (Claude Code)" → "2.0.14"，"codex-cli 0.149.0" → "0.149.0"。
pub fn parse_version(output: &str) -> Option<String> {
    output
        // Oh My Pi 打印 `omp/18.16.0`，OpenClaw 打印 `OpenClaw 2026.11.1 (4ffdd20)`。
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '/'))
        .map(|token| token.trim_start_matches('v'))
        .find(|token| {
            let mut parts = token.split('.');
            let major = parts.next().unwrap_or_default();
            let minor = parts.next().unwrap_or_default();
            !major.is_empty()
                && major.chars().all(|c| c.is_ascii_digit())
                && minor.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_wanted_variables_after_the_marker() {
        let output = "Welcome!\nHTTPS_PROXY=fake-banner\n__TL_ENV__\nPATH=/bin\0HTTPS_PROXY=http://127.0.0.1:7890\0NOTE=line one\nCODEX_HOME=/not/real\0OMP_PROFILE=\0";
        let found =
            parse_env_listing(output, &["HTTPS_PROXY", "CODEX_HOME", "OMP_PROFILE"]).unwrap();
        assert_eq!(
            found.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            found.get("CODEX_HOME"),
            None,
            "a newline inside a value is not a new variable"
        );
        assert_eq!(
            found.get("OMP_PROFILE").map(String::as_str),
            Some(""),
            "set but empty still counts"
        );
        assert_eq!(parse_env_listing("no marker", &["PATH"]), None);
    }

    #[test]
    fn parses_versions_from_cli_output() {
        assert_eq!(
            parse_version("2.0.14 (Claude Code)\n").as_deref(),
            Some("2.0.14")
        );
        assert_eq!(
            parse_version("codex-cli 0.149.0").as_deref(),
            Some("0.149.0")
        );
        assert_eq!(
            parse_version("codex-cli 0.150.0-alpha.2").as_deref(),
            Some("0.150.0-alpha.2")
        );
        assert_eq!(parse_version("v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version("build 7"), None);
    }

    #[test]
    fn config_paths_follow_environment_overrides() {
        let home = Path::new("/home/test");
        let none = |_: &str| None;
        assert_eq!(
            ToolKind::ClaudeCode.config_path(home, &none),
            home.join(".claude/settings.json")
        );
        assert_eq!(
            ToolKind::Codex.config_path(home, &none),
            home.join(".codex/config.toml")
        );
        assert_eq!(
            ToolKind::GeminiCli.config_path(home, &none),
            home.join(".gemini/.env")
        );
        assert_eq!(
            gemini_settings_path(home, &none),
            home.join(".gemini/settings.json")
        );
        assert_eq!(
            ToolKind::GrokBuild.config_path(home, &none),
            home.join(".grok/config.toml")
        );
        assert_eq!(
            ToolKind::OpenCode.config_path(home, &none),
            home.join(".config/opencode/opencode.json")
        );
        let moved = |key: &str| match key {
            "GEMINI_CLI_HOME" => Some("/data/g".to_string()),
            "GROK_HOME" => Some("/data/grok".to_string()),
            "XDG_CONFIG_HOME" => Some("/data/xdg".to_string()),
            _ => None,
        };
        assert_eq!(
            ToolKind::GeminiCli.config_path(home, &moved),
            PathBuf::from("/data/g/.gemini/.env")
        );
        assert_eq!(
            ToolKind::GrokBuild.config_path(home, &moved),
            PathBuf::from("/data/grok/config.toml")
        );
        assert_eq!(
            ToolKind::OpenCode.config_path(home, &moved),
            PathBuf::from("/data/xdg/opencode/opencode.json")
        );

        let custom = |key: &str| match key {
            "CLAUDE_CONFIG_DIR" => Some("/data/claude".to_string()),
            "CODEX_HOME" => Some("  ".to_string()),
            _ => None,
        };
        assert_eq!(
            ToolKind::ClaudeCode.config_path(home, &custom),
            PathBuf::from("/data/claude/settings.json")
        );
        assert_eq!(
            ToolKind::Codex.config_path(home, &custom),
            home.join(".codex/config.toml"),
            "blank overrides are ignored"
        );

        assert_eq!(
            claude_user_config_path(home, &none),
            home.join(".claude.json")
        );
        assert_eq!(
            claude_user_config_path(home, &custom),
            PathBuf::from("/data/claude/.claude.json")
        );
    }

    #[test]
    fn search_path_adds_common_install_locations_once() {
        let home = std::env::temp_dir().join(format!("transitlink-search-{}", std::process::id()));
        let nvm = home.join(".nvm/versions/node");
        std::fs::create_dir_all(nvm.join("v20.1.0/bin")).unwrap();
        std::fs::create_dir_all(nvm.join("v22.3.0/bin")).unwrap();
        let inherited =
            std::env::join_paths([home.join(".local/bin"), PathBuf::from("/usr/bin")]).unwrap();

        let joined = search_path(&home, Some(inherited));
        let dirs: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        std::fs::remove_dir_all(&home).unwrap();

        assert_eq!(
            dirs[0],
            home.join(".local/bin"),
            "inherited PATH keeps its order"
        );
        assert_eq!(
            dirs.iter()
                .filter(|dir| **dir == home.join(".local/bin"))
                .count(),
            1,
            "no duplicates"
        );
        assert!(dirs.contains(&home.join(".npm-global/bin")));
        let newer = dirs
            .iter()
            .position(|dir| *dir == nvm.join("v22.3.0/bin"))
            .unwrap();
        let older = dirs
            .iter()
            .position(|dir| *dir == nvm.join("v20.1.0/bin"))
            .unwrap();
        assert!(newer < older, "newer nvm versions come first");
    }

    #[test]
    fn finds_binaries_on_the_search_path() {
        let dir = std::env::temp_dir().join(format!("transitlink-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join(if cfg!(windows) { "codex.cmd" } else { "codex" });
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&script, "#!/bin/sh\necho 'codex-cli 0.149.0'\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        std::fs::write(&script, "@echo codex-cli 0.149.0\r\n").unwrap();

        let search = std::env::join_paths([dir.clone()]).unwrap();
        let status = detect(ToolKind::Codex, &dir, &|_: &str| None, &search);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(status.installed);
        assert_eq!(status.version.as_deref(), Some("0.149.0"));
        assert!(!status.config_exists);
    }

    #[cfg(unix)]
    #[test]
    fn the_old_python_kimi_cli_is_not_kimi_code() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("transitlink-kimi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (legacy, native) = (dir.join("uv-bin"), dir.join("kimi-code-bin"));
        for (folder, text) in [
            (
                &legacy,
                "#!/usr/bin/env python3\nfrom kimi_cli.cli import main\nmain()\n",
            ),
            (&native, "#!/bin/sh\necho 'kimi 2.5.1'\n"),
        ] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("kimi"), text).unwrap();
            std::fs::set_permissions(folder.join("kimi"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let both = std::env::join_paths([legacy.clone(), native.clone()]).unwrap();
        assert_eq!(
            find_binary(ToolKind::KimiCode, &both, &dir),
            Some(native.join("kimi"))
        );
        let only_legacy = std::env::join_paths([legacy.clone()]).unwrap();
        assert_eq!(find_binary(ToolKind::KimiCode, &only_legacy, &dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
