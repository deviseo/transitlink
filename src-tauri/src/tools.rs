//! 本机编程工具的检测（第一期：Claude Code、Codex）。写入配置在 `configure` 模块。

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
}

impl ToolKind {
    pub const ALL: [ToolKind; 2] = [ToolKind::ClaudeCode, ToolKind::Codex];

    fn binary(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claude",
            ToolKind::Codex => "codex",
        }
    }

    /// 和前端一致的 ID，也用在 `~/.transitlink/tools.json` 里。
    pub fn id(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claudeCode",
            ToolKind::Codex => "codex",
        }
    }

    /// 备份目录名：`~/.transitlink/backups/<名字>`。
    pub fn backup_dir(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claude-code",
            ToolKind::Codex => "codex",
        }
    }

    /// 一键配置时给这个工具建的专用 Key 的名字。
    pub fn key_name(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "TransitLink Claude Code",
            ToolKind::Codex => "TransitLink Codex",
        }
    }

    /// 配置文件位置：Claude Code 支持用 `CLAUDE_CONFIG_DIR` 改目录，Codex 支持 `CODEX_HOME`。
    pub fn config_path(self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
        let (variable, default_dir, file) = match self {
            ToolKind::ClaudeCode => ("CLAUDE_CONFIG_DIR", ".claude", "settings.json"),
            ToolKind::Codex => ("CODEX_HOME", ".codex", "config.toml"),
        };
        env(variable)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(default_dir))
            .join(file)
    }
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

fn detect(
    kind: ToolKind,
    home: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    search: &OsString,
) -> ToolStatus {
    let binary = which::which_in(kind.binary(), Some(search), home).ok();
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
        // Codex 官方安装脚本在 Windows 上的安装位置。
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            extra.push(PathBuf::from(local).join("Programs\\OpenAI\\Codex\\bin"));
        }
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
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ','))
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
}
