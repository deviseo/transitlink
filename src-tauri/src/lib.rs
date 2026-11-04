//! TransitLink 桌面端核心：中转站账号、Key 与本机编程工具配置都在 Rust 侧处理，
//! 凭证和网络请求不经过网页层。

mod commands;
mod paths;
mod tools;

#[cfg(desktop)]
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // 单实例：已经在运行时再次打开（例如点击 transitlink:// 专属链接），把窗口切到前台，
    // 链接参数由 deep-link 插件转交给正在运行的实例。单实例插件必须最先注册。
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }

    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            // Windows / Linux 的开发版和免安装版需要在运行时注册 transitlink:// 协议；
            // macOS 由安装包里的 Info.plist 注册。
            #[cfg(any(windows, target_os = "linux"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if let Err(error) = _app.deep_link().register_all() {
                    eprintln!("[deep-link] register failed: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::detect_tools
        ])
        .run(tauri::generate_context!())
        .expect("error while running TransitLink");
}
