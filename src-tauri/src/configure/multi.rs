//! 几个文件一起改的工具（Pi、Oh My Pi 等）：一次读出这些文件，算好所有新内容（有一个不合法就都不写），
//! 只写有变化的；写到一半出错时把已经写的改回去。各个工具只管怎么从现在的内容算出新内容。

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ConfigError, Connection, files, probe};
use crate::tools::ToolKind;

/// 一键配置要写进去的连接。
pub struct Target<'a> {
    pub site_name: &'a str,
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: &'a str,
    /// 写进工具模型列表的模型。
    pub models: &'a [String],
}

/// 一个配置文件：位置、备份用的名字（几个工具的文件可能同名）和现在的内容（不存在时为空）。
pub struct Doc {
    pub path: PathBuf,
    pub backup: String,
    pub text: Option<String>,
}

impl Doc {
    /// 这个文件格式有误。
    pub fn invalid(&self) -> ConfigError {
        ConfigError::InvalidFile(self.path.display().to_string())
    }

    /// 没法安全修改：错误里写上是哪个文件。
    pub fn unsupported(&self) -> ConfigError {
        ConfigError::Unsupported(self.path.display().to_string())
    }

    /// 把解析阶段的 Invalid、Unsupported 换成带文件名的。
    pub fn at(&self, error: ConfigError) -> ConfigError {
        match error {
            ConfigError::Invalid => self.invalid(),
            ConfigError::Unsupported(what) if what.is_empty() => self.unsupported(),
            other => other,
        }
    }

    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or_default()
    }
}

/// 一个多文件工具的写法。`docs` 和 `files` 的顺序一致；返回的新内容里 None 表示删掉这个文件。
pub trait MultiFile: Sync {
    /// 要读写的文件和备份名。第一个是给界面显示的「配置文件」。
    fn files(&self, home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<(PathBuf, String)>;

    /// 工具自己正在保存配置（锁着）：稍后再试。
    fn busy(&self, _docs: &[Doc]) -> bool {
        false
    }

    /// 写入站点连接。`previous` 是之前配置时留下的快照：已经配置过就沿用它，还原总是回到第一次之前。
    fn apply(
        &self,
        docs: &[Doc],
        target: &Target,
        previous: Option<&Value>,
    ) -> Result<(Vec<Option<String>>, Value), ConfigError>;

    /// 换模型。
    fn set_model(&self, docs: &[Doc], model: &str) -> Result<Vec<Option<String>>, ConfigError>;

    /// 还原到第一次配置之前（只放回我们改过的几项）。
    fn restore(&self, docs: &[Doc], snapshot: &Value) -> Result<Vec<Option<String>>, ConfigError>;

    /// 读出现在的连接。
    fn read(&self, docs: &[Doc]) -> Result<Connection, ConfigError>;

    /// 写进工具模型列表的模型（默认：和选的模型同一类，见 [`probe::opencode_family`]）。
    fn models(&self, _kind: ToolKind, chosen: &str, ids: &[String]) -> Vec<String> {
        probe::opencode_family(chosen, ids)
    }
}

/// 读出这些文件。
pub fn read(paths: Vec<(PathBuf, String)>) -> Result<Vec<Doc>, ConfigError> {
    paths
        .into_iter()
        .map(|(path, backup)| {
            let text = files::read(&path)?;
            Ok(Doc { path, backup, text })
        })
        .collect()
}

/// 修改前备份这些文件（`keep_original`：第一次配置，另存一份原文件）。
pub fn backup(docs: &[Doc], dir: &Path, now: u64, keep_original: bool) -> Result<(), ConfigError> {
    for doc in docs {
        files::backup_named(&doc.path, &doc.backup, dir, now, keep_original)?;
    }
    Ok(())
}

/// 只写有变化的文件；写到一半出错时把已经写的改回去。
pub fn write(docs: &[Doc], next: &[Option<String>]) -> Result<(), ConfigError> {
    if next.len() != docs.len() {
        return Err(ConfigError::Io(
            "write plan does not match the files".into(),
        ));
    }
    let mut written: Vec<usize> = Vec::new();
    for (index, doc) in docs.iter().enumerate() {
        if doc.text == next[index] {
            continue;
        }
        let result = match &next[index] {
            Some(text) => files::write(&doc.path, text),
            None => files::remove(&doc.path),
        };
        if let Err(error) = result {
            for done in written.into_iter().rev() {
                let _ = match &docs[done].text {
                    Some(text) => files::write(&docs[done].path, text),
                    None => files::remove(&docs[done].path),
                };
            }
            return Err(error.into());
        }
        written.push(index);
    }
    Ok(())
}

/// 快照转回具体的类型（旧记录缺字段时用默认值）。
pub fn snapshot<T: serde::de::DeserializeOwned + Default>(value: Option<&Value>) -> Option<T> {
    value.map(|value| serde_json::from_value(value.clone()).unwrap_or_default())
}

/// 快照转成 JSON 存起来。
pub fn to_value<T: serde::Serialize>(snapshot: &T) -> Result<Value, ConfigError> {
    serde_json::to_value(snapshot).map_err(|error| ConfigError::Io(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_only_changes_and_rolls_back() {
        let dir = std::env::temp_dir().join(format!("transitlink-multi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("a.json");
        std::fs::write(&first, "old").unwrap();
        // 第二个文件的位置是一个目录：写不进去，第一个要改回去。
        let blocked = dir.join("blocked");
        std::fs::create_dir_all(blocked.join("x")).unwrap();
        let docs = vec![
            Doc {
                path: first.clone(),
                backup: "a.json".into(),
                text: Some("old".into()),
            },
            Doc {
                path: blocked.clone(),
                backup: "b".into(),
                text: None,
            },
        ];
        assert!(write(&docs, &[Some("new".into()), Some("x".into())]).is_err());
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "old");
        // 没变的文件不写。
        write(&docs[..1], &[Some("old".into())]).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
