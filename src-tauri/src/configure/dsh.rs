//! DeepSeek Harness（`dsh`）：在 `$DSH_HOME`（默认 `~/.dsh`）的配置档里加一个 `transitlink` 模型服务商，
//! 再把默认模型指向它；Key 存在 `.credentials.yaml` 的 `refs` 里，服务商用 `apiKeyEnv` 引用它的名字。
//!
//! - 写 `web` 和 `desktop` 两个配置档（第一次运行前也可以先写好），已经有 `acp` 的也写。`headless`
//!   不读 `.credentials.yaml`，不写。
//! - 配置档 `profiles/<名字>/cordis.patch.yml` 是一个列表，每项按 `id` 区分：我们只改 `llm-pi-ai`
//!   这一项里 `config.providers.transitlink`，和 `agent-default-model` 这一项。这一项整个替换
//!   （DSH 的补丁不合并），其他项一个字节都不动（包括注释和 `!!js`）；要改的项本身用了标签、
//!   锚点时不改（重新生成会丢掉它们）。
//! - 几个文件当作一次修改：先拿好写锁（和 DSH 用同一套锁，见 [`FileLock`]），再算好所有新内容（有一个
//!   不合法就都不写），写的时候出错把已经写的改回去。
//! - 两项之间的空行和顶格注释（多半是写给下一项的）原样保留；`.credentials.yaml` 只改我们那一行。
//! - 总配置 `$DSH_HOME/cordis.patch.yml` 里有 `llm-pi-ai` 或 `agent-default-model` 的配置时，它会盖过
//!   各配置档，不写，提示用户。
//!
//! 写法参考了 cc-switch（MIT）对 DeepSeek Harness 的支持。2026-11-05 对照 DSH 0.2.1-rc.5、0.2.2-rc.1 核实。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value as Yaml};
use sha2::{Digest, Sha256};

use super::{ConfigError, files};

pub const PROVIDER_ID: &str = "transitlink";
/// `.credentials.yaml` 里存 Key 的引用名（不用 `OPENAI_API_KEY` 这类名字：存的值会盖过用户自己的环境变量）。
pub const KEY_REF: &str = "TRANSITLINK_API_KEY";
const PLUGIN: &str = "llm-pi-ai";
const DEFAULT_MODEL: &str = "agent-default-model";
/// 一定要写的配置档；`acp` 有才写。
const PROFILES: [&str; 2] = ["web", "desktop"];
const OPTIONAL_PROFILES: [&str; 1] = ["acp"];
/// 写锁被别人拿着时最多等这么久（和 DSH 默认的一样），之后报「正被别的程序修改」。
#[cfg(not(test))]
const LOCK_WAIT: Duration = Duration::from_secs(2);
#[cfg(test)]
const LOCK_WAIT: Duration = Duration::from_millis(200);

/// DSH 的数据目录：`DSH_HOME`，没设（或为空）时是 `~/.dsh`。
pub fn home(user_home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    env("DSH_HOME")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| user_home.join(".dsh"))
}

pub fn profile_path(home: &Path, name: &str) -> PathBuf {
    home.join("profiles").join(name).join("cordis.patch.yml")
}

fn credentials_path(home: &Path) -> PathBuf {
    home.join(".credentials.yaml")
}

/// 要写的配置档。
fn profiles(home: &Path) -> Vec<&'static str> {
    PROFILES
        .iter()
        .chain(
            OPTIONAL_PROFILES
                .iter()
                .filter(|name| profile_path(home, name).is_file()),
        )
        .copied()
        .collect()
}

/// 一键配置会改的文件和备份用的名字（几个配置档都叫 `cordis.patch.yml`，备份名前面加上配置档名）。
pub fn files(home: &Path) -> Vec<(PathBuf, String)> {
    let mut paths = vec![(credentials_path(home), "credentials.yaml".to_string())];
    paths.extend(
        profiles(home)
            .iter()
            .map(|name| (profile_path(home, name), format!("{name}-cordis.patch.yml"))),
    );
    paths
}

// ---------------------------------------------------------------- YAML 文本

/// 没法安全修改的写法。`row`：哪一项（不知道时为空），[`in_file`] 再补上文件。
fn complex(row: &str) -> ConfigError {
    ConfigError::Unsupported(row.to_string())
}

/// 错误里写上是哪个文件（几个配置档同名，只写文件名分不清）。
fn in_file(error: ConfigError, path: &Path) -> ConfigError {
    match error {
        ConfigError::Invalid => ConfigError::InvalidFile(path.display().to_string()),
        ConfigError::Unsupported(row) if row.is_empty() => {
            ConfigError::Unsupported(path.display().to_string())
        }
        ConfigError::Unsupported(row) => {
            ConfigError::Unsupported(format!("{} · {row}", path.display()))
        }
        other => other,
    }
}

/// 配置档里的列表（空文件当作空列表）。
fn entries(text: &str) -> Result<Vec<Yaml>, ConfigError> {
    match serde_yaml::from_str::<Yaml>(text).map_err(|_| ConfigError::Invalid)? {
        Yaml::Null => Ok(Vec::new()),
        Yaml::Sequence(items) => Ok(items),
        _ => Err(ConfigError::Invalid),
    }
}

/// 顶层每一项（`- ` 开头的行）在文本里的起点。
fn item_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("- ") || line.trim_end() == "-" {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts
}

/// 一项到它最后一行内容为止：后面的空行和顶格注释（多半是写给下一项的，或者注释掉的项）不算这一项，
/// 换掉、删掉这一项时留着。
fn item_end(text: &str, start: usize, next: usize) -> usize {
    let mut end = start;
    let mut offset = start;
    for line in text[start..next].split_inclusive('\n') {
        let first = offset == start;
        offset += line.len();
        if first || !(line.trim().is_empty() || line.starts_with('#')) {
            end = offset;
        }
    }
    end
}

/// 这一行有没有用 YAML 标签（`!!js`）、锚点（`&`）、别名（`*`）或合并键（`<<:`）。只看节点开头的位置，
/// 引号里的内容和注释不算（参考 cc-switch 的同名检查）。
fn uses_references(line: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut node_start = true;
    let mut chars = line.chars().peekable();
    let mut previous = ' ';
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if open == '"' && c == '\\' {
                chars.next();
            } else if c == open {
                if open == '\'' && chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    quote = None;
                    node_start = false;
                }
            }
            previous = c;
            continue;
        }
        if c == '<' && chars.peek() == Some(&'<') {
            let rest: String = chars.clone().take(2).collect();
            if rest == "<:" {
                return true;
            }
        }
        match c {
            // `#` 在行首或空白后面才是注释。
            '#' if previous.is_whitespace() => break,
            '"' | '\'' if node_start => quote = Some(c),
            '&' | '*' | '!' if node_start => return true,
            ' ' | '\t' => {}
            '-' | '?' if node_start => {
                // `- `、`? `：后面还是节点的开头。
                node_start = chars.peek().is_some_and(|next| next.is_whitespace());
            }
            ':' | '[' | '{' | ',' => node_start = true,
            _ => node_start = false,
        }
        previous = c;
    }
    false
}

