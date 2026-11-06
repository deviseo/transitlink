//! 一键卸载工具：只卸载程序，配置文件和使用记录都留着，重新安装后照常可用。做法参考 cc-switch 的
//! tool_uninstall（MIT）：
//!
//! - 只用当初装它的包管理器卸载；认不出来就不动，请用户用原来的办法卸载，不会换一个包管理器去卸；
//! - Homebrew 只卸认得的包（例如 Codex 的 cask `codex`），并且关掉 Homebrew 卸载后自动删「不再需要的
//!   依赖」（否则可能连别的工具还在用的 node 一起删掉）；npm 要核对全局目录里确实装着这个包；
//! - 官方安装脚本装的，只删认得出的几个文件（命令的链接和它指向的程序），从不删目录：
//!   这些工具常把设置、会话和项目放在程序旁边；删之前先把这几个文件都挪开，有一个挪不动就全部放回；
//! - 先把要运行的命令或要删的文件给用户看，用户确认时重新检测一次，和看到的不一样就不动；
//!   卸载后确认这个命令真的没了。

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tools::ToolKind;
use crate::upgrade::{self, HOMEBREW_NO_AUTOREMOVE, Manager, path_text};

/// 卸载办法。给用户确认时原样显示，确认时和重新算出来的比对。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Plan {
    /// 运行包管理器的卸载命令（`npm`：是 npm 命令，失败时按 npm 的错误分析；`env`：额外的环境变量）。
    Command {
        program: String,
        args: Vec<String>,
        display: String,
        npm: bool,
        env: Vec<(String, String)>,
        /// 先运行的命令（程序和参数），成功了才运行上面的（OpenClaw 要先停掉它的后台服务）。
        #[serde(default)]
        before: Option<Vec<String>>,
    },
    /// 删除这些文件（官方安装脚本装的；先删命令，再删程序）。`program`：命令指向的程序，删之前核对。
    Files { program: String, files: Vec<String> },
    /// 这里没法卸载：`command` 是请用户在终端里运行的命令；为空时请用原来的办法卸载
    /// （`desktop`：随桌面版一起装的，在系统里卸载桌面版）。
    Manual {
        command: Option<String>,
        desktop: bool,
    },
}

/// 卸载预览：检测到的命令位置和卸载办法。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub binary: String,
    pub plan: Plan,
}

fn manual(command: Option<String>) -> Plan {
    Plan::Manual {
        command,
        desktop: false,
    }
}

/// 给用户复制的命令里的一个参数：有特殊字符时加单引号（终端和 PowerShell 里单引号里的内容都不展开）。
fn quoted(value: &str) -> String {
    let safe = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '_' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',' | '~' | '-'
            )
            || (cfg!(windows) && c == '\\')
    };
    if !value.is_empty() && value.chars().all(safe) {
        value.to_string()
    } else if cfg!(windows) {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// 路径转成文字。Windows 上去掉 `\\?\` 前缀（npm 和用户都看不懂）；不是合法文字的路径返回 None。
fn plain(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if cfg!(windows)
        && let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return Some(rest.to_string());
    }
    Some(text.to_string())
}

