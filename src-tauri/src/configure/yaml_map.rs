//! 修改 YAML 映射里的一项（例如 `providers.transitlink`、`modelRoles.default`）：尽量只改那几行，
//! 其余内容、注释和格式不动。改完重新解析，和预期不一样时（例如写成了行内的 `{...}`、用了锚点）
//! 整个重新生成（这时注释会丢，但内容一定对）。

use serde_yaml::{Mapping, Value as Yaml};

use super::ConfigError;

/// 解析成映射（空文件是空映射）。顶层不是映射时算格式有误。
pub fn parse(text: &str) -> Result<Yaml, ConfigError> {
    match serde_yaml::from_str::<Yaml>(super::multi::strip_bom(text))
        .map_err(|_| ConfigError::Invalid)?
    {
        Yaml::Null => Ok(Yaml::Mapping(Mapping::new())),
        value @ Yaml::Mapping(_) => Ok(value),
        _ => Err(ConfigError::Invalid),
    }
}

/// 取一项。
pub fn get<'a>(doc: &'a Yaml, path: &[&str]) -> Option<&'a Yaml> {
    let mut current = doc;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

/// 一项的 YAML 文本（记原值用）。
pub fn saved(doc: &Yaml, path: &[&str]) -> Option<String> {
    get(doc, path).and_then(|value| serde_yaml::to_string(value).ok())
}

/// 记下的原值转回来。
pub fn from_saved(text: Option<&str>) -> Option<Yaml> {
    text.and_then(|text| serde_yaml::from_str(text).ok())
}

/// 把 `path` 设成 `value`（None 时删掉这一项，删完空了的上层映射也删掉）。返回新内容。
pub fn set(text: &str, path: &[&str], value: Option<&Yaml>) -> Result<String, ConfigError> {
    let text = super::multi::strip_bom(text);
    let mut expected = parse(text)?;
    if !change(&mut expected, path, value)? {
        return Ok(text.to_string());
    }
    if let Some(edited) = edit(text, path, value)
        && parse(&edited).ok().as_ref() == Some(&expected)
    {
        return Ok(edited);
    }
    render(&expected)
}

/// 整个映射空了。
pub fn is_empty(text: &str) -> bool {
    parse(text).is_ok_and(|doc| doc.as_mapping().is_some_and(Mapping::is_empty))
}

