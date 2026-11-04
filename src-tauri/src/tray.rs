//! 系统托盘（macOS 是菜单栏）：列出接入的站点，点一下就把已经用 TransitLink 配置过的工具
//! 都切到这个站点。关掉主窗口后应用留在托盘里，可以从托盘重新打开或退出。

use std::collections::BTreeSet;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::accounts::Accounts;
use crate::commands::CommandError;
use crate::configure::Configurator;
use crate::settings::SettingsStore;
use crate::tools::ToolKind;

const TRAY_ID: &str = "main";
const SITE_PREFIX: &str = "site:";

/// 托盘切换站点的结果，发给前端显示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchReport {
    pub site_id: String,
    pub switched: Vec<ToolKind>,
    pub failures: Vec<SwitchFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchFailure {
    pub kind: ToolKind,
    pub error: CommandError,
}

struct Labels {
    header: &'static str,
    no_sites: &'static str,
    open: &'static str,
    quit: &'static str,
}

fn labels(language: &str) -> Labels {
    if language.starts_with("zh") {
        Labels {
            header: "切换站点",
            no_sites: "还没有接入站点",
            open: "打开 TransitLink",
            quit: "退出",
        }
    } else {
        Labels {
            header: "Switch site",
            no_sites: "No sites yet",
            open: "Open TransitLink",
            quit: "Quit",
        }
    }
}

/// 建托盘图标。macOS 用单色模板图标（跟着菜单栏的深浅色变），其他系统用应用图标。
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("app icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(tooltip(app))
        .menu(&build_menu(app)?)
        // macOS 的习惯是点图标出菜单；Windows 左键打开窗口，右键出菜单。
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if !cfg!(target_os = "macos")
                && let tauri::tray::TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Up,
                    ..
                } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 工具现在用的站点。
fn current_sites(app: &AppHandle) -> BTreeSet<String> {
    app.state::<Configurator>()
        .configured()
        .into_iter()
        .map(|(_, applied)| applied.site_id)
        .collect()
}

fn tooltip(app: &AppHandle) -> String {
    let current = current_sites(app);
    let names: Vec<String> = app
        .state::<Accounts>()
        .sites()
        .unwrap_or_default()
        .into_iter()
        .filter(|site| current.contains(&site.id))
        .map(|site| site.name)
        .collect();
    if names.is_empty() {
        "TransitLink".to_string()
    } else {
        format!("TransitLink · {}", names.join(" / "))
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let labels = labels(&app.state::<SettingsStore>().get().language);
    let sites = app.state::<Accounts>().sites().unwrap_or_default();
    let current = current_sites(app);

    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "header",
        labels.header,
        false,
        None::<&str>,
    )?)?;
    if sites.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "none",
            labels.no_sites,
            false,
            None::<&str>,
        )?)?;
    }
    for site in sites {
        let checked = current.contains(&site.id);
        let label = format!("{} · {}", site.name, site.account);
        menu.append(&CheckMenuItem::with_id(
            app,
            format!("{SITE_PREFIX}{}", site.id),
            label,
            true,
            checked,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        labels.open,
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        labels.quit,
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

/// 站点、配置或语言变了以后重建托盘菜单。
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip(app)));
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "quit" => app.exit(0),
        _ => {
            if let Some(site_id) = id.strip_prefix(SITE_PREFIX) {
                let app = app.clone();
                let site_id = site_id.to_string();
                tauri::async_runtime::spawn(async move { switch_site(app, site_id).await });
            }
        }
    }
}

/// 把配置过的工具都切到这个站点。还没配置过任何工具时，打开窗口让用户先一键配置。
async fn switch_site(app: AppHandle, site_id: String) {
    let configurator = app.state::<Configurator>();
    let accounts = app.state::<Accounts>();
    let kinds: Vec<ToolKind> = configurator
        .configured()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    if kinds.is_empty() {
        refresh(&app);
        show_main(&app);
        let _ = app.emit("tray-setup", &site_id);
        return;
    }

    let mut report = SwitchReport {
        site_id: site_id.clone(),
        switched: Vec::new(),
        failures: Vec::new(),
    };
    for kind in kinds {
        match configurator.switch(&accounts, kind, &site_id).await {
            Ok(_) => report.switched.push(kind),
            Err(error) => report.failures.push(SwitchFailure {
                kind,
                error: error.into(),
            }),
        }
    }
    refresh(&app);
    let _ = app.emit("tray-switched", &report);
    if !report.failures.is_empty() {
        show_main(&app);
    }
}