/// 把第 `index` 项换成 `entry`（`entry` 为 None 时删掉这一项；`index` 为 None 时加在最后），
/// 其他内容一个字节都不动。
fn splice(text: &str, index: Option<usize>, entry: Option<&Yaml>) -> Result<String, ConfigError> {
    let parsed = entries(text)?;
    let replacement = match entry {
        Some(entry) => serde_yaml::to_string(&vec![entry.clone()])
            .map_err(|error| ConfigError::Io(error.to_string()))?,
        // 删掉最后一项时要留下 `[]`：DSH 要求配置档是列表，空文件会启动失败。
        None if parsed.len() <= 1 && index.is_some() => "[]\n".to_string(),
        None => String::new(),
    };
    if parsed.is_empty() {
        // 新配置档是几行注释加 `[]`：保留注释，把 `[]` 换成我们的项。
        let mut next: String = text
            .split_inclusive('\n')
            .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
            .collect();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&replacement);
        return Ok(next);
    }
    let starts = item_starts(text);
    if starts.len() != parsed.len() {
        // 写成了 `[{...}]` 这种行内列表，或者缩进不在第一列：没法只换一项。
        return Err(complex(""));
    }
    let next = match index {
        Some(index) => {
            let next_start = starts.get(index + 1).copied().unwrap_or(text.len());
            let end = item_end(text, starts[index], next_start);
            let original = &text[starts[index]..end];
            if entry.is_some() && original.lines().any(uses_references) {
                return Err(complex(&entry_id(&parsed[index]).unwrap_or_default()));
            }
            format!("{}{}{}", &text[..starts[index]], replacement, &text[end..])
        }
        None if replacement.is_empty() => text.to_string(),
        None => {
            let separator = if text.ends_with('\n') { "" } else { "\n" };
            format!("{text}{separator}{replacement}")
        }
    };
    // 删掉的项可能定义了别处引用的锚点：改完必须还能解析。
    entries(&next).map_err(|_| complex(""))?;
    Ok(next)
}

fn entry_id(entry: &Yaml) -> Option<String> {
    entry.get("id").and_then(Yaml::as_str).map(str::to_string)
}

/// 列表里 `id` 等于 `id` 的那一项的位置（有重复时算格式有误）。
fn find(parsed: &[Yaml], id: &str) -> Result<Option<usize>, ConfigError> {
    let matching: Vec<usize> = parsed
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry_id(entry).as_deref() == Some(id))
        .map(|(index, _)| index)
        .collect();
    match matching.len() {
        0 => Ok(None),
        1 => Ok(Some(matching[0])),
        _ => Err(ConfigError::Invalid),
    }
}

fn yaml_text(value: &Yaml) -> Result<String, ConfigError> {
    serde_yaml::to_string(value).map_err(|error| ConfigError::Io(error.to_string()))
}

