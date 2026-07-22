//! 系统托盘：按 Claude/Codex 分区列出供应商，勾选当前项，点击直接切换。
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::AppState;

pub const TRAY_ID: &str = "z-switch";

/// 依据当前 store 构建托盘菜单。
pub fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let state = app.state::<AppState>();
    let root = state.0.lock().unwrap();
    let mut mb = MenuBuilder::new(app);

    for (key, title) in [("claude", "Claude Code"), ("codex", "Codex"), ("grok", "Grok")] {
        let hdr = MenuItemBuilder::with_id(format!("hdr:{key}"), title)
            .enabled(false)
            .build(app)?;
        mb = mb.item(&hdr);

        if let Some(data) = root.apps.get(key) {
            for id in &data.order {
                if let Some(p) = data.providers.get(id) {
                    let checked = data.current.as_deref() == Some(id.as_str());
                    let item = CheckMenuItemBuilder::with_id(format!("switch:{key}:{id}"), &p.name)
                        .checked(checked)
                        .build(app)?;
                    mb = mb.item(&item);
                }
            }
        }
        mb = mb.separator();
    }

    let show = MenuItemBuilder::with_id("show", "打开主界面").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    mb = mb.item(&show).item(&quit);
    mb.build()
}

/// 重建并应用托盘菜单（切换/增删后调用）。
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    if let Ok(menu) = build_menu(app) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

/// 处理托盘菜单点击。
pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id == "quit" {
        app.exit(0);
    } else if id == "show" {
        show_main(app);
    } else if let Some(rest) = id.strip_prefix("switch:") {
        if let Some((appk, pid)) = rest.split_once(':') {
            let state = app.state::<AppState>();
            let proxy_handle = app.try_state::<crate::proxy::ProxyHandle>();
            let mut root = state.0.lock().unwrap();
            let backup = root
                .settings
                .get("backupBeforeWrite")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let ph = proxy_handle.as_deref();
            match crate::switch_in_place(&mut root, appk, pid, backup, ph) {
                Ok(()) => {
                    let _ = crate::store::save(&root);
                    drop(root);
                    refresh(app);
                    let _ = app.emit("config-changed", ());
                }
                Err(e) => eprintln!("[z-switch] 托盘切换失败: {e}"),
            }
        }
    }
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