fn render(doc: &Yaml) -> Result<String, ConfigError> {
    if doc.as_mapping().is_some_and(Mapping::is_empty) {
        return Ok(String::new());
    }
    serde_yaml::to_string(doc).map_err(|error| ConfigError::Io(error.to_string()))
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

/// 在解析出来的值上改，返回有没有变化。上层不是映射（例如是列表）时算没法安全修改。
fn change(doc: &mut Yaml, path: &[&str], value: Option<&Yaml>) -> Result<bool, ConfigError> {
    let Some((last, parents)) = path.split_last() else {
        return Ok(false);
    };
    let Some(value) = value else {
        return Ok(remove(doc, path));
    };
    let mut current = doc;
    for name in parents {
        let map = current
            .as_mapping_mut()
            .ok_or_else(|| ConfigError::Unsupported(String::new()))?;
        let entry = map.entry(key(name)).or_insert(Yaml::Null);
        if entry.is_null() {
            *entry = Yaml::Mapping(Mapping::new());
        }
        current = entry;
    }
    let map = current
        .as_mapping_mut()
        .ok_or_else(|| ConfigError::Unsupported(String::new()))?;
    if map.get(*last) == Some(value) {
        return Ok(false);
    }
    map.insert(key(last), value.clone());
    Ok(true)
}

fn remove(node: &mut Yaml, path: &[&str]) -> bool {
    let Some(map) = node.as_mapping_mut() else {
        return false;
    };
    if path.len() == 1 {
        return map.remove(path[0]).is_some();
    }
    let Some(child) = map.get_mut(path[0]) else {
        return false;
    };
    let removed = remove(child, &path[1..]);
    if removed && child.as_mapping().is_some_and(Mapping::is_empty) {
        map.remove(path[0]);
    }
    removed
}

fn content(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// 这一行是不是缩进正好 `indent` 的 `键:`，是的话返回键名。
fn key_at(line: &str, indent: usize) -> Option<String> {
    if indent_of(line) != indent {
        return None;
    }
    let rest = &line[indent..];
    let (name, after) = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        (quoted[..end].to_string(), &quoted[end + 1..])
    } else if let Some(quoted) = rest.strip_prefix('\'') {
        let end = quoted.find('\'')?;
        (quoted[..end].to_string(), &quoted[end + 1..])
    } else {
        let end = rest.find(':')?;
        (rest[..end].trim_end().to_string(), &rest[end..])
    };
    let after = after.strip_prefix(':')?;
    (after.is_empty() || after.starts_with(' ')).then_some(name)
}

/// `键:` 后面还写了值（不是空、也不只是注释）：下面没有块状的子项。
fn has_inline_value(line: &str) -> bool {
    let Some(colon) = line.find(':') else {
        return true;
    };
    let rest = line[colon + 1..].trim();
    !rest.is_empty() && !rest.starts_with('#')
}

/// 第 `line` 行这一项到哪里结束：后面缩进更深的行都属于它；列表项可以和键一样缩进（`key:` 下一行
/// 直接是 `- …`），也属于它。末尾的空行和注释不算。
fn block_end(lines: &[String], line: usize, end: usize, indent: usize) -> usize {
    let mut last = line;
    for (index, text) in lines.iter().enumerate().take(end).skip(line + 1) {
        if !content(text) {
            continue;
        }
        let deeper = indent_of(text) > indent;
        let item = indent_of(text) == indent && text[indent..].starts_with('-');
        if !deeper && !item {
            break;
        }
        last = index;
    }
    last + 1
}

/// `键: 值` 写成几行，前面加上缩进。
fn render_entry(name: &str, value: &Yaml, indent: usize) -> Option<Vec<String>> {
    let mut map = Mapping::new();
    map.insert(key(name), value.clone());
    let text = serde_yaml::to_string(&Yaml::Mapping(map)).ok()?;
    let pad = " ".repeat(indent);
    Some(text.lines().map(|line| format!("{pad}{line}")).collect())
}

/// 删掉一项以后，下面已经没有内容的上层键也删掉（和 `remove` 一样），留下的注释不动。
fn prune(lines: &mut Vec<String>, ancestors: &[(usize, usize)]) {
    for &(line, indent) in ancestors.iter().rev() {
        if block_end(lines, line, lines.len(), indent) != line + 1 {
            break;
        }
        lines.remove(line);
    }
}

/// 剩下的路径包成一层层的映射。
fn nest(rest: &[&str], value: &Yaml) -> Yaml {
    rest.iter().rev().fold(value.clone(), |inner, name| {
        let mut map = Mapping::new();
        map.insert(key(name), inner);
        Yaml::Mapping(map)
    })
}

/// 按行改：找到这一项就换掉（或删掉）那几行；没有就加在上一层的最后。看不懂的写法返回 None。
fn edit(text: &str, path: &[&str], value: Option<&Yaml>) -> Option<String> {
    if text.contains('\t') {
        return None;
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let join = |lines: Vec<String>| {
        if lines.is_empty() {
            return String::new();
        }
        let mut out = lines.join(newline);
        out.push_str(newline);
        out
    };
    let (mut start, mut end) = (0usize, lines.len());
    let mut parent: Option<usize> = None;
    let mut ancestors: Vec<(usize, usize)> = Vec::new();
    for (depth, name) in path.iter().enumerate() {
        let first_child = (start..end).find(|index| content(&lines[*index]));
        let indent = match (parent, first_child) {
            (None, _) => 0,
            (Some(_), Some(index)) => indent_of(&lines[index]),
            (Some(parent_indent), None) => parent_indent + 2,
        };
        if parent.is_some_and(|parent_indent| indent <= parent_indent) {
            return None;
        }
        if first_child.is_some_and(|index| lines[index].trim_start().starts_with("- ")) {
            return None;
        }
        let found =
            (start..end).find(|index| key_at(&lines[*index], indent).as_deref() == Some(*name));
        let last = depth + 1 == path.len();
        match found {
            Some(line) => {
                let block = block_end(&lines, line, end, indent);
                if last {
                    let replacement = match value {
                        Some(value) => render_entry(name, value, indent)?,
                        None => Vec::new(),
                    };
                    lines.splice(line..block, replacement);
                    if value.is_none() {
                        prune(&mut lines, &ancestors);
                    }
                    return Some(join(lines));
                }
                if has_inline_value(&lines[line]) {
                    return None;
                }
                ancestors.push((line, indent));
                start = line + 1;
                end = block;
                parent = Some(indent);
            }
            None => {
                let value = value?;
                let entry = render_entry(name, &nest(&path[depth + 1..], value), indent)?;
                let at = (start..end)
                    .rev()
                    .find(|index| content(&lines[*index]))
                    .map_or(start, |index| index + 1);
                lines.splice(at..at, entry);
                return Some(join(lines));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Yaml {
        serde_yaml::from_str(text).unwrap()
    }

    #[test]
    fn replaces_inserts_and_removes_only_our_lines() {
        let original = "# 我的模型\nproviders:\n  mine:\n    baseUrl: http://localhost:8080/v1 # 本地\n    models:\n      - id: local\n\n# 结尾的注释\ntheme: dark\n";
        let ours = value("baseUrl: https://relay.example.com/v1\napiKey: sk-relay\n");
        let added = set(original, &["providers", "transitlink"], Some(&ours)).unwrap();
        assert!(added.starts_with(
            "# 我的模型\nproviders:\n  mine:\n    baseUrl: http://localhost:8080/v1 # 本地\n"
        ));
        assert!(added.contains("  transitlink:\n    baseUrl: https://relay.example.com/v1\n"));
        assert!(added.contains("# 结尾的注释\ntheme: dark\n"));
        // 换成新的值：只换我们那几行。
        let changed = set(
            &added,
            &["providers", "transitlink", "apiKey"],
            Some(&value("sk-new")),
        )
        .unwrap();
        assert!(changed.contains("    apiKey: sk-new\n"));
        assert!(changed.contains("# 本地"));
        // 删掉以后回到原样。
        assert_eq!(
            set(&changed, &["providers", "transitlink"], None).unwrap(),
            original
        );
    }

    #[test]
    fn creates_missing_parents_and_prunes_empty_ones() {
        let text = set(
            "",
            &["modelRoles", "default"],
            Some(&value("transitlink/gpt-6")),
        )
        .unwrap();
        assert_eq!(text, "modelRoles:\n  default: transitlink/gpt-6\n");
        let back = set(&text, &["modelRoles", "default"], None).unwrap();
        assert_eq!(back, "");
        // 上层是我们加的：删掉时整个去掉，用户的注释和其他项不动（不会整个重新生成）。
        let commented = "# 主题\ntheme: dark # 深色\n";
        let added = set(
            commented,
            &["modelRoles", "default"],
            Some(&value("transitlink/gpt-6")),
        )
        .unwrap();
        assert_eq!(
            added,
            "# 主题\ntheme: dark # 深色\nmodelRoles:\n  default: transitlink/gpt-6\n"
        );
        assert_eq!(
            set(&added, &["modelRoles", "default"], None).unwrap(),
            commented
        );
        let with_other = "modelRoles:\n  smol: anthropic/claude-haiku\n";
        let next = set(
            with_other,
            &["modelRoles", "default"],
            Some(&value("transitlink/gpt-6")),
        )
        .unwrap();
        assert_eq!(
            next,
            "modelRoles:\n  smol: anthropic/claude-haiku\n  default: transitlink/gpt-6\n"
        );
        assert_eq!(
            set(&next, &["modelRoles", "default"], None).unwrap(),
            with_other
        );
    }

    #[test]
    fn falls_back_to_rewriting_unusual_layouts() {
        let flow = "providers: {mine: {baseUrl: x}}\n";
        let next = set(
            flow,
            &["providers", "transitlink"],
            Some(&value("apiKey: k")),
        )
        .unwrap();
        let doc = parse(&next).unwrap();
        assert_eq!(
            get(&doc, &["providers", "mine", "baseUrl"]).and_then(Yaml::as_str),
            Some("x")
        );
        assert_eq!(
            get(&doc, &["providers", "transitlink", "apiKey"]).and_then(Yaml::as_str),
            Some("k")
        );
        assert!(matches!(
            set("- a\n", &["x"], Some(&value("1"))),
            Err(ConfigError::Invalid)
        ));
        assert!(matches!(
            set("providers: [1]\n", &["providers", "x"], Some(&value("1"))),
            Err(ConfigError::Unsupported(_))
        ));
    }

    #[test]
    fn quoted_keys_and_crlf_are_kept() {
        let original = "\"providers\":\r\n  'other': 1\r\n";
        let next = set(original, &["providers", "transitlink"], Some(&value("2"))).unwrap();
        assert_eq!(
            next,
            "\"providers\":\r\n  'other': 1\r\n  transitlink: 2\r\n"
        );
    }
}