fn yaml_from(text: &str) -> Option<Yaml> {
    serde_yaml::from_str(text).ok()
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

/// 配置档里我们的服务商（`llm-pi-ai` 项的 `config.providers.transitlink`）。
fn provider_in(text: &str) -> Result<Option<Yaml>, ConfigError> {
    let parsed = entries(text)?;
    Ok(find(&parsed, PLUGIN)?
        .and_then(|index| parsed[index].get("config").cloned())
        .and_then(|config| config.get("providers").cloned())
        .and_then(|providers| providers.get(PROVIDER_ID).cloned()))
}

/// 设置（`Some`）或删掉（`None`）我们的服务商；`llm-pi-ai` 项里的其他服务商和设置不动。
fn set_provider(text: &str, provider: Option<&Yaml>) -> Result<String, ConfigError> {
    let parsed = entries(text)?;
    let index = find(&parsed, PLUGIN)?;
    let mut entry = match index {
        Some(index) => parsed[index].clone(),
        None if provider.is_none() => return Ok(text.to_string()),
        None => {
            let mut entry = Mapping::new();
            entry.insert(key("id"), key(PLUGIN));
            Yaml::Mapping(entry)
        }
    };
    let entry_map = entry.as_mapping_mut().ok_or(ConfigError::Invalid)?;
    let config = entry_map
        .entry(key("config"))
        .or_insert_with(|| Yaml::Mapping(Mapping::new()));
    if config.is_null() {
        *config = Yaml::Mapping(Mapping::new());
    }
    // 写成了 `!!js` 等动态值：没法只改我们的服务商。
    let config = config.as_mapping_mut().ok_or_else(|| complex(PLUGIN))?;
    let providers = config
        .entry(key("providers"))
        .or_insert_with(|| Yaml::Mapping(Mapping::new()));
    if providers.is_null() {
        *providers = Yaml::Mapping(Mapping::new());
    }
    let providers = providers.as_mapping_mut().ok_or_else(|| complex(PLUGIN))?;
    match provider {
        Some(provider) => {
            providers.insert(key(PROVIDER_ID), provider.clone());
        }
        None => {
            if providers.remove(key(PROVIDER_ID)).is_none() {
                return Ok(text.to_string());
            }
        }
    }
    splice(text, index, Some(&entry))
}

/// 配置档里的 `agent-default-model` 整项。
fn default_in(text: &str) -> Result<Option<Yaml>, ConfigError> {
    let parsed = entries(text)?;
    Ok(find(&parsed, DEFAULT_MODEL)?.map(|index| parsed[index].clone()))
}

/// 换掉（`Some`）或删掉（`None`）`agent-default-model` 整项。
fn set_default(text: &str, entry: Option<&Yaml>) -> Result<String, ConfigError> {
    let parsed = entries(text)?;
    let index = find(&parsed, DEFAULT_MODEL)?;
    if index.is_none() && entry.is_none() {
        return Ok(text.to_string());
    }
    splice(text, index, entry)
}

fn default_entry(model: &str) -> Yaml {
    let mut config = Mapping::new();
    config.insert(key("provider"), key(PROVIDER_ID));
    config.insert(key("model"), key(model));
    let mut entry = Mapping::new();
    entry.insert(key("id"), key(DEFAULT_MODEL));
    entry.insert(key("config"), Yaml::Mapping(config));
    Yaml::Mapping(entry)
}

/// `.credentials.yaml`（没有时是空的 `version: 1`）。只认第 1 版格式。
fn credentials(text: &str) -> Result<Yaml, ConfigError> {
    if text.trim().is_empty() {
        let mut document = Mapping::new();
        document.insert(key("version"), Yaml::Number(1.into()));
        return Ok(Yaml::Mapping(document));
    }
    let document: Yaml = serde_yaml::from_str(text).map_err(|_| ConfigError::Invalid)?;
    let refs_ok = document
        .get("refs")
        .is_none_or(|refs| refs.is_null() || refs.is_mapping());
    if document.get("version").and_then(Yaml::as_u64) != Some(1) || !refs_ok {
        return Err(complex(""));
    }
    Ok(document)
}

fn key_ref_in(text: &str) -> Result<Option<String>, ConfigError> {
    Ok(credentials(text)?
        .get("refs")
        .and_then(|refs| refs.get(KEY_REF))
        .and_then(Yaml::as_str)
        .map(str::to_string))
}

/// 设置（`Some`）或删掉（`None`）我们的 Key；其他引用和登录记录（`records`）不动。
fn set_key_ref(text: &str, value: Option<&str>) -> Result<String, ConfigError> {
    let mut document = credentials(text)?;
    let root = document.as_mapping_mut().ok_or(ConfigError::Invalid)?;
    let refs = root
        .entry(key("refs"))
        .or_insert_with(|| Yaml::Mapping(Mapping::new()));
    if refs.is_null() {
        *refs = Yaml::Mapping(Mapping::new());
    }
    let refs = refs.as_mapping_mut().ok_or(ConfigError::Invalid)?;
    match value {
        Some(value) => {
            if refs.get(key(KEY_REF)).and_then(Yaml::as_str) == Some(value) {
                return Ok(text.to_string());
            }
            refs.insert(key(KEY_REF), key(value));
        }
        None => {
            if refs.remove(key(KEY_REF)).is_none() {
                return Ok(text.to_string());
            }
        }
    }
    // 先试着只改我们那一行（DSH 自己写这个文件时也保留注释和格式）；改出来的内容和预期不一样
    // （例如值写成了多行）就整个重新生成。
    if let Some(edited) = edit_ref_line(text, value)
        && serde_yaml::from_str::<Yaml>(&edited).ok().as_ref() == Some(&document)
    {
        return Ok(edited);
    }
    yaml_text(&document)
}

/// 在 `refs:` 下面改、加或删 `TRANSITLINK_API_KEY` 这一行，其他行不动。看不懂的写法返回 None。
fn edit_ref_line(text: &str, value: Option<&str>) -> Option<String> {
    let rendered = |indent: &str, value: &str| {
        // JSON 字符串也是合法的 YAML 双引号字符串。
        let quoted = serde_json::to_string(value).ok()?;
        Some(format!("{indent}{KEY_REF}: {quoted}"))
    };
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = lines.iter().position(|line| {
        line.strip_prefix("refs:").is_some_and(|rest| {
            let rest = rest.trim();
            rest.is_empty() || rest.starts_with('#')
        })
    });
    let Some(header) = header else {
        // 还没有 `refs:`：加在最后。
        let value = value?;
        lines.push("refs:".to_string());
        lines.push(rendered("  ", value)?);
        return Some(lines.join(newline) + newline);
    };
    // `refs:` 下面缩进的行（中间可以有空行和注释）。
    let mut block_end = header + 1;
    let mut last_entry = None;
    let mut indent = None;
    for (index, line) in lines.iter().enumerate().skip(header + 1) {
        let blank = line.trim().is_empty();
        if !blank && !line.starts_with(' ') && !line.starts_with('#') {
            break;
        }
        block_end = index + 1;
        if !blank && !line.trim_start().starts_with('#') {
            last_entry = Some(index);
            indent.get_or_insert_with(|| line[..line.len() - line.trim_start().len()].to_string());
        }
    }
    let indent = indent.unwrap_or_else(|| "  ".to_string());
    let ours = (header + 1..block_end).find(|index| {
        let line = &lines[*index];
        line.starts_with(indent.as_str())
            && line[indent.len()..]
                .strip_prefix(KEY_REF)
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
    });
    match (ours, value) {
        (Some(index), Some(value)) => lines[index] = rendered(&indent, value)?,
        (Some(index), None) => {
            lines.remove(index);
        }
        (None, Some(value)) => {
            lines.insert(last_entry.unwrap_or(header) + 1, rendered(&indent, value)?);
        }
        (None, None) => return None,
    }
    Some(lines.join(newline) + newline)
}

/// 服务商的写法：Claude 模型走 Anthropic 接口（地址不带 `/v1`，SDK 自己接 `/v1/messages`），
/// 其他模型走 OpenAI 兼容接口（地址带 `/v1`）。中转站不一定认 `developer` 角色和
/// `max_completion_tokens`，改用 `system` 和 `max_tokens`；DeepSeek 模型的思考内容按 DeepSeek 的格式读。
fn provider_value(target: &Target) -> Yaml {
    let claude = target.model.to_ascii_lowercase().contains("claude");
    let deepseek = target.model.to_ascii_lowercase().contains("deepseek");
    let mut provider = Mapping::new();
    provider.insert(
        key("displayName"),
        key(&format!("TransitLink · {}", target.site_name)),
    );
    provider.insert(key("apiKeyEnv"), key(KEY_REF));
    if claude {
        provider.insert(key("api"), key("anthropic-messages"));
        provider.insert(key("baseURL"), key(target.base_url));
    } else {
        provider.insert(key("api"), key("openai-completions"));
        provider.insert(key("baseURL"), key(&format!("{}/v1", target.base_url)));
        let mut compat = Mapping::new();
        compat.insert(key("supportsDeveloperRole"), Yaml::Bool(false));
        compat.insert(key("maxTokensField"), key("max_tokens"));
        if deepseek {
            compat.insert(key("thinkingFormat"), key("deepseek"));
        }
        provider.insert(key("compat"), Yaml::Mapping(compat));
    }
    let mut models: Vec<&str> = target.models.iter().map(String::as_str).collect();
    if !models.contains(&target.model) {
        models.insert(0, target.model);
    }
    provider.insert(
        key("models"),
        Yaml::Sequence(models.into_iter().map(model_value).collect()),
    );
    Yaml::Mapping(provider)
}

/// 模型要写明上下文和输出上限（不写时 DSH 用很大的默认值，上限低的模型会拒绝请求）。
fn model_value(model: &str) -> Yaml {
    let (context, output) = super::opencode::limits(model);
    let mut value = Mapping::new();
    value.insert(key("id"), key(model));
    value.insert(key("contextWindow"), Yaml::Number(context.into()));
    value.insert(key("maxTokens"), Yaml::Number(output.into()));
    Yaml::Mapping(value)
}

// ---------------------------------------------------------------- 文件

/// DSH 的写锁（dsh-atomic-write 的 `withFileLock`）：独占创建 `<文件>.lock`，内容是 `<进程号>\n`，
/// 用完删掉。被占用时退避重试，最多等 [`LOCK_WAIT`]；和 DSH 一样只接管记下的进程已经不在了的锁
/// （崩溃残留），不看锁有多旧。接管前先独占创建 `<锁>.takeover-<记录的摘要>` 认领（名字和 DSH 一样），
/// 免得两个接管者一个删了旧锁、另一个又删掉刚拿到的新锁。
struct FileLock(PathBuf);

impl FileLock {
    /// 拿 `target` 的写锁（锁文件是 `<target>.lock`）。
    fn acquire(target: &Path) -> Result<Self, ConfigError> {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = PathBuf::from(format!("{}.lock", target.display()));
        let deadline = Instant::now() + LOCK_WAIT;
        let mut delay = Duration::from_millis(20);
        loop {
            match Self::create(&lock) {
                Err(ConfigError::Busy) => {
                    if take_over_exited(&lock) {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(ConfigError::Busy);
                    }
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_millis(200));
                }
                result => return result,
            }
        }
    }

    fn create(lock: &Path) -> Result<Self, ConfigError> {
        let mut file = create_private(lock).map_err(|error| {
            // Windows 上删到一半的文件再创建会报拒绝访问，也当作被占用。
            let contended = error.kind() == std::io::ErrorKind::AlreadyExists
                || (cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied);
            if contended {
                ConfigError::Busy
            } else {
                ConfigError::Io(error.to_string())
            }
        })?;
        let guard = FileLock(lock.to_path_buf());
        file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
        Ok(guard)
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// 独占创建一个只有自己能读写的文件（和 DSH 的锁文件一样是 600）。
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// 记下的进程已经不在了：接管（删掉）这把锁。返回有没有删掉。
fn take_over_exited(lock: &Path) -> bool {
    let Ok(record) = fs::read_to_string(lock) else {
        return false;
    };
    if !holder_exited(&record) {
        return false;
    }
    let digest = Sha256::digest(record.as_bytes());
    let suffix: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let claim = PathBuf::from(format!("{}.takeover-{suffix}", lock.display()));
    let Ok(mut file) = create_private(&claim) else {
        // 别人正在接管。
        return false;
    };
    let _ = file.write_all(format!("{}\n", std::process::id()).as_bytes());
    drop(file);
    // 认领以后再看一次：还是那条记录、那个进程还是不在，才删。
    let removed = fs::read_to_string(lock).ok().as_deref() == Some(record.as_str())
        && holder_exited(&record)
        && fs::remove_file(lock).is_ok();
    let _ = fs::remove_file(&claim);
    removed
}

/// 锁里记的进程（`<进程号>\n`）确定已经不在了。格式不对、是自己、查不清楚的都不算。
fn holder_exited(record: &str) -> bool {
    let Some(digits) = record.strip_suffix('\n') else {
        return false;
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Ok(pid) = digits.parse::<u32>() else {
        return false;
    };
    if pid == 0 || pid > i32::MAX as u32 || pid == std::process::id() {
        return false;
    }
    process_gone(pid)
}

/// 没有这个进程（和 Node 的 `process.kill(pid, 0)` 报 ESRCH 一样）。
#[cfg(unix)]
fn process_gone(pid: u32) -> bool {
    // SAFETY: 信号 0 只检查进程在不在，不发送任何信号。
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// 没有这个进程，或者它已经退出了（和 libuv 在 Windows 上的判断一样）。
#[cfg(windows)]
fn process_gone(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: 只查询进程状态；拿到的句柄在返回前关掉。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return GetLastError() == ERROR_INVALID_PARAMETER;
        }
        let mut code = 0u32;
        let exited = GetExitCodeProcess(handle, &mut code) != 0 && code != STILL_ACTIVE as u32;
        CloseHandle(handle);
        exited
    }
}

#[cfg(not(any(unix, windows)))]
fn process_gone(_pid: u32) -> bool {
    false
}

/// 改这个文件要拿的写锁：DSH 改配置档时（配置编辑器、插件管理）锁的是同目录的 `package.json`，
/// cc-switch 锁的是配置档本身，两个都拿；其他文件锁它自己。
fn lock_targets(path: &Path) -> Vec<PathBuf> {
    let mut targets = vec![path.to_path_buf()];
    if path
        .file_name()
        .is_some_and(|name| name == "cordis.patch.yml")
        && let Some(dir) = path.parent()
    {
        targets.push(dir.join("package.json"));
    }
    targets
}

/// 几个文件当作一次修改：按路径顺序拿锁（避免互相等），读出现在的内容交给 `plan` 算新内容
/// （`None` 表示删掉文件），只写有变化的；写到一半出错时把已经写的改回去。
fn edit_files(
    paths: &[PathBuf],
    plan: impl FnOnce(&[Option<String>]) -> Result<Vec<Option<String>>, ConfigError>,
) -> Result<(), ConfigError> {
    let mut order: Vec<PathBuf> = paths.iter().flat_map(|path| lock_targets(path)).collect();
    order.sort();
    order.dedup();
    let mut guards = Vec::with_capacity(order.len());
    for target in &order {
        guards.push(FileLock::acquire(target)?);
    }
    let current: Vec<Option<String>> = paths
        .iter()
        .map(|path| files::read(path))
        .collect::<Result<_, _>>()?;
    let next = plan(&current)?;
    if next.len() != paths.len() {
        return Err(ConfigError::Io(
            "write plan does not match the files".into(),
        ));
    }
    let mut written: Vec<usize> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if current[index] == next[index] {
            continue;
        }
        let result = match &next[index] {
            Some(text) => files::write(path, text),
            None => files::remove(path),
        };
        if let Err(error) = result {
            for done in written.into_iter().rev() {
                let _ = match &current[done] {
                    Some(text) => files::write(&paths[done], text),
                    None => files::remove(&paths[done]),
                };
            }
            return Err(error.into());
        }
        written.push(index);
    }
    drop(guards);
    Ok(())
}

/// 总配置 `$DSH_HOME/cordis.patch.yml` 在各配置档之后生效：里面的 `llm-pi-ai` 或 `agent-default-model`
/// 带了 `config`（哪怕是空的）就盖过配置档里我们写的。有的话返回这个文件。
fn home_override(home: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let path = home.join("cordis.patch.yml");
    let Some(text) = files::read(&path)? else {
        return Ok(None);
    };
    let parsed = entries(&text).map_err(|error| in_file(error, &path))?;
    let overridden = parsed.iter().any(|entry| {
        matches!(entry_id(entry).as_deref(), Some(PLUGIN | DEFAULT_MODEL))
            && entry.get("config").is_some()
    });
    Ok(overridden.then_some(path))
}

fn check_not_shadowed(home: &Path) -> Result<(), ConfigError> {
    match home_override(home)? {
        Some(path) => Err(ConfigError::Unsupported(path.display().to_string())),
        None => Ok(()),
    }
}

/// 一键配置前先确认这些文件都能改（不写）：免得在站点上建好 Key 才发现写不进去。
pub fn check(home: &Path) -> Result<(), ConfigError> {
    check_not_shadowed(home)?;
    let path = credentials_path(home);
    let text = files::read(&path)?.unwrap_or_default();
    set_key_ref(&text, Some("check")).map_err(|error| in_file(error, &path))?;
    let probe = Target {
        site_name: "",
        base_url: "https://relay.example.com",
        token: "",
        model: "check",
        models: &[],
    };
    for name in profiles(home) {
        let path = profile_path(home, name);
        let text = files::read(&path)?.unwrap_or_default();
        set_provider(&text, Some(&provider_value(&probe)))
            .and_then(|next| set_default(&next, Some(&default_entry("check"))))
            .map_err(|error| in_file(error, &path))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- 配置

pub struct Target<'a> {
    pub site_name: &'a str,
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: &'a str,
    /// 站点上同一类的模型，都写进服务商的模型列表。
    pub models: &'a [String],
}

/// 第一次配置前的原样：每个配置档里我们的服务商和默认模型那一项、Key 引用。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    credentials_existed: bool,
    key_ref: Option<String>,
    profiles: Vec<ProfileSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSnapshot {
    name: String,
    existed: bool,
    /// 原来就有 `llm-pi-ai` 这一项（没有的话，这一项是我们加的，还原时只剩空服务商就整项删掉）。
    #[serde(default)]
    plugin_existed: bool,
    provider: Option<String>,
    default: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Current {
    /// 默认模型用的服务商。
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
}

/// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过的配置档沿用它，新出现的配置档补上。
pub fn apply(
    home: &Path,
    target: &Target,
    previous: Option<&Snapshot>,
) -> Result<Snapshot, ConfigError> {
    check_not_shadowed(home)?;
    let names = profiles(home);
    let mut paths = vec![credentials_path(home)];
    paths.extend(names.iter().map(|name| profile_path(home, name)));
    let provider = provider_value(target);
    let default = default_entry(target.model);
    let mut snapshot = previous.cloned().unwrap_or_default();
    let first = previous.is_none();
    edit_files(&paths, |current| {
        if first {
            snapshot.credentials_existed = current[0].is_some();
            snapshot.key_ref = key_ref_in(current[0].as_deref().unwrap_or_default())?;
        }
        let mut next = vec![Some(
            set_key_ref(
                current[0].as_deref().unwrap_or_default(),
                Some(target.token),
            )
            .map_err(|error| in_file(error, &paths[0]))?,
        )];
        for (offset, name) in names.iter().enumerate() {
            let path = &paths[offset + 1];
            let text = current[offset + 1].as_deref().unwrap_or_default();
            let mut edit = || -> Result<String, ConfigError> {
                if !snapshot.profiles.iter().any(|saved| saved.name == *name) {
                    snapshot.profiles.push(ProfileSnapshot {
                        name: name.to_string(),
                        existed: current[offset + 1].is_some(),
                        plugin_existed: find(&entries(text)?, PLUGIN)?.is_some(),
                        provider: provider_in(text)?
                            .map(|value| yaml_text(&value))
                            .transpose()?,
                        default: default_in(text)?
                            .map(|value| yaml_text(&value))
                            .transpose()?,
                    });
                }
                let with_provider = set_provider(text, Some(&provider))?;
                set_default(&with_provider, Some(&default))
            };
            next.push(Some(edit().map_err(|error| in_file(error, path))?));
        }
        Ok(next)
    })?;
    Ok(snapshot)
}

/// 换模型：每个配置档的默认模型换成它，服务商的模型列表里没有就加上。
pub fn set_model(home: &Path, model: &str) -> Result<(), ConfigError> {
    let names = profiles(home);
    let paths: Vec<PathBuf> = names.iter().map(|name| profile_path(home, name)).collect();
    edit_files(&paths, |current| {
        current
            .iter()
            .zip(&paths)
            .map(|(text, path)| {
                // 配置档不在了（例如被重置）：不建空文件（DSH 读到空文件会启动失败）。
                let Some(text) = text.as_deref() else {
                    return Ok(None);
                };
                let edit = || -> Result<Option<String>, ConfigError> {
                    let Some(mut provider) = provider_in(text)? else {
                        return Ok(Some(text.to_string()));
                    };
                    if let Some(models) = provider.get_mut("models").and_then(Yaml::as_sequence_mut)
                        && !models
                            .iter()
                            .any(|item| item.get("id").and_then(Yaml::as_str) == Some(model))
                    {
                        models.push(model_value(model));
                    }
                    let with_provider = set_provider(text, Some(&provider))?;
                    Ok(Some(set_default(
                        &with_provider,
                        Some(&default_entry(model)),
                    )?))
                };
                edit().map_err(|error| in_file(error, path))
            })
            .collect()
    })
}

/// 还原：每个配置档里我们的服务商和默认模型放回原样（原来没有的删掉），Key 引用也放回原值。
/// 配置档是我们建的、撤掉以后什么都不剩时删掉文件；DSH 后来写进去的设置留着。
pub fn restore(home: &Path, snapshot: &Snapshot) -> Result<(), ConfigError> {
    let mut paths = vec![credentials_path(home)];
    paths.extend(
        snapshot
            .profiles
            .iter()
            .map(|saved| profile_path(home, &saved.name)),
    );
    edit_files(&paths, |current| {
        let credentials = current[0].as_deref().unwrap_or_default();
        let restored = set_key_ref(credentials, snapshot.key_ref.as_deref())
            .map_err(|error| in_file(error, &paths[0]))?;
        let empty = credentials_empty(&restored).map_err(|error| in_file(error, &paths[0]))?;
        // 文件是我们建的、现在又空了，或者本来就没有文件也没有要放回的 Key：不留文件。
        let drop_file = (!snapshot.credentials_existed && empty)
            || (current[0].is_none() && snapshot.key_ref.is_none());
        let mut next = vec![if drop_file { None } else { Some(restored) }];
        for (offset, saved) in snapshot.profiles.iter().enumerate() {
            let path = &paths[offset + 1];
            let Some(text) = current[offset + 1].as_deref() else {
                next.push(None);
                continue;
            };
            let edit = || -> Result<Option<String>, ConfigError> {
                let provider = saved.provider.as_deref().and_then(yaml_from);
                let default = saved.default.as_deref().and_then(yaml_from);
                let mut restored = set_provider(text, provider.as_ref())?;
                if !saved.plugin_existed {
                    restored = remove_empty_plugin(&restored)?;
                }
                let restored = set_default(&restored, default.as_ref())?;
                // 我们建的配置档、现在一项都不剩：删掉，DSH 第一次运行时会自己建。
                if !saved.existed && entries(&restored)?.is_empty() {
                    return Ok(None);
                }
                Ok(Some(restored))
            };
            next.push(edit().map_err(|error| in_file(error, path))?);
        }
        Ok(next)
    })
}

/// `llm-pi-ai` 这一项只剩空的服务商（`config: {providers: {}}`）时整项删掉：这一项会整个替换 DSH
/// 自带的设置，留着空的会改变 DSH 原来的行为。
fn remove_empty_plugin(text: &str) -> Result<String, ConfigError> {
    let parsed = entries(text)?;
    let Some(index) = find(&parsed, PLUGIN)? else {
        return Ok(text.to_string());
    };
    let entry = parsed[index].as_mapping().ok_or(ConfigError::Invalid)?;
    let config = entry.get(key("config")).and_then(Yaml::as_mapping);
    let only_empty_providers = entry.len() == 2
        && config.is_some_and(|config| {
            config.len() == 1
                && config
                    .get(key("providers"))
                    .and_then(Yaml::as_mapping)
                    .is_some_and(Mapping::is_empty)
        });
    if only_empty_providers {
        splice(text, Some(index), None)
    } else {
        Ok(text.to_string())
    }
}

fn credentials_empty(text: &str) -> Result<bool, ConfigError> {
    let document = credentials(text)?;
    let empty = |name: &str| {
        document.get(name).is_none_or(|value| {
            value.is_null() || value.as_mapping().is_some_and(Mapping::is_empty)
        })
    };
    Ok(empty("refs") && empty("records"))
}

/// 读出现在的连接：看 `web` 配置档（没有时看 `desktop`）。总配置盖过了配置档时，算作用的是别的
/// （`provider` 是总配置文件的位置）。
pub fn read(home: &Path) -> Result<Current, ConfigError> {
    if let Some(path) = home_override(home)? {
        return Ok(Current {
            provider: Some(path.display().to_string()),
            ..Current::default()
        });
    }
    let path = PROFILES
        .iter()
        .map(|name| profile_path(home, name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| profile_path(home, PROFILES[0]));
    let text = files::read(&path)?.unwrap_or_default();
    let default = default_in(&text).map_err(|error| in_file(error, &path))?;
    let config = default.as_ref().and_then(|entry| entry.get("config"));
    let provider = config
        .and_then(|config| config.get("provider"))
        .and_then(Yaml::as_str)
        .map(str::to_string);
    let model = config
        .and_then(|config| config.get("model"))
        .and_then(Yaml::as_str)
        .map(str::to_string);
    let ours = provider.as_deref() == Some(PROVIDER_ID);
    let entry = if ours {
        provider_in(&text).map_err(|error| in_file(error, &path))?
    } else {
        None
    };
    let base_url = entry
        .as_ref()
        .and_then(|entry| entry.get("baseURL"))
        .and_then(Yaml::as_str)
        .map(|url| {
            url.trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_string()
        });
    let reference = entry
        .as_ref()
        .and_then(|entry| entry.get("apiKeyEnv"))
        .and_then(Yaml::as_str)
        .map(str::to_string);
    let token = match reference {
        Some(reference) => {
            let credentials_text = files::read(&credentials_path(home))?.unwrap_or_default();
            credentials(&credentials_text).ok().and_then(|document| {
                document
                    .get("refs")
                    .and_then(|refs| refs.get(reference.as_str()))
                    .and_then(Yaml::as_str)
                    .map(str::to_string)
            })
        }
        None => None,
    };
    Ok(Current {
        provider,
        base_url,
        token,
        model: model.filter(|_| ours),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("transitlink-dsh-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn target<'a>(model: &'a str, models: &'a [String]) -> Target<'a> {
        Target {
            site_name: "示例中转站",
            base_url: "https://relay.example.com",
            token: "sk-relay",
            model,
            models,
        }
    }

    #[test]
    fn references_are_detected_outside_quotes_and_comments() {
        assert!(uses_references("  config: !!js/function x"));
        assert!(uses_references("- &base"));
        assert!(uses_references("  value: *base"));
        assert!(uses_references("  <<: *base"));
        assert!(!uses_references("  name: \"a & b * c!\""));
        assert!(!uses_references("  name: 'it''s !fine'"));
        assert!(!uses_references(
            "  url: https://x.example.com/a&b # not &anchor"
        ));
        assert!(!uses_references("- id: llm-pi-ai"));
    }

    #[test]
    fn only_our_entries_change_and_everything_else_stays() {
        let original = "# 我的设置\n- id: llm-deepseek\n  config:\n    model: !!js/function \"() => 1\"\n- id: llm-pi-ai\n  config:\n    providers:\n      mine:\n        api: openai-completions\n        baseURL: http://localhost:8080/v1\n        models:\n          - id: local\n";
        let models = vec!["claude-sonnet-5-5".to_string()];
        let target = target("claude-sonnet-5-5", &models);
        let with_provider = set_provider(original, Some(&provider_value(&target))).unwrap();
        let next = set_default(&with_provider, Some(&default_entry("claude-sonnet-5-5"))).unwrap();
        // 别的项（包括 !!js）和注释原样保留。
        assert!(next.starts_with(
            "# 我的设置\n- id: llm-deepseek\n  config:\n    model: !!js/function \"() => 1\"\n"
        ));
        let parsed = entries(&next).unwrap();
        let plugin = &parsed[find(&parsed, PLUGIN).unwrap().unwrap()];
        let providers = plugin.get("config").unwrap().get("providers").unwrap();
        assert!(providers.get("mine").is_some(), "other providers are kept");
        let ours = providers.get(PROVIDER_ID).unwrap();
        assert_eq!(
            ours.get("api").unwrap().as_str(),
            Some("anthropic-messages")
        );
        assert_eq!(
            ours.get("baseURL").unwrap().as_str(),
            Some("https://relay.example.com")
        );
        assert_eq!(ours.get("apiKeyEnv").unwrap().as_str(), Some(KEY_REF));
        let default = default_in(&next).unwrap().unwrap();
        assert_eq!(
            default
                .get("config")
                .unwrap()
                .get("provider")
                .unwrap()
                .as_str(),
            Some(PROVIDER_ID)
        );

        // 撤掉以后回到原样。
        let without = set_provider(&set_default(&next, None).unwrap(), None).unwrap();
        assert_eq!(entries(&without).unwrap(), entries(original).unwrap());
    }

    #[test]
    fn entries_with_tags_or_flow_style_are_not_rewritten() {
        let tagged = "- id: llm-pi-ai\n  config: !!js/function \"() => ({})\"\n";
        let models = vec!["gpt-6.1-sol".to_string()];
        let value = provider_value(&target("gpt-6.1-sol", &models));
        assert!(matches!(
            set_provider(tagged, Some(&value)),
            Err(ConfigError::Unsupported(_))
        ));
        let flow = "[{ id: hmr }]\n";
        assert!(matches!(
            set_provider(flow, Some(&value)),
            Err(ConfigError::Unsupported(_))
        ));
        // 新配置档（注释加 []）可以写。
        let fresh = "# DSH profile\n[]\n";
        let next = set_provider(fresh, Some(&value)).unwrap();
        assert!(next.starts_with("# DSH profile\n- id: llm-pi-ai\n"));
        assert!(provider_in(&next).unwrap().is_some());
    }

    #[test]
    fn openai_compatible_providers_suit_relays() {
        let models = vec!["deepseek-v4".to_string(), "deepseek-v4-flash".to_string()];
        let value = provider_value(&target("deepseek-v4", &models));
        assert_eq!(
            value.get("api").unwrap().as_str(),
            Some("openai-completions")
        );
        assert_eq!(
            value.get("baseURL").unwrap().as_str(),
            Some("https://relay.example.com/v1")
        );
        let compat = value.get("compat").unwrap();
        assert_eq!(
            compat.get("supportsDeveloperRole").unwrap().as_bool(),
            Some(false)
        );
        assert_eq!(
            compat.get("maxTokensField").unwrap().as_str(),
            Some("max_tokens")
        );
        assert_eq!(
            compat.get("thinkingFormat").unwrap().as_str(),
            Some("deepseek")
        );
        let models = value.get("models").unwrap().as_sequence().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].get("maxTokens").unwrap().as_u64(), Some(8192));
    }

    #[test]
    fn credentials_keep_other_refs_and_records() {
        let original = "version: 1\nrefs:\n  MINE: abc\nrecords:\n  deepseek-account-platform/default:\n    kind: grant\n";
        let next = set_key_ref(original, Some("sk-relay")).unwrap();
        let document: Yaml = serde_yaml::from_str(&next).unwrap();
        assert_eq!(
            document.get("refs").unwrap().get("MINE").unwrap().as_str(),
            Some("abc")
        );
        assert_eq!(
            document.get("refs").unwrap().get(KEY_REF).unwrap().as_str(),
            Some("sk-relay")
        );
        assert!(
            document
                .get("records")
                .unwrap()
                .get("deepseek-account-platform/default")
                .is_some()
        );
        let back = set_key_ref(&next, None).unwrap();
        let document: Yaml = serde_yaml::from_str(&back).unwrap();
        assert!(document.get("refs").unwrap().get(KEY_REF).is_none());
        assert!(matches!(
            set_key_ref("version: 2\n", Some("x")),
            Err(ConfigError::Unsupported(_))
        ));
    }

    #[test]
    fn applies_reads_switches_models_and_restores_on_disk() {
        let home = temp_home("flow");
        // 已经运行过的 web 配置档（有自己的设置）；desktop 还没运行过。
        let web = profile_path(&home, "web");
        fs::create_dir_all(web.parent().unwrap()).unwrap();
        let web_before = "# web\n- id: hmr\n  config:\n    enabled: false\n";
        fs::write(&web, web_before).unwrap();
        let models = vec![
            "claude-sonnet-5-5".to_string(),
            "claude-haiku-5-5".to_string(),
        ];

        let snapshot = apply(&home, &target("claude-sonnet-5-5", &models), None).unwrap();
        let current = read(&home).unwrap();
        assert_eq!(current.provider.as_deref(), Some(PROVIDER_ID));
        assert_eq!(
            current.base_url.as_deref(),
            Some("https://relay.example.com")
        );
        assert_eq!(current.token.as_deref(), Some("sk-relay"));
        assert_eq!(current.model.as_deref(), Some("claude-sonnet-5-5"));
        assert!(profile_path(&home, "desktop").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(credentials_path(&home))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "DSH refuses credentials readable by others");
        }
        // 没有残留的锁。
        assert!(!PathBuf::from(format!("{}.lock", web.display())).exists());

        set_model(&home, "claude-haiku-5-5").unwrap();
        assert_eq!(
            read(&home).unwrap().model.as_deref(),
            Some("claude-haiku-5-5")
        );

        restore(&home, &snapshot).unwrap();
        // 原来没有 llm-pi-ai 项：还原后不留下空的服务商项（它会替换 DSH 自带的设置）。
        assert_eq!(fs::read_to_string(&web).unwrap(), web_before);
        assert!(!profile_path(&home, "desktop").exists());
        assert!(!credentials_path(&home).exists());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_home_level_provider_config_blocks_the_setup() {
        let home = temp_home("shadow");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join("cordis.patch.yml"),
            "- id: llm-pi-ai\n  config:\n    providers: {}\n",
        )
        .unwrap();
        let models = vec!["gpt-6.1-sol".to_string()];
        assert!(matches!(
            apply(&home, &target("gpt-6.1-sol", &models), None),
            Err(ConfigError::Unsupported(_))
        ));
        assert!(!profile_path(&home, "web").exists());
        let _ = fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn locks_held_by_a_live_process_are_respected() {
        let home = temp_home("lock");
        let web = profile_path(&home, "web");
        fs::create_dir_all(web.parent().unwrap()).unwrap();
        let models = vec!["gpt-6.1-sol".to_string()];
        let alive = format!("{}\n", std::os::unix::process::parent_id());
        // DSH 改配置档时锁的是 package.json；cc-switch 锁的是配置档本身。
        for lock in ["package.json.lock", "cordis.patch.yml.lock"] {
            let path = web.parent().unwrap().join(lock);
            fs::write(&path, &alive).unwrap();
            assert_eq!(
                apply(&home, &target("gpt-6.1-sol", &models), None),
                Err(ConfigError::Busy)
            );
            assert!(path.exists(), "a live holder keeps its lock");
            fs::remove_file(&path).unwrap();
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn locks_left_by_an_exited_process_are_taken_over() {
        let home = temp_home("stale");
        let web = profile_path(&home, "web");
        fs::create_dir_all(web.parent().unwrap()).unwrap();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        let lock = web.parent().unwrap().join("package.json.lock");
        fs::write(&lock, format!("{pid}\n")).unwrap();
        // 格式不对的记录（例如别人写到一半）不接管。
        let odd = credentials_path(&home).with_extension("yaml.lock");
        let models = vec!["gpt-6.1-sol".to_string()];
        apply(&home, &target("gpt-6.1-sol", &models), None).unwrap();
        assert!(!lock.exists());
        let leftovers: Vec<_> = fs::read_dir(web.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".lock"))
            .collect();
        assert!(leftovers.is_empty(), "no lock or takeover claim is left");
        fs::write(&odd, "").unwrap();
        assert_eq!(
            apply(&home, &target("gpt-6.1-sol", &models), None),
            Err(ConfigError::Busy)
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn comments_between_entries_stay() {
        let original = "# 头\n- id: agent-default-model\n  config:\n    provider: mine\n    model: local\n\n# 下面是我的插件\n- id: hmr\n  config:\n    enabled: false\n# - id: disabled-plugin\n";
        let next = set_default(original, Some(&default_entry("gpt-6.1-sol"))).unwrap();
        assert!(next.contains("\n\n# 下面是我的插件\n- id: hmr\n"));
        assert!(next.starts_with("# 头\n- id: agent-default-model\n"));
        // 最后一项后面的注释也留着。
        let models = vec!["gpt-6.1-sol".to_string()];
        let with_provider = set_provider(
            &next,
            Some(&provider_value(&target("gpt-6.1-sol", &models))),
        )
        .unwrap();
        assert!(with_provider.contains("# - id: disabled-plugin\n"));
        let removed = set_default(&set_provider(&with_provider, None).unwrap(), None).unwrap();
        assert!(removed.contains("# 下面是我的插件\n- id: hmr\n"));
        assert!(removed.contains("# - id: disabled-plugin\n"));
    }

    #[test]
    fn credentials_change_only_our_line() {
        let original = "# DSH 的凭据\nversion: 1\nrefs:\n  # 我自己的\n  MINE: abc\nrecords:\n  deepseek-account-platform/default:\n    kind: grant\n";
        let next = set_key_ref(original, Some("sk-relay")).unwrap();
        assert_eq!(
            next,
            "# DSH 的凭据\nversion: 1\nrefs:\n  # 我自己的\n  MINE: abc\n  TRANSITLINK_API_KEY: \"sk-relay\"\nrecords:\n  deepseek-account-platform/default:\n    kind: grant\n"
        );
        let changed = set_key_ref(&next, Some("sk-other")).unwrap();
        assert!(changed.contains("  TRANSITLINK_API_KEY: \"sk-other\"\n"));
        assert_eq!(set_key_ref(&changed, None).unwrap(), original);
        // 还没有 refs：加在最后。
        let bare = set_key_ref("version: 1\n", Some("sk-relay")).unwrap();
        assert_eq!(
            bare,
            "version: 1\nrefs:\n  TRANSITLINK_API_KEY: \"sk-relay\"\n"
        );
        // 写成行内的 refs：整个重新生成，内容照样对。
        let flow = set_key_ref("version: 1\nrefs: {MINE: abc}\n", Some("sk-relay")).unwrap();
        assert_eq!(key_ref_in(&flow).unwrap().as_deref(), Some("sk-relay"));
    }

    #[test]
    fn restoring_keeps_what_dsh_added_to_a_profile_we_created() {
        let home = temp_home("created");
        let models = vec!["deepseek-v4".to_string()];
        let snapshot = apply(&home, &target("deepseek-v4", &models), None).unwrap();
        // DSH 第一次运行后自己往这个配置档里加了设置。
        let desktop = profile_path(&home, "desktop");
        let mut text = fs::read_to_string(&desktop).unwrap();
        text.push_str("- id: hmr\n  config:\n    enabled: false\n");
        fs::write(&desktop, &text).unwrap();
        restore(&home, &snapshot).unwrap();
        assert_eq!(
            fs::read_to_string(&desktop).unwrap(),
            "- id: hmr\n  config:\n    enabled: false\n"
        );
        // 只有我们写的那个配置档删掉了。
        assert!(!profile_path(&home, "web").exists());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn changing_the_model_leaves_missing_profiles_alone() {
        let home = temp_home("missing");
        let models = vec!["deepseek-v4".to_string(), "deepseek-v4-flash".to_string()];
        apply(&home, &target("deepseek-v4", &models), None).unwrap();
        fs::remove_file(profile_path(&home, "desktop")).unwrap();
        set_model(&home, "deepseek-v4-flash").unwrap();
        assert!(
            !profile_path(&home, "desktop").exists(),
            "no empty profile is created"
        );
        assert_eq!(
            read(&home).unwrap().model.as_deref(),
            Some("deepseek-v4-flash")
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_home_level_default_model_counts_as_another_setup() {
        let home = temp_home("home-default");
        let models = vec!["deepseek-v4".to_string()];
        apply(&home, &target("deepseek-v4", &models), None).unwrap();
        let patch = home.join("cordis.patch.yml");
        fs::write(&patch, "- id: agent-default-model\n  config:\n    provider: deepseek\n    model: deepseek-v4\n").unwrap();
        let current = read(&home).unwrap();
        assert_eq!(
            current.provider.as_deref(),
            Some(patch.display().to_string().as_str())
        );
        assert_eq!(current.base_url, None);
        assert!(matches!(check(&home), Err(ConfigError::Unsupported(_))));
        // 只带 id、不带 config 的行不算。
        fs::write(&patch, "- id: agent-default-model\n").unwrap();
        assert_eq!(read(&home).unwrap().provider.as_deref(), Some(PROVIDER_ID));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn the_check_before_creating_a_key_reads_every_profile() {
        let home = temp_home("check");
        assert_eq!(check(&home), Ok(()));
        let desktop = profile_path(&home, "desktop");
        fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        fs::write(&desktop, "[{ id: hmr }]\n").unwrap();
        match check(&home) {
            Err(ConfigError::Unsupported(what)) => assert!(what.contains("desktop")),
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            !profile_path(&home, "web").exists(),
            "the check writes nothing"
        );
        let _ = fs::remove_dir_all(&home);
    }
}
