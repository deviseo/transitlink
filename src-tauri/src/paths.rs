//! 本地目录。

use std::path::PathBuf;

/// 用户主目录；极少数取不到的情况退回当前目录。
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// TransitLink 的数据目录：站点、缓存、配置备份都放在 `~/.transitlink`。
pub fn app_data_dir() -> PathBuf {
    home_dir().join(".transitlink")
}
