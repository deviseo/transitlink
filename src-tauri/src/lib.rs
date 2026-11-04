//! TransitLink 桌面端核心：中转站账号、Key 与本机编程工具配置都在 Rust 侧处理，
//! 凭证和网络请求不经过网页层。

// 单元测试的二进制不会调用 run()，只从它可达的命令在测试构建里会被误报为未使用。
#![cfg_attr(test, allow(dead_code))]

mod accounts;
mod clock;
mod commands;
mod configure;
mod paths;
mod secrets;
mod settings;
mod sites;
mod store;
mod tools;
#[cfg(desktop)]
mod tray;

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
            tray::show_main(app);
        }));
        // 自动更新：安装包和 latest.json 都在 GitHub Releases，签名用 tauri.conf.json 里的公钥校验。
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    let accounts = accounts::Accounts::new(
        sites::http::Http::new(),
        store::SiteStore::new(paths::app_data_dir().join("sites.json")),
        Box::new(secrets::KeychainStore),
    );

    let configurator = configure::Configurator::new(
        sites::http::Http::new(),
        paths::home_dir(),
        paths::app_data_dir(),
        Box::new(|key| std::env::var(key).ok()),
    );

    let settings = settings::SettingsStore::load(paths::app_data_dir().join("settings.json"));

    builder
        .manage(accounts)
        .manage(configurator)
        .manage(settings)
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            #[cfg(desktop)]
            tray::create(_app.handle())?;
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
        // 关掉主窗口时留在托盘里（设置里可以关掉这个行为）。
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && window
                    .app_handle()
                    .state::<settings::SettingsStore>()
                    .get()
                    .close_to_tray
            {
                api.prevent_close();
                let _ = window.hide();
            }
            #[cfg(not(desktop))]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::detect_tools,
            commands::detect_site,
            commands::login_site,
            commands::submit_login_code,
            commands::cancel_login,
            commands::list_sites,
            commands::remove_site,
            commands::site_account,
            commands::site_keys,
            commands::copy_site_key,
            commands::site_groups,
            commands::create_site_key,
            commands::setup_plan,
            commands::apply_tool,
            commands::test_tool,
            commands::tool_models,
            commands::set_tool_model,
            commands::restore_tool,
            commands::check_update,
            commands::install_update,
            commands::app_settings,
            commands::set_close_to_tray,
            commands::set_language,
        ])
        .build(tauri::generate_context!())
        .expect("error while building TransitLink")
        .run(|_app, _event| {
            // macOS：窗口藏起来以后点程序坞图标，重新显示窗口。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                tray::show_main(_app);
            }
        });
}
