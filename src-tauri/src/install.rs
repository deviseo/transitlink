//! 一键安装编程工具：运行工具官方的安装脚本，或者用 npm 全局安装。安装过程的输出一行行发给界面
//! （事件 `tool-install`）。
//!
//! - 安装命令都是固定的，不拼接任何用户输入。
//! - 跟随系统代理：系统设了代理时，把代理地址作为 `HTTPS_PROXY`、`HTTP_PROXY` 交给安装命令
//!   （用户自己设了这些环境变量的不覆盖），这样国内也能下载官方安装包。
//! - npm 可以改用国内镜像。
//! - 同一时间只装一个，最多等 15 分钟。

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::tools::{self, ToolKind};

const TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// npm 国内镜像。
pub const NPM_MIRROR: &str = "https://registry.npmmirror.com";
/// 留着分析失败原因的最后几行输出。
const KEEP_LINES: usize = 200;
/// 一行最多给界面这么多字符（进度条之类的长行截断）。
const MAX_LINE_CHARS: usize = 400;

/// 安装方式（按当前系统选好的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// macOS / Linux：curl 下载官方安装脚本，交给 `shell` 运行。
    Unix {
        url: &'static str,
        shell: &'static str,
    },
    /// Windows：PowerShell 运行官方安装脚本。
    PowerShell { url: &'static str },
    /// npm 全局安装，需要 Node.js（至少 `node_major` 版）。`extra`：额外参数。
    Npm {
        package: &'static str,
        node_major: u32,
        extra: &'static [&'static str],
    },
}

/// npm 12 默认拦截依赖包的安装脚本：靠安装脚本放好程序的包（OpenCode 等）要明确允许，
/// 否则装完只有一个占位文件（做法参考 cc-switch）。npm 10、11 上这些参数没有副作用。
const NPM_ALLOW_SCRIPTS_OPENCODE: &[&str] = &[
    "--ignore-scripts=false",
    "--include=optional",
    "--allow-scripts=opencode-ai",
];

/// 每个工具在当前系统上的官方安装方式：
/// - Claude Code、Codex、Grok Build：官方安装脚本，不需要 Node.js；
/// - Gemini CLI：只有 npm（Homebrew 的版本已经停止更新）；
/// - OpenCode：macOS / Linux 用官方安装脚本；Windows 没有 PowerShell 安装脚本，用 npm。
pub fn method(kind: ToolKind) -> Method {
    let windows = cfg!(windows);
    match kind {
        ToolKind::ClaudeCode if windows => Method::PowerShell {
            url: "https://claude.ai/install.ps1",
        },
        ToolKind::ClaudeCode => Method::Unix {
            url: "https://claude.ai/install.sh",
            shell: "bash",
        },
        ToolKind::Codex if windows => Method::PowerShell {
            url: "https://chatgpt.com/codex/install.ps1",
        },
        ToolKind::Codex => Method::Unix {
            url: "https://chatgpt.com/codex/install.sh",
            shell: "sh",
        },
        ToolKind::GrokBuild if windows => Method::PowerShell {
            url: "https://x.ai/cli/install.ps1",
        },
        ToolKind::GrokBuild => Method::Unix {
            url: "https://x.ai/cli/install.sh",
            shell: "bash",
        },
        ToolKind::GeminiCli => Method::Npm {
            package: "@google/gemini-cli",
            node_major: 20,
            extra: &[],
        },
        ToolKind::OpenCode if windows => Method::Npm {
            package: "opencode-ai",
            node_major: 18,
            extra: NPM_ALLOW_SCRIPTS_OPENCODE,
        },
        // 现在的包没有声明 Node 版本要求（装之前我们自己检查）；以后声明了，版本不对时 npm 直接拒绝。
        ToolKind::DeepseekHarness => Method::Npm {
            package: "@deepseek-ai/dsh",
            node_major: 22,
            extra: &["--engine-strict"],
        },
        ToolKind::OpenCode => Method::Unix {
            url: "https://opencode.ai/install",
            shell: "bash",
        },
    }
}

/// 安装命令（给界面显示，也给装不上时让用户自己在终端里运行）。
pub fn display_command(method: Method, mirror: bool) -> String {
    match method {
        Method::Unix { url, shell } => format!("curl -fsSL {url} | {shell}"),
        Method::PowerShell { url } => format!("irm {url} | iex"),
        Method::Npm { package, extra, .. } => npm_args(package, extra, mirror).join(" "),
    }
}