/// 选卸载办法。`binary`：检测到的命令位置；`local`：本机的版本（只用来给 pnpm 等的命令选包名）。
pub fn plan(
    kind: ToolKind,
    binary: &Path,
    local: Option<&str>,
    search: &OsString,
    home: &Path,
) -> Plan {
    let real = fs::canonicalize(binary).unwrap_or_else(|_| binary.to_path_buf());
    // 路径不是合法文字（个别 Linux 系统会有）：转换会出错，不动。
    if binary.to_str().is_none() || real.to_str().is_none() {
        return manual(None);
    }
    let lower = path_text(&real).to_ascii_lowercase();
    let package = upgrade::npm_package(kind, local);

    if kind == ToolKind::DeepseekHarness
        && (lower.contains(".app/") || lower.contains("/resources/"))
    {
        return Plan::Manual {
            command: None,
            desktop: true,
        };
    }
    // Homebrew 的包里也可能有 node_modules：先看是不是 Homebrew 装的。
    if let Some(plan) = homebrew_plan(kind, &real) {
        return plan;
    }
    // OpenClaw 先停掉并删掉它的后台服务（npm 卸载不会管，留下的服务会一直失败）。
    if kind == ToolKind::OpenClaw {
        const STOP: [&str; 4] = ["uninstall", "--service", "--yes", "--non-interactive"];
        let stop = format!("openclaw {} && ", STOP.join(" "));
        return match upgrade::manager(binary, &real, upgrade::npm_packages(kind)) {
            Some(Manager::Npm) => match npm_plan(kind, binary, &real, search, home) {
                Ok(Plan::Command {
                    program,
                    args,
                    display,
                    npm,
                    env,
                    ..
                }) => Plan::Command {
                    program,
                    args,
                    display: format!("{stop}{display}"),
                    npm,
                    env,
                    before: Some(
                        std::iter::once(binary.display().to_string())
                            .chain(STOP.iter().map(|arg| arg.to_string()))
                            .collect(),
                    ),
                },
                Ok(other) => other,
                Err(NpmIssue::NoNpm(package)) => {
                    manual(Some(format!("{stop}npm uninstall -g {package}")))
                }
                Err(NpmIssue::Unverified) => manual(None),
            },
            Some(Manager::Pnpm) => manual(Some(format!("{stop}pnpm remove -g {package}"))),
            Some(Manager::Bun) => manual(Some(format!("{stop}bun remove -g {package}"))),
            _ => manual(None),
        };
    }
    match upgrade::manager(binary, &real, upgrade::npm_packages(kind)) {
        Some(Manager::Npm) => {
            return match npm_plan(kind, binary, &real, search, home) {
                Ok(plan) => plan,
                // 确实是 npm 装的，只是找不到 npm：告诉用户命令。
                Err(NpmIssue::NoNpm(package)) => {
                    manual(Some(format!("npm uninstall -g {package}")))
                }
                // 核对不上（例如项目里的 node_modules、链接到别处的目录）：不猜。
                Err(NpmIssue::Unverified) => manual(None),
            };
        }
        Some(Manager::Pnpm) => return manual(Some(format!("pnpm remove -g {package}"))),
        Some(Manager::Bun) => return manual(Some(format!("bun remove -g {package}"))),
        Some(Manager::Yarn) => return manual(Some(format!("yarn global remove {package}"))),
        Some(Manager::Volta) => return manual(Some(format!("volta uninstall {package}"))),
        None => {}
    }
    match native_files(kind, binary, &real) {
        Some(files) => match (
            plain(&real),
            files
                .iter()
                .map(|path| plain(path))
                .collect::<Option<Vec<_>>>(),
        ) {
            (Some(program), Some(files)) => Plan::Files { program, files },
            _ => manual(None),
        },
        None => manual(None),
    }
}

/// Homebrew 装的：只卸认得的包；别的包（例如提供 node 的 formula）不猜，请用户自己卸。
fn homebrew_plan(kind: ToolKind, real: &Path) -> Option<Plan> {
    let text = path_text(real);
    for (marker, flag) in [("/Caskroom/", "--cask"), ("/Cellar/", "--formula")] {
        let Some((prefix, rest)) = text.split_once(marker) else {
            continue;
        };
        let token = rest.split('/').next().unwrap_or_default();
        let owners: &[(&str, &str)] = match kind {
            ToolKind::ClaudeCode => &[
                ("/Caskroom/", "claude-code"),
                ("/Caskroom/", "claude-code@latest"),
            ],
            ToolKind::Codex => &[("/Caskroom/", "codex")],
            ToolKind::GeminiCli => &[("/Cellar/", "gemini-cli")],
            ToolKind::OpenCode => &[("/Cellar/", "opencode")],
            ToolKind::Pi => &[("/Cellar/", "pi-coding-agent")],
            ToolKind::OhMyPi => &[("/Cellar/", "omp")],
            ToolKind::KimiCode => &[("/Cellar/", "kimi-code")],
            // `--zap` 会删掉用户数据，我们只用普通的卸载。
            ToolKind::Antigravity => &[("/Caskroom/", "antigravity-cli")],
            ToolKind::GrokBuild
            | ToolKind::DeepseekHarness
            | ToolKind::MinimaxCode
            | ToolKind::OpenClaw => &[],
        };
        let brew = PathBuf::from(prefix).join("bin").join("brew");
        if !owners.contains(&(marker, token)) || !brew.is_file() {
            return Some(manual(None));
        }
        let (key, value) = HOMEBREW_NO_AUTOREMOVE;
        return Some(Plan::Command {
            program: plain(&brew)?,
            args: vec!["uninstall".into(), flag.into(), token.to_string()],
            display: format!("{key}={value} brew uninstall {flag} {token}"),
            npm: false,
            env: vec![(key.to_string(), value.to_string())],
            before: None,
        });
    }
    None
}

