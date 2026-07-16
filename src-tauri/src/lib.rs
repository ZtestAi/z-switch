//! z-switch 后端入口 + Tauri 命令。
mod claude_desktop;
mod claude_ext;
mod config;
mod connectivity;
mod live;
mod model_fetch;
mod official;
mod original;
mod proxy;
mod proxy_log;
mod store;
mod stream_test;
mod tray;

use std::sync::Mutex;
use store::{Provider, Root};
use tauri::{AppHandle, Manager, State};

/// 内存中持有整个 providers.json，改动后原子落盘。
pub struct AppState(pub Mutex<Root>);

/// 代理起停控制（async 锁，因 start 含 await）。
pub struct ProxyState(pub tokio::sync::Mutex<proxy::ProxyControl>);

fn persist(root: &Root) -> Result<(), String> {
    store::save(root)
}

fn backup_flag(root: &Root) -> bool {
    root.settings
        .get("backupBeforeWrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// 切换核心（供命令与托盘复用）。
/// - 直连模式：backfill 旧项 → 写目标 live → 更新 current。
/// - 代理模式：中转站之间只热切换内存 target；官方账号保持客户端直连，
///   在官方账号与中转站跨类型切换时才重写对应 app 的 live。
/// 切换/恢复后按需同步 VS Code 扩展放行标记（`~/.claude/config.json` 的 primaryApiKey）。
/// best-effort：仅 claude 且用户开了「应用到插件」开关时生效，失败只记日志、不影响切换本身。
fn sync_claude_plugin_after_switch(plugin_on: bool, app: &str, target_is_official: bool) {
    if plugin_on && app == "claude" {
        if let Err(e) = claude_ext::apply_primary_api_key(!target_is_official) {
            eprintln!("[z-switch] 同步 Claude Code 插件放行标记失败: {e}");
        }
    }
}

/// 切换/恢复后按需让 Claude **桌面版**（独立聊天 App）跟随当前 Claude 供应商。
/// 仅 claude 且用户开了「Claude 桌面版」开关、且平台支持（macOS/Windows）时生效。
/// - 官方账号 → 桌面版退回 1p；
/// - 第三方 + 代理在跑 → 网关指向本地 `/claude`（复用 claude 代理目标，纯透传）；
/// - 第三方 + 代理关 → 网关直连供应商自己的地址。
/// best-effort：失败只记日志，绝不阻断切换本身。
fn sync_claude_desktop_after_switch(
    desktop_on: bool,
    app: &str,
    target_is_official: bool,
    provider: Option<&Provider>,
    proxy_handle: Option<&proxy::ProxyHandle>,
) {
    if !desktop_on || app != "claude" || !claude_desktop::is_supported() {
        return;
    }
    let result = if target_is_official {
        claude_desktop::restore_official()
    } else if proxy_handle.map(|h| h.is_running()).unwrap_or(false) {
        let handle = proxy_handle.expect("running proxy must have a handle");
        claude_desktop::apply_proxy(&proxy::local_base(handle.current_port(), "claude"))
    } else if let Some(p) = provider {
        claude_desktop::apply_direct(p)
    } else {
        return;
    };
    if let Err(e) = result {
        eprintln!("[z-switch] 同步 Claude 桌面版失败: {e}");
    }
}

pub(crate) fn switch_in_place(
    root: &mut Root,
    app: &str,
    id: &str,
    backup: bool,
    proxy_handle: Option<&proxy::ProxyHandle>,
) -> Result<(), String> {
    // 借用 data 前先读设置（避免与 apps 的可变借用冲突）。
    let plugin_on = root
        .settings
        .get("applyClaudePlugin")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let desktop_on = root
        .settings
        .get("applyClaudeDesktop")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let data = root
        .apps
        .get_mut(app)
        .ok_or_else(|| format!("未知应用: {app}"))?;
    if !data.providers.contains_key(id) {
        return Err(format!("供应商不存在: {id}"));
    }

    let proxy_on = proxy_handle.map(|h| h.is_running()).unwrap_or(false);
    let target = data.providers.get(id).cloned().unwrap();
    let target_is_official = store::is_official_provider(&target);
    let current_id = data.current.clone();
    let current_is_official = current_id
        .as_ref()
        .and_then(|current| data.providers.get(current))
        .is_some_and(store::is_official_provider);

    if proxy_on {
        let handle = proxy_handle.expect("running proxy must have a handle");
        if target_is_official {
            // 官方账号始终保持客户端直连；另一个应用仍可继续使用本地代理。
            live::write_live(app, &target, backup)?;
            proxy::clear_target(&handle.targets, app);
        } else {
            let runtime_target = proxy::target_from_provider(app, &target)
                .ok_or_else(|| format!("供应商 {} 缺少可转发的 Base URL", target.name))?;

            // 从官方直连、或“无当前供应商”（如刚恢复过 Codex，current=None、
            // live 已被写回官方 config.toml）进入代理时，live 此刻并不是 localhost
            // 配置，必须先保存登录态、再把 live 改成 localhost，否则客户端仍直连官方，
            // 代理的内存 target 根本用不上（#3：恢复后切第三方不生效）。
            if current_is_official || current_id.is_none() {
                if let Some(current) = current_id.as_ref() {
                    if current != id {
                        if let Some(old) = data.providers.get_mut(current) {
                            live::backfill(app, old);
                        }
                    }
                }
                proxy::set_target(&handle.targets, app, runtime_target);
                let proxied = proxy::proxied_provider(app, &target, handle.current_port());
                if let Err(error) = live::write_live(app, &proxied, backup) {
                    proxy::clear_target(&handle.targets, app);
                    return Err(error);
                }
            } else {
                // 中转站之间仍然只需更新内存上游，保持无感热切换。
                proxy::set_target(&handle.targets, app, runtime_target);
            }
        }
        data.current = Some(id.to_string());
        sync_claude_plugin_after_switch(plugin_on, app, target_is_official);
        sync_claude_desktop_after_switch(
            desktop_on,
            app,
            target_is_official,
            Some(&target),
            proxy_handle,
        );
        return Ok(());
    }

    // 直连模式：原有逻辑。
    if let Some(cur) = data.current.clone() {
        if cur != id {
            if let Some(old) = data.providers.get_mut(&cur) {
                live::backfill(app, old);
            }
        }
    }
    live::write_live(app, &target, backup)?;
    data.current = Some(id.to_string());
    sync_claude_plugin_after_switch(plugin_on, app, target_is_official);
    sync_claude_desktop_after_switch(desktop_on, app, target_is_official, Some(&target), proxy_handle);
    Ok(())
}

/// 读取完整配置
#[tauri::command]
fn get_config(state: State<AppState>) -> Root {
    state.0.lock().unwrap().clone()
}

/// 新增或更新一个供应商（存在即覆盖）。编辑当前激活项时同步写 live。
#[tauri::command]
fn save_provider(
    app_handle: AppHandle,
    state: State<AppState>,
    app: String,
    provider: Provider,
) -> Result<Root, String> {
    let mut root = state.0.lock().unwrap();
    let backup = backup_flag(&root);
    let data = root.apps.entry(app.clone()).or_default();
    let id = provider.id.clone();
    if id.trim().is_empty() {
        return Err("供应商 id 不能为空".into());
    }
    if store::official_provider_id(&app) == Some(id.as_str()) {
        return Err("官方账号是系统卡片，不能编辑".into());
    }
    if !data.order.contains(&id) {
        data.order.push(id.clone());
    }
    let is_current = data.current.as_deref() == Some(id.as_str());
    data.providers.insert(id, provider.clone());
    if is_current {
        let handle = app_handle.state::<proxy::ProxyHandle>();
        if handle.is_running() {
            // 代理模式：更新内存 target（live 保持 localhost，不覆盖）
            if let Some(t) = proxy::target_from_provider(&app, &provider) {
                proxy::set_target(&handle.targets, &app, t);
            }
        } else {
            live::write_live(&app, &provider, backup)?;
        }
    }
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

/// 删除供应商。删除当前项时必须指定处理方式：
/// - keep：保留当前电脑配置，仅解除 z-switch 管理；
/// - restore：先恢复首次保存的本机原始配置。
#[tauri::command]
fn delete_provider(
    app_handle: AppHandle,
    state: State<AppState>,
    app: String,
    id: String,
    active_mode: Option<String>,
) -> Result<Root, String> {
    if store::official_provider_id(&app) == Some(id.as_str()) {
        return Err("官方账号是系统卡片，不能删除".into());
    }
    let handle = app_handle.state::<proxy::ProxyHandle>();
    let mut root = state.0.lock().unwrap();
    let data = root
        .apps
        .get(&app)
        .ok_or_else(|| format!("未知应用: {app}"))?;
    let is_current = data.current.as_deref() == Some(id.as_str());
    let current_provider = data.providers.get(&id).cloned();

    if is_current {
        match active_mode.as_deref() {
            Some("keep") => {
                // 代理模式的 live 文件指向 localhost；解除管理前必须写回真实地址。
                if handle.is_running() {
                    let provider = current_provider
                        .as_ref()
                        .ok_or_else(|| "当前供应商不存在".to_string())?;
                    live::write_live(&app, provider, backup_flag(&root))?;
                }
            }
            Some("restore") => original::restore_app(&app)?,
            _ => return Err("删除正在使用的供应商前，请选择保留当前配置或恢复原始配置".into()),
        }
        proxy::clear_target(&handle.targets, &app);
    }

    let data = root
        .apps
        .get_mut(&app)
        .ok_or_else(|| format!("未知应用: {app}"))?;
    if is_current {
        data.current = None;
    }
    data.providers.remove(&id);
    data.order.retain(|x| x != &id);
    persist(&root)?;
    // 删除当前项并「恢复原始配置」= 回到官方直连，清掉插件放行标记，桌面版退回 1p。
    if is_current && active_mode.as_deref() == Some("restore") {
        let plugin_on = root
            .settings
            .get("applyClaudePlugin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let desktop_on = root
            .settings
            .get("applyClaudeDesktop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        sync_claude_plugin_after_switch(plugin_on, &app, true);
        sync_claude_desktop_after_switch(desktop_on, &app, true, None, Some(handle.inner()));
    }
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

/// 首次保存的本机原始配置状态。
#[tauri::command]
fn original_config_status() -> original::OriginalConfigStatus {
    original::status()
}

/// 用系统文件管理器打开一个目录（跨平台）。
fn reveal_dir_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("打开目录 {} 失败：{error}", path.display()))
}

/// 打开指定配置文件所在目录，方便用户直接查看/编辑：
/// claude → ~/.claude（settings.json、config.json）；codex → ~/.codex（auth.json、config.toml）；
/// app → ~/.z-switch（providers.json、backups）。目录不存在时退回打开用户主目录，避免空按钮，
/// 且不擅自创建 Claude/Codex 目录。
#[tauri::command]
fn open_config_dir(kind: String) -> Result<(), String> {
    let path = match kind.as_str() {
        "claude" => config::get_home_dir().join(".claude"),
        "codex" => config::get_home_dir().join(".codex"),
        "app" => config::get_app_config_dir(),
        other => return Err(format!("未知配置目录：{other}")),
    };
    if kind == "app" {
        let _ = std::fs::create_dir_all(&path);
    }
    // 不做 canonicalize：Windows 上它会返回 \\?\ 扩展长度路径，explorer.exe 往往不认。
    // get_home_dir().join(...) 已是普通绝对路径，直接交给文件管理器即可。
    let target = if path.exists() {
        path
    } else {
        config::get_home_dir()
    };
    reveal_dir_in_file_manager(&target)
}

/// 创建并使用系统文件管理器打开写前备份目录。
#[tauri::command]
fn open_backups_folder() -> Result<(), String> {
    let path = config::get_app_config_dir().join("backups");
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("创建备份目录 {} 失败：{error}", path.display()))?;
    let path = std::fs::canonicalize(&path).unwrap_or(path);

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(&path).spawn();

    result
        .map(|_| ())
        .map_err(|error| format!("打开备份目录 {} 失败：{error}", path.display()))
}

/// 使用系统默认浏览器打开版本可追踪的使用帮助。
#[tauri::command]
fn open_help_document() -> Result<(), String> {
    const HELP_URL: &str = "https://github.com/ZtestAi/z-switch/blob/master/docs/USAGE.md";

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer.exe")
        .arg(HELP_URL)
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(HELP_URL).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(HELP_URL).spawn();

    result
        .map(|_| ())
        .map_err(|error| format!("打开使用帮助失败：{error}"))
}

#[tauri::command]
fn open_proxy_log_folder() -> Result<(), String> {
    proxy_log::open_folder()
}

#[tauri::command]
fn clear_proxy_error_log() -> Result<(), String> {
    proxy_log::clear()
}

/// 恢复指定应用的首次原始配置，并解除当前供应商关联。
#[tauri::command]
fn restore_original(
    app_handle: AppHandle,
    state: State<AppState>,
    app: String,
) -> Result<Root, String> {
    original::restore_app(&app)?;
    let handle = app_handle.state::<proxy::ProxyHandle>();
    proxy::clear_target(&handle.targets, &app);

    let mut root = state.0.lock().unwrap();
    let data = root
        .apps
        .get_mut(&app)
        .ok_or_else(|| format!("未知应用: {app}"))?;
    // “原始配置”可能本来就是用户已有的中转配置，它属于灾难恢复，
    // 不能冒充官方账号的生效状态。
    data.current = None;
    // 恢复原始配置 = 回官方直连，清掉插件放行标记，桌面版退回 1p。
    let plugin_on = root
        .settings
        .get("applyClaudePlugin")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let desktop_on = root
        .settings
        .get("applyClaudeDesktop")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    sync_claude_plugin_after_switch(plugin_on, &app, true);
    sync_claude_desktop_after_switch(desktop_on, &app, true, None, Some(handle.inner()));
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

/// 切换当前激活供应商。直连模式写 live；代理模式仅热切换内存 target。
#[tauri::command]
fn switch_provider(
    app_handle: AppHandle,
    state: State<AppState>,
    app: String,
    id: String,
) -> Result<Root, String> {
    let handle = app_handle.state::<proxy::ProxyHandle>();
    let mut root = state.0.lock().unwrap();
    let backup = backup_flag(&root);
    switch_in_place(&mut root, &app, &id, backup, Some(&handle))?;
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

/// 读取代理端口（settings.reliability.proxyPort，缺省 DEFAULT_PORT）。
fn proxy_port(root: &Root) -> u16 {
    root.settings
        .get("reliability")
        .and_then(|r| r.get("proxyPort"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u16)
        .filter(|&p| p != 0)
        .unwrap_or(proxy::DEFAULT_PORT)
}

/// 设 settings.reliability.proxyEnabled。
fn set_proxy_enabled_flag(root: &mut Root, enabled: bool) {
    if !root.settings.is_object() {
        root.settings = serde_json::json!({});
    }
    let obj = root.settings.as_object_mut().unwrap();
    let rel = obj
        .entry("reliability")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(r) = rel.as_object_mut() {
        r.insert("proxyEnabled".into(), serde_json::Value::Bool(enabled));
    }
}

/// 代理状态（前端查询用）。in_flight/total/last_activity_ms 为本地活跃度计数，
/// 仅事件次数、不碰请求内容、不出本机。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyStatus {
    enabled: bool,
    port: u16,
    in_flight: u32,
    total: u64,
    last_activity_ms: u64,
}

/// 查询代理是否在跑 + 端口 + 本地活跃度计数。
#[tauri::command]
fn proxy_status(app_handle: AppHandle) -> ProxyStatus {
    let handle = app_handle.state::<proxy::ProxyHandle>();
    ProxyStatus {
        enabled: handle.is_running(),
        port: handle.current_port(),
        in_flight: handle.in_flight(),
        total: handle.total(),
        last_activity_ms: handle.last_activity_ms(),
    }
}

/// 开启/关闭本地热切换代理。
/// 开：起服务 + 两个 app 的当前 provider → 设内存 target + 把 live 的 base_url 改写为 localhost。
/// 关：停服务 + 把两个 app 的当前 provider 真实配置写回 live（恢复直连）。
#[tauri::command]
async fn set_proxy_enabled(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    proxy_state: State<'_, ProxyState>,
    enabled: bool,
) -> Result<Root, String> {
    let handle = app_handle.state::<proxy::ProxyHandle>();
    // 先取出需要的数据，避免把 std MutexGuard 跨 await 持有。
    let (backup, port, runtime_config, currents, desktop_on) = {
        let root = state.0.lock().unwrap();
        let mut cur: Vec<(String, Provider)> = Vec::new();
        for app in ["claude", "codex"] {
            if let Some(data) = root.apps.get(app) {
                if let Some(id) = &data.current {
                    if let Some(p) = data.providers.get(id) {
                        cur.push((app.to_string(), p.clone()));
                    }
                }
            }
        }
        (
            backup_flag(&root),
            proxy_port(&root),
            proxy::ProxyRuntimeConfig::from_settings(&root.settings),
            cur,
            root.settings
                .get("applyClaudeDesktop")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        )
    };

    if enabled {
        {
            let mut ctl = proxy_state.0.lock().await;
            ctl.start(port, runtime_config).await?;
        }
        // 设 target + 改写 live 为 localhost
        for (app, provider) in &currents {
            if store::is_official_provider(provider) {
                proxy::clear_target(&handle.targets, app);
                continue;
            }
            if let Some(t) = proxy::target_from_provider(app, provider) {
                proxy::set_target(&handle.targets, app, t);
            }
            let proxied = proxy::proxied_provider(app, provider, port);
            live::write_live(app, &proxied, backup)?;
        }
    } else {
        {
            let mut ctl = proxy_state.0.lock().await;
            ctl.stop();
        }
        // 恢复真实配置写回 live
        for (app, provider) in &currents {
            if store::is_official_provider(provider) {
                proxy::clear_target(&handle.targets, app);
                continue;
            }
            live::write_live(app, provider, backup)?;
        }
    }

    // 代理起停翻转了桌面版有效端点（localhost ↔ 直连），按新状态重写桌面版 profile。
    if desktop_on {
        if let Some((app, provider)) = currents.iter().find(|(app, _)| app == "claude") {
            let is_official = store::is_official_provider(provider);
            sync_claude_desktop_after_switch(
                true,
                app,
                is_official,
                Some(provider),
                Some(handle.inner()),
            );
        }
    }

    let mut root = state.0.lock().unwrap();
    set_proxy_enabled_flag(&mut root, enabled);
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}
#[tauri::command]
fn reorder_providers(
    app_handle: AppHandle,
    state: State<AppState>,
    app: String,
    order: Vec<String>,
) -> Result<Root, String> {
    let mut root = state.0.lock().unwrap();
    let data = root
        .apps
        .get_mut(&app)
        .ok_or_else(|| format!("未知应用: {app}"))?;
    data.order = order;
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

/// 导入整份配置（覆盖）
#[tauri::command]
fn import_config(
    app_handle: AppHandle,
    state: State<AppState>,
    mut root_in: Root,
) -> Result<Root, String> {
    root_in.ensure_official_providers();
    let mut root = state.0.lock().unwrap();
    *root = root_in;
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

/// 保存全局设置
#[tauri::command]
fn save_settings(state: State<AppState>, settings: serde_json::Value) -> Result<Root, String> {
    let mut root = state.0.lock().unwrap();
    root.settings = settings;
    persist(&root)?;
    Ok(root.clone())
}

/// 「应用到 Claude Code 插件」开关的**文件副作用**：立即按当前 Claude 供应商同步放行标记。
/// 开→当前是第三方则写 primaryApiKey=any、官方则删除；关→一律删除。仅动 ~/.claude/config.json。
/// 设置本身（applyClaudePlugin）的持久化由前端走 save_settings，与其它开关一致。
#[tauri::command]
fn set_claude_plugin_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    // 只在锁内读一下当前 claude 供应商是否官方，随即释放锁再写文件。
    let managed = enabled && {
        let root = state.0.lock().unwrap();
        let data = root.apps.get("claude");
        data.and_then(|d| d.current.clone())
            .and_then(|id| data.and_then(|d| d.providers.get(&id).cloned()))
            .map(|p| !store::is_official_provider(&p))
            .unwrap_or(false)
    };
    claude_ext::apply_primary_api_key(managed)
}

/// 「跳过 Claude Code 初次安装确认」开关的**文件副作用**：写/删 ~/.claude.json 的
/// hasCompletedOnboarding。设置本身的持久化同样由前端 save_settings 负责。
#[tauri::command]
fn set_claude_onboarding_skip(enabled: bool) -> Result<(), String> {
    claude_ext::apply_onboarding_completed(enabled)
}

/// 「Claude 桌面版随切换」开关的**文件副作用**：按当前 Claude 供应商 + 代理状态
/// 立即写/撤桌面版 3p 网关 profile。开→官方则退 1p、第三方按代理/直连写；关→一律退 1p。
/// 不支持的平台（非 macOS/Windows）直接成功返回。设置本身持久化由前端 save_settings 负责。
#[tauri::command]
fn set_claude_desktop_enabled(
    app_handle: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<(), String> {
    if !claude_desktop::is_supported() {
        return Ok(());
    }
    if !enabled {
        return claude_desktop::restore_official();
    }
    // 开启：读当前 claude 供应商（锁内取值随即释放，再写文件）。
    let provider = {
        let root = state.0.lock().unwrap();
        let data = root.apps.get("claude");
        data.and_then(|d| d.current.clone())
            .and_then(|id| data.and_then(|d| d.providers.get(&id).cloned()))
    };
    let Some(provider) = provider else {
        // 无当前供应商 = 视作官方直连。
        return claude_desktop::restore_official();
    };
    if store::is_official_provider(&provider) {
        return claude_desktop::restore_official();
    }
    let handle = app_handle.state::<proxy::ProxyHandle>();
    if handle.is_running() {
        claude_desktop::apply_proxy(&proxy::local_base(handle.current_port(), "claude"))
    } else {
        claude_desktop::apply_direct(&provider)
    }
}

/// 导出为格式化 JSON 字符串
#[tauri::command]
fn export_json(state: State<AppState>) -> Result<String, String> {
    let root = state.0.lock().unwrap();
    serde_json::to_string_pretty(&*root).map_err(|e| e.to_string())
}

/// 端点测速（HTTP 层往返毫秒，保留亚毫秒精度）。
/// 走 HTTP 而非纯 TCP，避免本机 TUN/透明代理就地应答导致的 <1ms 失真。
#[tauri::command]
async fn speedtest(url: String) -> Result<f64, String> {
    connectivity::latency(&url).await
}

/// 拉取供应商可用模型列表
#[tauri::command]
async fn fetch_models(
    base_url: String,
    api_key: String,
    models_url: Option<String>,
) -> Result<Vec<String>, String> {
    model_fetch::fetch_models(&base_url, &api_key, models_url.as_deref()).await
}

/// 连通性测试：探测地址通不通 / key 对不对
#[tauri::command]
async fn test_connectivity(
    base_url: String,
    api_key: String,
) -> Result<connectivity::ConnResult, String> {
    connectivity::test(&base_url, &api_key).await
}

/// 使用编辑页当前配置发送一条真实的最小流式模型请求。
#[tauri::command]
async fn test_stream(
    app: String,
    base_url: String,
    api_key: String,
    model: String,
    wire_api: String,
    api_key_field: Option<String>,
    on_event: tauri::ipc::Channel<stream_test::StreamTestEvent>,
) -> Result<stream_test::StreamTestResult, String> {
    stream_test::run(
        &app,
        &base_url,
        &api_key,
        &model,
        &wire_api,
        api_key_field.as_deref(),
        on_event,
    )
    .await
}

/// 设置开机自启（同步到系统 + 持久化 settings.autoLaunch）
#[tauri::command]
fn set_auto_launch(
    app_handle: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<Root, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let mgr = app_handle.autolaunch();
        let r = if enabled { mgr.enable() } else { mgr.disable() };
        r.map_err(|e| format!("设置开机自启失败: {e}"))?;
    }
    let mut root = state.0.lock().unwrap();
    if let Some(obj) = root.settings.as_object_mut() {
        obj.insert("autoLaunch".into(), serde_json::Value::Bool(enabled));
    }
    persist(&root)?;
    Ok(root.clone())
}

/// 从现有 live 配置（~/.claude、~/.codex）反向导入为供应商。
/// 返回导入的应用列表（如 ["claude","codex"]），无可导入内容则为空。
#[tauri::command]
fn import_live(app_handle: AppHandle, state: State<AppState>) -> Result<Root, String> {
    let mut root = state.0.lock().unwrap();
    if !import_live_in_place(&mut root) {
        return Err("未在 ~/.claude 或 ~/.codex 找到可导入的现有配置".into());
    }
    persist(&root)?;
    let out = root.clone();
    drop(root);
    tray::refresh(&app_handle);
    Ok(out)
}

fn import_live_in_place(root: &mut Root) -> bool {
    let mut touched = false;
    // claude
    if let Some(mut p) = live::import_claude() {
        let data = root.apps.entry("claude".into()).or_default();
        let id = unique_id(&data.providers, "imported-current");
        p.id = id.clone();
        if !data.order.contains(&id) {
            data.order.push(id.clone());
        }
        data.providers.insert(id.clone(), p);
        data.current = Some(id);
        touched = true;
    }
    // codex
    if let Some(mut p) = live::import_codex() {
        let data = root.apps.entry("codex".into()).or_default();
        let id = unique_id(&data.providers, "imported-current");
        p.id = id.clone();
        if !data.order.contains(&id) {
            data.order.push(id.clone());
        }
        data.providers.insert(id.clone(), p);
        data.current = Some(id);
        touched = true;
    }
    touched
}

/// 生成不与现有键冲突的 id
fn unique_id(providers: &std::collections::HashMap<String, Provider>, base: &str) -> String {
    if !providers.contains_key(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}-{n}");
        if !providers.contains_key(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Windows 11 起，为无边框窗口（`decorations: false`）显式声明圆角。
/// 无边框窗口 DWM 默认不自动圆角，需设 `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND`。
/// Win10 无此能力，系统忽略该属性、保持直角，无副作用；macOS/Linux 不涉及。
#[cfg(windows)]
fn apply_rounded_corners(window: &tauri::WebviewWindow) {
    use std::ffi::c_void;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const c_void, size: u32) -> i32;
    }
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let pref: u32 = DWMWCP_ROUND;
    // best-effort：失败（如 Win10 不支持）时静默保持直角。
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd.0 as isize,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut root = store::load();
    let mut root_changed = root.ensure_official_providers();

    if let Err(error) = official::capture_codex_if_logged_in() {
        eprintln!("[z-switch] 初始化 Codex 官方登录态失败：{error}");
    }

    // 新版本首次建立原始快照时，如果上次保持代理开启，先把 provider 的真实配置
    // 写回 live，避免把 localhost 占位配置保存成恢复基线。
    if !original::status().captured {
        let proxy_was_enabled = root
            .settings
            .get("reliability")
            .and_then(|r| r.get("proxyEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if proxy_was_enabled {
            for app in ["claude", "codex"] {
                let provider = root
                    .apps
                    .get(app)
                    .and_then(|data| data.current.as_ref().and_then(|id| data.providers.get(id)))
                    .cloned();
                if let Some(provider) = provider {
                    if let Err(error) = live::write_live(app, &provider, false) {
                        eprintln!("[z-switch] 建立原始快照前恢复 {app} 真实配置失败：{error}");
                    }
                }
            }
        }
    }

    let snapshot_ready = match original::capture_once() {
        Ok(_) => true,
        Err(error) => {
            eprintln!("[z-switch] 保存本机原始配置失败：{error}");
            false
        }
    };

    let initial_import_done = root
        .settings
        .get("initialImportDone")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if snapshot_ready && !initial_import_done {
        if !root.has_non_official_provider() {
            import_live_in_place(&mut root);
        }
        if let Some(settings) = root.settings.as_object_mut() {
            settings.insert("initialImportDone".into(), serde_json::Value::Bool(true));
        }
        root_changed = true;
    }

    // 官方卡片只保存去除中转字段后的公共配置；OAuth/API Key 不会进入卡片。
    for app in ["claude", "codex"] {
        if let Some(id) = store::official_provider_id(app) {
            if let Some(provider) = root
                .apps
                .get_mut(app)
                .and_then(|data| data.providers.get_mut(id))
            {
                root_changed |= live::hydrate_official_provider(app, provider);
            }
        }
    }

    // 首次初始化或旧种子迁移后，如果该应用当前选择的是官方账号，立即让
    // live 与界面状态一致；原始快照已经先完成，因此不会丢失用户原配置。
    if snapshot_ready && root_changed {
        for app in ["claude", "codex"] {
            let current_official = root
                .apps
                .get(app)
                .and_then(|data| data.current.as_ref().and_then(|id| data.providers.get(id)))
                .filter(|provider| store::is_official_provider(provider))
                .cloned();
            if let Some(provider) = current_official {
                if let Err(error) = live::write_live(app, &provider, false) {
                    eprintln!("[z-switch] 初始化 {app} 官方账号配置失败：{error}");
                }
            }
        }
    }
    if root_changed {
        if let Err(error) = persist(&root) {
            eprintln!("[z-switch] 保存官方账号/首次导入结果失败：{error}");
        }
    }
    let proxy_handle = proxy::ProxyHandle::default();
    let proxy_control = proxy::ProxyControl::new(proxy_handle.clone());
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }));
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ));
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
        builder = builder.plugin(tauri_plugin_process::init());
    }

    builder
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_deep_link::init())
        .manage(AppState(Mutex::new(root)))
        .manage(proxy_handle)
        .manage(ProxyState(tokio::sync::Mutex::new(proxy_control)))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_provider,
            delete_provider,
            switch_provider,
            reorder_providers,
            import_config,
            export_json,
            save_settings,
            speedtest,
            fetch_models,
            import_live,
            original_config_status,
            open_backups_folder,
            open_config_dir,
            open_help_document,
            open_proxy_log_folder,
            clear_proxy_error_log,
            restore_original,
            test_connectivity,
            test_stream,
            set_auto_launch,
            proxy_status,
            set_proxy_enabled,
            set_claude_plugin_enabled,
            set_claude_onboarding_skip,
            set_claude_desktop_enabled
        ])
        .setup(|app| {
            // 运行时注册 zswitch:// 协议（便于未装安装包时也能测试深链）
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register_all();
            }
            // Windows 11 无边框窗口圆角（Win10 静默忽略）。
            #[cfg(windows)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    apply_rounded_corners(&window);
                }
            }
            let menu = tray::build_menu(app.handle())?;
            tauri::tray::TrayIconBuilder::with_id(tray::TRAY_ID)
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("z-switch")
                .menu(&menu)
                .on_menu_event(|app, event| tray::handle_menu_event(app, event.id.as_ref()))
                .build(app)?;

            // 上次退出时代理是开的 → live 文件仍指向 localhost，必须自动拉起代理，
            // 否则 CLI 请求会打到死端口。读 flag + 当前 providers，起服务并设 target
            // （live 已是 localhost，无需重写）。
            let handle = app.handle().clone();
            let (enabled, port, runtime_config, currents) = {
                let st = handle.state::<AppState>();
                let root = st.0.lock().unwrap();
                let on = root
                    .settings
                    .get("reliability")
                    .and_then(|r| r.get("proxyEnabled"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut cur: Vec<(String, Provider)> = Vec::new();
                for a in ["claude", "codex"] {
                    if let Some(d) = root.apps.get(a) {
                        if let Some(id) = &d.current {
                            if let Some(p) = d.providers.get(id) {
                                cur.push((a.to_string(), p.clone()));
                            }
                        }
                    }
                }
                (
                    on,
                    proxy_port(&root),
                    proxy::ProxyRuntimeConfig::from_settings(&root.settings),
                    cur,
                )
            };
            if enabled {
                let ph = handle.state::<proxy::ProxyHandle>().inner().clone();
                let ps = handle.state::<ProxyState>();
                let ctl_mutex = &ps.0;
                tauri::async_runtime::block_on(async {
                    let mut ctl = ctl_mutex.lock().await;
                    if let Err(e) = ctl.start(port, runtime_config).await {
                        eprintln!("[z-switch] 自动拉起代理失败：{e}");
                        return;
                    }
                    for (app, provider) in &currents {
                        if store::is_official_provider(provider) {
                            proxy::clear_target(&ph.targets, app);
                            continue;
                        }
                        if let Some(t) = proxy::target_from_provider(app, provider) {
                            proxy::set_target(&ph.targets, app, t);
                        }
                        let proxied = proxy::proxied_provider(app, provider, port);
                        if let Err(error) = live::write_live(app, &proxied, false) {
                            eprintln!("[z-switch] 自动恢复代理地址失败：{error}");
                        }
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running z-switch");
}