fn npm_args(package: &str, extra: &[&str], mirror: bool) -> Vec<String> {
    let mut args = vec!["npm".into(), "install".into(), "-g".into(), package.into()];
    args.extend(extra.iter().map(|arg| arg.to_string()));
    if mirror {
        args.push(format!("--registry={NPM_MIRROR}"));
    }
    args
}

/// 安装过程中的一行输出。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallLine {
    pub kind: ToolKind,
    pub line: String,
}

/// 安装结果。失败时带上命令，界面上可以让用户复制到终端里自己运行。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum InstallOutcome {
    /// 装好了。`path_hint`：终端里还找不到这个命令时，命令所在的目录（要加到 PATH 里）。
    #[serde(rename_all = "camelCase")]
    Installed {
        version: Option<String>,
        binary_path: Option<String>,
        path_hint: Option<String>,
    },
    /// 要先装 Node.js（npm 安装的工具）。`found`：现在的版本，没装时为空。
    #[serde(rename_all = "camelCase")]
    NeedsNode {
        required: u32,
        found: Option<String>,
    },
    /// npm 的全局目录没有写权限（例如用官方安装包装的 Node）。
    NpmPermission { command: String },
    /// 网络问题：下载失败或连不上。
    Network { command: String },
    /// 超时。
    TimedOut { command: String },
    /// 其他失败。
    Failed { code: Option<i32>, command: String },
    /// 这里没法升级：`command` 是请用户在终端里运行的命令（pnpm、bun 等装的）；为空时是桌面版带的，
    /// 请在桌面版里更新。
    Manual { command: Option<String> },
}

/// 同一时间只装一个。
#[derive(Default)]
pub struct Installer {
    lock: tokio::sync::Mutex<()>,
}

/// 已经有一个在装了。
#[derive(Debug)]
pub struct Busy;

impl Installer {
    pub async fn install(
        &self,
        app: &AppHandle,
        kind: ToolKind,
        mirror: bool,
    ) -> Result<InstallOutcome, Busy> {
        let Ok(_guard) = self.lock.try_lock() else {
            return Err(Busy);
        };
        let home = crate::paths::home_dir();
        let search = tools::search_path(&home, std::env::var_os("PATH"));
        let method = method(kind);
        let display = display_command(method, mirror);
        let command = match prepare(method, &home, &search, mirror).await {
            Ok(command) => command,
            Err(outcome) => return Ok(outcome),
        };
        let npm = matches!(method, Method::Npm { .. });
        Ok(execute(app, kind, command, display, npm, &search, home).await)
    }

    /// 升级：按工具是怎么装的选办法（见 `upgrade`）。`target`：要升到的版本（查到的最新版）。
    pub async fn upgrade(
        &self,
        app: &AppHandle,
        kind: ToolKind,
        mirror: bool,
        target: Option<&str>,
    ) -> Result<InstallOutcome, Busy> {
        let Ok(_guard) = self.lock.try_lock() else {
            return Err(Busy);
        };
        let home = crate::paths::home_dir();
        let search = tools::search_path(&home, std::env::var_os("PATH"));
        let status = tauri::async_runtime::spawn_blocking(move || tools::detect_one(kind))
            .await
            .ok();
        // 检测不到（例如刚被卸载）：按失败处理，界面会重新检测。
        let not_upgradable = InstallOutcome::Failed {
            code: None,
            command: String::new(),
        };
        let Some((binary, version)) = status.and_then(|status| {
            status
                .binary_path
                .map(|binary| (std::path::PathBuf::from(binary), status.version))
        }) else {
            return Ok(not_upgradable);
        };
        // 命令所在的目录放在 PATH 最前面：`#!/usr/bin/env node` 要找到同一处的 node，npm 也先找同一处的。
        let mut dirs: Vec<std::path::PathBuf> =
            binary.parent().map(Path::to_path_buf).into_iter().collect();
        dirs.extend(std::env::split_paths(&search));
        let search = std::env::join_paths(dirs).unwrap_or(search);
        let plan = crate::upgrade::plan(
            kind,
            &binary,
            version.as_deref(),
            target,
            mirror,
            &search,
            &home,
        );
        let (command, display, npm) = match plan {
            None => return Ok(not_upgradable),
            Some(crate::upgrade::Plan::Manual { command }) => {
                return Ok(InstallOutcome::Manual { command });
            }
            Some(crate::upgrade::Plan::Reinstall(reinstall)) => {
                let command = match prepare(reinstall, &home, &search, mirror).await {
                    Ok(command) => command,
                    Err(outcome) => return Ok(outcome),
                };
                let npm = matches!(reinstall, Method::Npm { .. });
                (command, display_command(reinstall, mirror), npm)
            }
            Some(crate::upgrade::Plan::Run {
                program,
                args,
                display,
                npm,
            }) => {
                // 只能用 npm 装的工具，升级前也确认 Node.js 版本够：新版装上了却运行不了更麻烦。
                if npm && let Method::Npm { node_major, .. } = method(kind) {
                    let found = node_version(&home, &search).await;
                    if found
                        .as_deref()
                        .and_then(major_version)
                        .is_none_or(|major| major < node_major)
                    {
                        return Ok(InstallOutcome::NeedsNode {
                            required: node_major,
                            found,
                        });
                    }
                }
                let mut command = Command::new(program);
                command.args(args);
                (command, display, npm)
            }
        };
        Ok(execute(app, kind, command, display, npm, &search, home).await)
    }
}