enum NpmIssue {
    /// 全局目录核对不上。
    Unverified,
    /// 核对上了，但找不到 npm：附上包名。
    NoNpm(&'static str),
}

/// npm 全局装的：命令在 `<前缀>/bin`（Windows 在 `<前缀>` 本身），包正好在
/// `<前缀>/lib/node_modules/<包>`（Windows 是 `<前缀>/node_modules/<包>`，都不能是链接到别处的），
/// 包的名字对得上、命令指向这个包，前缀本身也不是一个项目（没有 package.json），才用这一处的 npm 卸载。
fn npm_plan(
    kind: ToolKind,
    binary: &Path,
    real: &Path,
    search: &OsString,
    home: &Path,
) -> Result<Plan, NpmIssue> {
    let windows = cfg!(windows);
    let launcher_dir = binary.parent().ok_or(NpmIssue::Unverified)?;
    let prefix = if windows {
        if launcher_dir.file_name().is_some_and(|name| name == ".bin") {
            return Err(NpmIssue::Unverified);
        }
        launcher_dir
    } else {
        if launcher_dir.file_name().is_none_or(|name| name != "bin") {
            return Err(NpmIssue::Unverified);
        }
        launcher_dir.parent().ok_or(NpmIssue::Unverified)?
    };
    let prefix = fs::canonicalize(prefix).map_err(|_| NpmIssue::Unverified)?;
    if prefix.join("package.json").exists() {
        return Err(NpmIssue::Unverified);
    }
    let modules = if windows {
        prefix.join("node_modules")
    } else {
        prefix.join("lib").join("node_modules")
    };
    // OpenCode 1、2 是两个包：看装的是哪个（不靠 `--version`，它可能超时）。
    let package = upgrade::npm_packages(kind)
        .iter()
        .copied()
        .find(|package| owns(binary, real, &modules, package, windows))
        .ok_or(NpmIssue::Unverified)?;
    let sibling = launcher_dir.join(if windows { "npm.cmd" } else { "npm" });
    let npm = Some(sibling)
        .filter(|path| path.is_file())
        .or_else(|| which::which_in("npm", Some(search), home).ok())
        .ok_or(NpmIssue::NoNpm(package))?;
    let prefix_text = plain(&prefix).ok_or(NpmIssue::Unverified)?;
    Ok(Plan::Command {
        program: plain(&npm).ok_or(NpmIssue::Unverified)?,
        args: vec![
            "uninstall".into(),
            "-g".into(),
            "--prefix".into(),
            prefix_text.clone(),
            package.to_string(),
        ],
        display: format!(
            "npm uninstall -g --prefix {} {package}",
            quoted(&prefix_text)
        ),
        npm: true,
        env: Vec::new(),
        before: None,
    })
}

/// `modules` 下的 `package` 是不是这个命令的包：包目录正好在那里（不是链接到别处）、名字对得上，
/// 命令指向包里（Windows 上命令是启动脚本，看脚本里写的是不是这个包）。
fn owns(binary: &Path, real: &Path, modules: &Path, package: &str, windows: bool) -> bool {
    let expected = package
        .split('/')
        .fold(modules.to_path_buf(), |path, part| path.join(part));
    let Ok(package_dir) = fs::canonicalize(&expected) else {
        return false;
    };
    if package_dir != expected {
        return false;
    }
    let named = fs::read(package_dir.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|manifest| {
            manifest.get("name").and_then(serde_json::Value::as_str) == Some(package)
        });
    named
        && (real.starts_with(&package_dir)
            || (windows
                && upgrade::small_text(binary).is_some_and(|script| {
                    script
                        .replace('\\', "/")
                        .contains(&format!("node_modules/{package}/"))
                })))
}

/// 是不是编译好的程序（ELF、Mach-O、Windows 的 PE）：只删真正的程序，不删别的脚本。
fn is_native_executable(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    let read = fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic));
    read.is_ok()
        && (matches!(
            magic,
            [0x7f, b'E', b'L', b'F']
                | [0xcf, 0xfa, 0xed, 0xfe]
                | [0xce, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xfe, 0xed, 0xfa, 0xce]
                | [0xca, 0xfe, 0xba, 0xbe]
        ) || magic.starts_with(b"MZ"))
}

