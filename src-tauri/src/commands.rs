//! 前端可以调用的命令。

use serde::Serialize;

use crate::{paths, tools};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    data_dir: String,
    platform: String,
}

/// 应用名称、版本、数据目录和操作系统。
#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    let package = app.package_info();
    AppInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        data_dir: paths::app_data_dir().display().to_string(),
        platform: std::env::consts::OS.to_string(),
    }
}

/// 检测本机的编程工具（第一期：Claude Code、Codex）。要运行 `--version`，放到阻塞线程里执行，不卡界面。
#[tauri::command]
pub async fn detect_tools() -> Vec<tools::ToolStatus> {
    tauri::async_runtime::spawn_blocking(tools::detect_all)
        .await
        .unwrap_or_default()
}