/// 运行安装或升级命令，把输出发给界面，结束后重新检测（或者分析失败原因；`npm`：是 npm 命令）。
async fn execute(
    app: &AppHandle,
    kind: ToolKind,
    mut command: Command,
    display: String,
    npm: bool,
    search: &OsString,
    home: std::path::PathBuf,
) -> InstallOutcome {
    command.env("PATH", search);
    for (key, value) in proxy_env(|key| std::env::var_os(key).is_some()) {
        command.env(key, value);
    }
    emit(app, kind, &format!("$ {display}"));
    let finished = run(command, |line| emit(app, kind, line)).await;
    match finished {
        Run::TimedOut => InstallOutcome::TimedOut { command: display },
        Run::Spawn => InstallOutcome::Failed {
            code: None,
            command: display,
        },
        Run::Done { code, lines } => {
            if code == Some(0) {
                // 重新检测要运行 --version、读终端的 PATH，放到后台线程里。
                tauri::async_runtime::spawn_blocking(move || installed(kind, &home))
                    .await
                    .unwrap_or(InstallOutcome::Installed {
                        version: None,
                        binary_path: None,
                        path_hint: None,
                    })
            } else {
                failure(npm, code, &lines, display)
            }
        }
    }
}

fn emit(app: &AppHandle, kind: ToolKind, line: &str) {
    let _ = app.emit(
        "tool-install",
        InstallLine {
            kind,
            line: line.to_string(),
        },
    );
}

/// 准备安装命令。npm 安装的工具先确认有 Node.js 和 npm，版本也够。
async fn prepare(
    method: Method,
    home: &Path,
    search: &OsString,
    mirror: bool,
) -> Result<Command, InstallOutcome> {
    match method {
        Method::PowerShell { url } => {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!("irm {url} | iex"),
            ]);
            Ok(command)
        }
        Method::Unix { url, shell } => {
            // pipefail：下载失败时整条命令算失败，不会把空内容交给 shell。
            let mut command = Command::new("/bin/bash");
            command.args([
                "-c",
                &format!("set -o pipefail; curl -fsSL {url} | {shell}"),
            ]);
            Ok(command)
        }
        Method::Npm {
            package,
            node_major,
            extra,
        } => {
            let found = node_version(home, search).await;
            if found
                .as_deref()
                .and_then(major_version)
                .is_none_or(|major| major < node_major)
            {
                return Err(InstallOutcome::NeedsNode {
                    required: node_major,
                    found,
                });
            }
            let npm = which::which_in("npm", Some(search), home).map_err(|_| {
                InstallOutcome::NeedsNode {
                    required: node_major,
                    found: None,
                }
            })?;
            let mut command = Command::new(npm);
            command.args(&npm_args(package, extra, mirror)[1..]);
            Ok(command)
        }
    }
}