/// 官方安装脚本装的：要删的文件（先删命令，再删程序）。认不出来时返回 None。
fn native_files(kind: ToolKind, binary: &Path, real: &Path) -> Option<Vec<PathBuf>> {
    if !is_native_executable(real) {
        return None;
    }
    let lower = path_text(real).to_ascii_lowercase();
    let known = match kind {
        // `~/.local/bin/claude` 链接到 `~/.local/share/claude/versions/<版本>`；Windows 上
        // `~/.local/bin/claude.exe` 就是程序本身。`~/.local/bin` 是大家共用的目录，macOS、Linux 上
        // 只认指向 versions 的。
        ToolKind::ClaudeCode => {
            lower
                .split_once("/.local/share/claude/versions/")
                .is_some_and(|(_, version)| !version.is_empty() && !version.contains('/'))
                || (cfg!(windows) && lower.ends_with("/.local/bin/claude.exe"))
        }
        ToolKind::Codex => lower
            .split_once("/packages/standalone/releases/")
            .and_then(|(_, release)| release.split_once('/'))
            .is_some_and(|(version, program)| {
                !version.is_empty()
                    && matches!(
                        program,
                        "bin/codex" | "bin/codex.exe" | "codex" | "codex.exe"
                    )
            }),
        ToolKind::GrokBuild => grok_root(real).is_some(),
        ToolKind::OpenCode => {
            lower.ends_with("/.opencode/bin/opencode")
                || lower.ends_with("/.opencode/bin/opencode.exe")
        }
        // 官方安装脚本只放这一个程序（Oh My Pi 指定装编好的程序时）。
        ToolKind::OhMyPi => lower.ends_with("/.local/bin/omp") || lower.ends_with("/omp/omp.exe"),
        ToolKind::KimiCode => {
            lower.ends_with("/.kimi-code/bin/kimi") || lower.ends_with("/.kimi-code/bin/kimi.exe")
        }
        ToolKind::Antigravity => {
            lower.ends_with("/.local/bin/agy") || lower.ends_with("/agy/bin/agy.exe")
        }
        // 官方安装脚本装的是一整个目录（几个版本的 npm 依赖），只能由用户自己删。
        ToolKind::GeminiCli
        | ToolKind::DeepseekHarness
        | ToolKind::Pi
        | ToolKind::MinimaxCode
        | ToolKind::OpenClaw => false,
    };
    if !known {
        return None;
    }
    let mut files = vec![located(binary), real.to_path_buf()];
    if kind == ToolKind::GrokBuild
        && let Some(root) = grok_root(real)
    {
        files.extend(grok_files(&root));
    }
    // 同一个文件可能经由不同的路径出现（例如 macOS 的 /var 是 /private/var 的链接）：按真实目录去重。
    let mut seen = std::collections::HashSet::new();
    files.retain(|path| seen.insert(path.clone()));
    Some(files)
}

/// 文件所在目录换成真实目录（文件本身是链接时不跟着走，删的还是这个链接）。
fn located(path: &Path) -> PathBuf {
    match (
        path.parent().and_then(|dir| fs::canonicalize(dir).ok()),
        path.file_name(),
    ) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

const GROK_PLATFORMS: [&str; 6] = [
    "macos-aarch64",
    "macos-x86_64",
    "linux-aarch64",
    "linux-x86_64",
    "windows-aarch64.exe",
    "windows-x86_64.exe",
];

fn is_grok_version(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// Grok 官方安装脚本写的程序名：`bin/grok`、`bin/grok-<版本>`、`downloads/grok-<平台>`、
/// `downloads/grok-<版本>-<平台>`。
fn is_grok_binary(dir: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let Some(rest) = name.strip_prefix("grok-") else {
        return dir == "bin" && matches!(name.as_str(), "grok" | "grok.exe");
    };
    match dir {
        "bin" => is_grok_version(rest.strip_suffix(".exe").unwrap_or(rest)),
        "downloads" => GROK_PLATFORMS.iter().any(|platform| {
            rest == *platform
                || rest
                    .strip_suffix(platform)
                    .and_then(|version| version.strip_suffix('-'))
                    .is_some_and(is_grok_version)
        }),
        _ => false,
    }
}

/// 程序在 `~/.grok/bin` 或 `~/.grok/downloads` 里时，返回 `~/.grok`。
fn grok_root(real: &Path) -> Option<PathBuf> {
    let name = real.file_name()?.to_str()?;
    let dir = real.parent()?;
    let kind = dir.file_name()?.to_str()?.to_ascii_lowercase();
    let root = dir.parent()?;
    (root.file_name()?.eq_ignore_ascii_case(".grok") && is_grok_binary(&kind, name))
        .then(|| root.to_path_buf())
}

/// Grok 的安装脚本会留下下载过的每个版本，再用 `grok`、`agent` 链接到其中一个：只删当前那个的话，
/// 其他版本和还能用的 `agent` 都会留下。
fn grok_files(root: &Path) -> Vec<PathBuf> {
    let mut launchers = Vec::new();
    let mut binaries = Vec::new();
    for kind in ["bin", "downloads"] {
        let Ok(entries) = fs::read_dir(root.join(kind)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if metadata.file_type().is_symlink() {
                let launcher = kind == "bin"
                    && matches!(name.as_str(), "grok" | "grok.exe" | "agent" | "agent.exe");
                if launcher
                    && fs::canonicalize(&path)
                        .is_ok_and(|target| grok_root(&target).as_deref() == Some(root))
                {
                    launchers.push(path);
                }
            } else if metadata.is_file()
                && is_grok_binary(kind, &name)
                && is_native_executable(&path)
            {
                binaries.push(path);
            }
        }
    }
    launchers.sort();
    binaries.sort();
    launchers.extend(binaries);
    launchers
}

/// 挪开的文件在同目录里的临时名字。
fn aside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(
        ".{name}.transitlink-removed-{}",
        std::process::id()
    ))
}

/// 删掉官方安装脚本装的文件。先核对命令还指向原来的程序（链接被换过就不删）；再把每个文件挪到同目录
/// 的临时名字（链接本身，不跟着走；碰到目录就停），有一个挪不动就全部挪回去、什么都不删，免得删到
/// 一半命令没了程序还在；都挪开以后再删。返回删不掉的（例如 Windows 上正在运行的）临时文件。
pub fn remove_files(binary: &Path, program: &str, files: &[String]) -> Result<Vec<String>, String> {
    if fs::canonicalize(binary)
        .ok()
        .as_deref()
        .and_then(plain)
        .as_deref()
        != Some(program)
    {
        return Err("changed".into());
    }
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let put_back = |moved: &[(PathBuf, PathBuf)]| {
        for (original, temporary) in moved.iter().rev() {
            let _ = fs::rename(temporary, original);
        }
    };
    for file in files {
        let path = Path::new(file);
        let problem = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                let temporary = aside(path);
                match fs::rename(path, &temporary) {
                    Ok(()) => {
                        moved.push((path.to_path_buf(), temporary));
                        None
                    }
                    Err(error) => Some(format!("{}: {error}", path.display())),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Ok(_) => Some(format!("{} is a directory", path.display())),
            Err(error) => Some(format!("{}: {error}", path.display())),
        };
        if let Some(problem) = problem {
            put_back(&moved);
            return Err(problem);
        }
    }
    Ok(moved
        .iter()
        .filter(|(_, temporary)| fs::remove_file(temporary).is_err())
        .map(|(_, temporary)| temporary.display().to_string())
        .collect())
}