/// `node --version` 的输出，例如 `v22.11.0`。
async fn node_version(home: &Path, search: &OsString) -> Option<String> {
    let node = which::which_in("node", Some(search), home).ok()?;
    let mut command = Command::new(node);
    command
        .arg("--version")
        .env("PATH", search)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_window(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// `v22.11.0` → 22。
fn major_version(version: &str) -> Option<u32> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// 系统代理（和桌面端访问站点用的一样）交给安装命令。`already_set`：用户自己设了的变量不覆盖。
fn proxy_env(already_set: impl Fn(&str) -> bool) -> Vec<(&'static str, String)> {
    let custom = [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .any(|key| already_set(key));
    if custom {
        return Vec::new();
    }
    let matcher = hyper_util::client::proxy::matcher::Matcher::from_system();
    let mut vars = Vec::new();
    for (scheme, upper, lower) in [
        ("https://example.com", "HTTPS_PROXY", "https_proxy"),
        ("http://example.com", "HTTP_PROXY", "http_proxy"),
    ] {
        let Ok(uri) = scheme.parse::<http::Uri>() else {
            continue;
        };
        if let Some(intercept) = matcher.intercept(&uri) {
            let proxy = intercept.uri().to_string();
            vars.push((upper, proxy.clone()));
            vars.push((lower, proxy));
        }
    }
    vars
}

fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        // 不弹出控制台窗口。
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

enum Run {
    /// 进程结束：退出码和最后几行输出。
    Done {
        code: Option<i32>,
        lines: Vec<String>,
    },
    /// 启动不了。
    Spawn,
    TimedOut,
}

/// 运行命令，把输出一行行交给 `on_line`。
async fn run(mut command: Command, on_line: impl Fn(&str) + Send + Sync) -> Run {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_window(&mut command);
    let Ok(mut child) = command.spawn() else {
        return Run::Spawn;
    };
    let lines = Arc::new(Mutex::new(Vec::new()));
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let work = async {
        tokio::join!(
            pump(stdout, &lines, &on_line),
            pump(stderr, &lines, &on_line)
        );
        child.wait().await
    };
    match tokio::time::timeout(TIMEOUT, work).await {
        Err(_) => {
            let _ = child.start_kill();
            Run::TimedOut
        }
        Ok(status) => {
            let lines = lines.lock().map(|lines| lines.clone()).unwrap_or_default();
            Run::Done {
                code: status.ok().and_then(|status| status.code()),
                lines,
            }
        }
    }
}

/// 读一个输出流，按行（\n 或 \r）切开、去掉颜色控制符后交给 `on_line`，并留下最后几行。
async fn pump(
    stream: Option<impl AsyncRead + Unpin>,
    kept: &Mutex<Vec<String>>,
    on_line: &(impl Fn(&str) + Send + Sync),
) {
    let Some(mut stream) = stream else {
        return;
    };
    let mut pending = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        pending.extend_from_slice(&buffer[..read]);
        while let Some(end) = pending
            .iter()
            .position(|byte| *byte == b'\n' || *byte == b'\r')
        {
            let raw: Vec<u8> = pending.drain(..=end).collect();
            deliver(&raw, kept, on_line);
        }
    }
    deliver(&pending, kept, on_line);
}

fn deliver(raw: &[u8], kept: &Mutex<Vec<String>>, on_line: &(impl Fn(&str) + Send + Sync)) {
    let Some(line) = clean_line(&String::from_utf8_lossy(raw)) else {
        return;
    };
    on_line(&line);
    if let Ok(mut kept) = kept.lock() {
        kept.push(line);
        let overflow = kept.len().saturating_sub(KEEP_LINES);
        kept.drain(..overflow);
    }
}

/// 去掉颜色等控制符和首尾空白，太长的截断；空行返回 None。
fn clean_line(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ ... 字母：颜色、光标移动等。
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    let line = out.trim();
    if line.is_empty() {
        return None;
    }
    Some(line.chars().take(MAX_LINE_CHARS).collect())
}

/// 安装命令成功后重新检测：版本、位置，终端里能不能找到。
fn installed(kind: ToolKind, home: &Path) -> InstallOutcome {
    let status = tools::detect_one(kind);
    let path_hint = status
        .binary_path
        .as_deref()
        .and_then(|binary| Path::new(binary).parent())
        .filter(|dir| !tools::on_shell_path(home, dir))
        .map(|dir| dir.display().to_string());
    InstallOutcome::Installed {
        version: status.version,
        binary_path: status.binary_path,
        path_hint,
    }
}

/// 按输出判断失败原因（`npm`：是 npm 命令）。
fn failure(npm: bool, code: Option<i32>, lines: &[String], command: String) -> InstallOutcome {
    let text = lines.join("\n");
    if npm && (text.contains("EACCES") || text.contains("EPERM")) {
        return InstallOutcome::NpmPermission {
            command: if cfg!(windows) {
                command
            } else {
                format!("sudo {command}")
            },
        };
    }
    const NETWORK: [&str; 12] = [
        "ENOTFOUND",
        "ETIMEDOUT",
        "ECONNRESET",
        "ECONNREFUSED",
        "EAI_AGAIN",
        "Could not resolve host",
        "Failed to connect",
        "Connection timed out",
        "Operation timed out",
        "SSL_ERROR",
        "Unable to connect to the remote server",
        "The remote name could not be resolved",
    ];
    // curl 的网络错误码：6 解析不了域名，7 连不上，28 超时，35 TLS 握手失败，56 收数据失败。
    let curl_network = [6, 7, 28, 35, 56]
        .iter()
        .any(|code| text.contains(&format!("curl: ({code})")));
    if curl_network || NETWORK.iter().any(|pattern| text.contains(pattern)) {
        return InstallOutcome::Network { command };
    }
    InstallOutcome::Failed { code, command }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_lines_are_cleaned() {
        assert_eq!(
            clean_line("\u{1b}[32m✔ Installed\u{1b}[0m  \r"),
            Some("✔ Installed".into())
        );
        assert_eq!(clean_line("   \t "), None);
        assert_eq!(clean_line(&"x".repeat(1000)).unwrap().len(), MAX_LINE_CHARS);
    }

    #[test]
    fn node_versions_are_parsed() {
        assert_eq!(major_version("v22.11.0\n"), Some(22));
        assert_eq!(major_version("18.20.4"), Some(18));
        assert_eq!(major_version("not node"), None);
    }

    #[test]
    fn failures_are_explained_from_the_output() {
        let lines = |text: &str| vec![text.to_string()];
        assert!(matches!(
            failure(
                true,
                Some(243),
                &lines("npm error code EACCES"),
                "npm install -g @scope/tool".into()
            ),
            InstallOutcome::NpmPermission { .. }
        ));
        assert!(matches!(
            failure(
                true,
                Some(1),
                &lines("npm error code ETIMEDOUT"),
                String::new()
            ),
            InstallOutcome::Network { .. }
        ));
        assert!(matches!(
            failure(
                false,
                Some(28),
                &lines("curl: (28) Failed to connect to claude.ai"),
                String::new()
            ),
            InstallOutcome::Network { .. }
        ));
        assert_eq!(
            failure(false, Some(1), &lines("unsupported platform"), "cmd".into()),
            InstallOutcome::Failed {
                code: Some(1),
                command: "cmd".into()
            }
        );
    }

    #[test]
    fn npm_commands_can_use_the_mirror() {
        let npm = Method::Npm {
            package: "@scope/tool",
            node_major: 20,
            extra: &[],
        };
        assert_eq!(display_command(npm, false), "npm install -g @scope/tool");
        assert_eq!(
            display_command(npm, true),
            format!("npm install -g @scope/tool --registry={NPM_MIRROR}")
        );
    }

    #[test]
    fn user_proxy_settings_are_kept() {
        assert!(proxy_env(|key| key == "https_proxy").is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn streams_output_lines_and_reports_the_exit_code() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf 'downloading 10%%\\rdownloading 100%%\\n'; echo 'oops' 1>&2; printf '\\033[32mdone\\033[0m\\n'; exit 3",
        ]);
        let seen = Mutex::new(Vec::new());
        let result = run(command, |line| seen.lock().unwrap().push(line.to_string())).await;
        let Run::Done { code, lines } = result else {
            panic!("the command should finish");
        };
        assert_eq!(code, Some(3));
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        assert_eq!(
            seen,
            ["done", "downloading 10%", "downloading 100%", "oops"]
        );
        assert_eq!(lines.len(), 4);
    }
}