/// 手动删这些文件的命令（删不掉时给用户）。
pub fn remove_command(files: &[String]) -> String {
    let list: Vec<String> = files.iter().map(|file| quoted(file)).collect();
    if cfg!(windows) {
        format!("Remove-Item -LiteralPath {}", list.join(","))
    } else {
        format!("rm -f {}", list.join(" "))
    }
}

/// 命令已经不在了（`exists` 会跟着链接走，断掉的链接也算还在）。
pub fn removed(binary: &Path) -> bool {
    matches!(fs::symlink_metadata(binary), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假装是 Mach-O 程序的文件。
    fn program(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, [0xcf, 0xfa, 0xed, 0xfe, 0, 0]).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "transitlink-uninstall-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir).unwrap()
    }

    #[cfg(unix)]
    fn npm_prefix(dir: &Path, package: &str, name: &str) -> PathBuf {
        let prefix = dir.join("node");
        let package_dir = prefix.join("lib/node_modules").join(package);
        fs::create_dir_all(package_dir.join("dist")).unwrap();
        fs::write(package_dir.join("dist/index.js"), "").unwrap();
        fs::write(
            package_dir.join("package.json"),
            format!(r#"{{"name":"{package}"}}"#),
        )
        .unwrap();
        fs::create_dir_all(prefix.join("bin")).unwrap();
        fs::write(prefix.join("bin/npm"), "").unwrap();
        std::os::unix::fs::symlink(
            package_dir.join("dist/index.js"),
            prefix.join("bin").join(name),
        )
        .unwrap();
        prefix
    }

    #[cfg(unix)]
    #[test]
    fn npm_installs_are_verified_before_uninstalling() {
        let dir = temp_dir("npm");
        let home = dir.join("home");
        let search = OsString::new();
        let prefix = npm_prefix(&dir, "@google/gemini-cli", "gemini");
        let binary = prefix.join("bin/gemini");
        let Plan::Command {
            program, args, npm, ..
        } = plan(ToolKind::GeminiCli, &binary, None, &search, &home)
        else {
            panic!("expected an npm command");
        };
        assert_eq!(PathBuf::from(program), prefix.join("bin/npm"));
        assert_eq!(
            args,
            [
                "uninstall",
                "-g",
                "--prefix",
                &prefix.display().to_string(),
                "@google/gemini-cli"
            ]
        );
        assert!(npm);

        // package.json 的名字对不上、前缀本身是个项目：不动，也不给可能卸错的命令。
        let manifest = prefix.join("lib/node_modules/@google/gemini-cli/package.json");
        fs::write(&manifest, r#"{"name":"something-else"}"#).unwrap();
        assert_eq!(
            plan(ToolKind::GeminiCli, &binary, None, &search, &home),
            manual(None)
        );
        fs::write(&manifest, r#"{"name":"@google/gemini-cli"}"#).unwrap();
        fs::write(prefix.join("package.json"), "{}").unwrap();
        assert_eq!(
            plan(ToolKind::GeminiCli, &binary, None, &search, &home),
            manual(None)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn npm_modules_linked_elsewhere_are_not_trusted() {
        let dir = temp_dir("npm-link");
        let elsewhere = npm_prefix(&dir.join("store"), "@openai/codex", "codex");
        let prefix = dir.join("profile");
        fs::create_dir_all(prefix.join("lib")).unwrap();
        fs::create_dir_all(prefix.join("bin")).unwrap();
        std::os::unix::fs::symlink(
            elsewhere.join("lib/node_modules"),
            prefix.join("lib/node_modules"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            elsewhere.join("lib/node_modules/@openai/codex/dist/index.js"),
            prefix.join("bin/codex"),
        )
        .unwrap();
        assert_eq!(
            plan(
                ToolKind::Codex,
                &prefix.join("bin/codex"),
                None,
                &OsString::new(),
                &dir
            ),
            manual(None)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn openclaw_stops_its_background_service_first() {
        let dir = temp_dir("openclaw");
        let prefix = npm_prefix(&dir, "openclaw", "openclaw");
        let binary = prefix.join("bin/openclaw");
        let Plan::Command {
            before,
            display,
            args,
            ..
        } = plan(ToolKind::OpenClaw, &binary, None, &OsString::new(), &dir)
        else {
            panic!("expected a command");
        };
        assert_eq!(
            before,
            Some(vec![
                binary.display().to_string(),
                "uninstall".into(),
                "--service".into(),
                "--yes".into(),
                "--non-interactive".into()
            ])
        );
        assert!(display.starts_with(
            "openclaw uninstall --service --yes --non-interactive && npm uninstall -g"
        ));
        assert_eq!(args.last().map(String::as_str), Some("openclaw"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn opencode_two_is_found_on_disk_without_the_version() {
        let dir = temp_dir("opencode2");
        let prefix = npm_prefix(&dir, "@opencode/cli", "opencode");
        let Plan::Command { args, .. } = plan(
            ToolKind::OpenCode,
            &prefix.join("bin/opencode"),
            None,
            &OsString::new(),
            &dir,
        ) else {
            panic!("expected an npm command");
        };
        assert_eq!(args.last().map(String::as_str), Some("@opencode/cli"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn native_installs_remove_only_their_own_files() {
        let dir = temp_dir("native");
        let home = dir.join("home");
        let search = OsString::new();

        // Claude Code 官方安装：链接加它指向的那个版本。
        let version = home.join(".local/share/claude/versions/2.1.318");
        program(&version);
        let other = home.join(".local/share/claude/versions/2.1.300");
        program(&other);
        let launcher = home.join(".local/bin/claude");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&version, &launcher).unwrap();
        let claude = plan(ToolKind::ClaudeCode, &launcher, None, &search, &home);
        let real = version.display().to_string();
        assert_eq!(
            claude,
            Plan::Files {
                program: real.clone(),
                files: vec![launcher.display().to_string(), real.clone()]
            }
        );
        let Plan::Files {
            program: target,
            files,
        } = claude
        else {
            unreachable!()
        };
        assert_eq!(remove_files(&launcher, &target, &files), Ok(Vec::new()));
        assert!(removed(&launcher));
        assert!(!version.exists());
        assert!(other.exists(), "other versions and the data folder stay");

        // ~/.local/bin 里别的程序碰巧叫 claude：不是官方安装，不删。
        program(&launcher);
        assert_eq!(
            plan(ToolKind::ClaudeCode, &launcher, None, &search, &home),
            manual(None)
        );

        // 不是编译好的程序（例如自己写的脚本）：不删。
        let script = home.join(".opencode/bin/opencode");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "#!/bin/sh\n").unwrap();
        assert_eq!(
            plan(ToolKind::OpenCode, &script, None, &search, &home),
            manual(None)
        );
        program(&script);
        let real = script.display().to_string();
        assert_eq!(
            plan(ToolKind::OpenCode, &script, None, &search, &home),
            Plan::Files {
                program: real.clone(),
                files: vec![real]
            },
            "the program itself is listed once"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_moved_puts_everything_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("partial");
        let home = dir.join("home");
        let version = home.join(".local/share/claude/versions/2.1.318");
        program(&version);
        let launcher = home.join(".local/bin/claude");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&version, &launcher).unwrap();
        let Plan::Files {
            program: target,
            files,
        } = plan(
            ToolKind::ClaudeCode,
            &launcher,
            None,
            &OsString::new(),
            &home,
        )
        else {
            panic!("expected files");
        };
        // 程序所在的目录只读：链接已经挪开了也要放回去。
        let versions = version.parent().unwrap();
        fs::set_permissions(versions, fs::Permissions::from_mode(0o555)).unwrap();
        let result = remove_files(&launcher, &target, &files);
        fs::set_permissions(versions, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert!(!removed(&launcher), "the command is put back");
        assert!(version.exists());
        assert_eq!(fs::read_dir(launcher.parent().unwrap()).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn grok_installs_remove_every_installed_version() {
        let dir = temp_dir("grok");
        let home = dir.join("home");
        let root = home.join(".grok");
        program(&root.join("bin/grok-1.0.70"));
        program(&root.join("bin/grok-1.0.69"));
        program(&root.join("downloads/grok-1.0.70-macos-aarch64"));
        fs::write(root.join("bin/notes.txt"), "keep").unwrap();
        std::os::unix::fs::symlink(root.join("bin/grok-1.0.70"), root.join("bin/grok")).unwrap();
        std::os::unix::fs::symlink(root.join("bin/grok-1.0.70"), root.join("bin/agent")).unwrap();
        let Plan::Files {
            program: target,
            files,
        } = plan(
            ToolKind::GrokBuild,
            &root.join("bin/grok"),
            None,
            &OsString::new(),
            &home,
        )
        else {
            panic!("expected files");
        };
        assert_eq!(files.len(), 5, "{files:?}");
        assert_eq!(
            remove_files(&root.join("bin/grok"), &target, &files),
            Ok(Vec::new())
        );
        assert!(root.join("bin/notes.txt").exists());
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), 1);
        assert!(
            fs::read_dir(root.join("downloads"))
                .unwrap()
                .next()
                .is_none()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn other_installs_are_left_to_the_user() {
        let dir = temp_dir("other");
        let home = dir.join("home");
        let search = OsString::new();
        // Homebrew：认得的包才卸，并且不让它顺手删别的依赖；别的包不猜。
        let cellar = dir.join("homebrew/Cellar/gemini-cli/0.67.0/bin/gemini");
        program(&cellar);
        let brew = dir.join("homebrew/bin/brew");
        program(&brew);
        assert_eq!(
            plan(ToolKind::GeminiCli, &cellar, None, &search, &home),
            Plan::Command {
                program: brew.display().to_string(),
                args: vec!["uninstall".into(), "--formula".into(), "gemini-cli".into()],
                display: "HOMEBREW_NO_AUTOREMOVE=1 brew uninstall --formula gemini-cli".into(),
                npm: false,
                env: vec![("HOMEBREW_NO_AUTOREMOVE".into(), "1".into())],
                before: None,
            }
        );
        let node = dir.join("homebrew/Cellar/node/24.0.0/lib/node_modules/@openai/codex/bin/codex");
        program(&node);
        assert_eq!(
            plan(ToolKind::Codex, &node, None, &search, &home),
            manual(None)
        );

        // pnpm 装的：给出 pnpm 的命令。
        let pnpm = dir.join("Library/pnpm/gemini");
        fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
        fs::write(&pnpm, "exec node \"$basedir/global/5/node_modules/@google/gemini-cli/dist/index.js\" # /Library/pnpm/global/5\n").unwrap();
        assert_eq!(
            plan(ToolKind::GeminiCli, &pnpm, None, &search, &home),
            manual(Some("pnpm remove -g @google/gemini-cli".into()))
        );

        // DeepSeek Harness 桌面版带的：在系统里卸载桌面版。
        let app = dir.join("DeepSeek Harness.app/Contents/Resources/bin/dsh");
        program(&app);
        assert_eq!(
            plan(ToolKind::DeepseekHarness, &app, None, &search, &home),
            Plan::Manual {
                command: None,
                desktop: true
            }
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_replaced_link_is_not_followed() {
        let dir = temp_dir("replaced");
        let first = dir.join("a/program");
        let second = dir.join("b/program");
        program(&first);
        program(&second);
        let link = dir.join("bin/tool");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&second, &link).unwrap();
        let target = first.display().to_string();
        let files = vec![link.display().to_string(), target.clone()];
        assert_eq!(remove_files(&link, &target, &files), Err("changed".into()));
        assert!(first.exists() && second.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn copied_commands_are_quoted_for_the_shell() {
        assert_eq!(
            quoted("/Users/a/.local/bin/claude"),
            "/Users/a/.local/bin/claude"
        );
        assert_eq!(quoted("/tmp/u$USER/x"), "'/tmp/u$USER/x'");
        assert_eq!(quoted("/a b/it's"), "'/a b/it'\\''s'");
        assert_eq!(
            remove_command(&["/a b".into(), "/c;d".into()]),
            "rm -f '/a b' '/c;d'"
        );
    }
}
