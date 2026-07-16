//! Claude 桌面版（独立聊天 App）3p 网关配置写盘。
//!
//! 让 Claude **桌面 App**（非 CLI、非 VS Code 扩展）随 z-switch 当前 Claude 供应商切换而生效。
//! 桌面版读 `%LOCALAPPDATA%\Claude` / `Claude-3p`（mac 为 `~/Library/Application Support/...`），
//! 只改 `settings.json` 的 env 它不认。这里照 cc-switch 的成熟做法写它的 **3p gateway profile**：
//!
//! - **代理开着**：网关指向本地 `http://127.0.0.1:<port>/claude`（复用现有 claude 代理目标，
//!   proxy 剥离客户端鉴权头并注入真实 key，纯透传）——随热切换自动跟随。
//! - **代理关着**：网关直连该供应商自己的 `ANTHROPIC_BASE_URL` + key（bearer）。
//! - **官方账号 / 恢复原始**：退回 `1p` 官方模式，删除我们写的 profile。
//!
//! 轻量路线：不做格式转换、不改 body。仅 macOS/Windows 生效；其它平台 no-op。
//! 全程 best-effort：任何失败只应记日志，绝不阻断 CLI 切换本身。
use crate::config::{atomic_write, read_json_file, write_json_file};
use crate::store::Provider;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// 我们写入的 profile 的固定 UUID（z-switch 专属，刻意区别于 cc-switch，
/// 两者都装时各自维护自己的 profile，不互相覆盖）。
pub const PROFILE_ID: &str = "00000000-0000-4000-8000-007a53000001";
pub const PROFILE_NAME: &str = "z-switch";

#[cfg(any(target_os = "macos", windows, test))]
const CONFIG_FILE: &str = "claude_desktop_config.json";
#[cfg(any(target_os = "macos", windows, test))]
const CONFIG_LIBRARY_DIR: &str = "configLibrary";

/// proxy 模式下写入 profile 的占位网关 token：会被本地代理剥离并注入真实 key，
/// 故用固定占位值即可（代理不校验它）。
const GATEWAY_TOKEN: &str = "z-switch-desktop";

/// 恢复官方时需从 3p config 的 `enterpriseConfig` 清掉的键（照 cc-switch）。
const ENTERPRISE_KEYS: &[&str] = &[
    "disableDeploymentModeChooser",
    "inferenceGatewayApiKey",
    "inferenceGatewayAuthScheme",
    "inferenceGatewayBaseUrl",
    "inferenceProvider",
];

/// 默认写入桌面版模型菜单的 4 档 claude 路由 + 是否标 `supports1m`。
/// 只给真正支持 100 万上下文的档位标 1M（对齐真机已应用的 ccsw 配置：仅 opus），
/// 避免给不支持的档位暴露会报错的 1M 选项。跟随供应商时纯透传，不做名称映射。
const DEFAULT_ROUTES: &[(&str, bool)] = &[
    ("claude-sonnet-4-6", false),
    ("claude-opus-4-8", true),
    ("claude-haiku-4-5", false),
    ("claude-fable-5", false),
];

#[derive(Debug, Clone)]
struct Paths {
    normal_config: PathBuf,
    threep_config: PathBuf,
    profile: PathBuf,
    meta: PathBuf,
}

struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

/// 仅 macOS / Windows 支持 Claude 桌面版 3p 配置。
pub fn is_supported() -> bool {
    cfg!(any(target_os = "macos", windows))
}

/// 桌面 App 是否已安装（其数据目录存在）。未安装时所有写盘 no-op，避免为不存在的
/// App 凭空造出 `Claude` / `Claude-3p` 目录与配置文件。
fn desktop_app_present(paths: &Paths) -> bool {
    paths.normal_config.parent().is_some_and(Path::exists)
        || paths.threep_config.parent().is_some_and(Path::exists)
}

/// 直连模式：用供应商自己的 Base URL + key 写网关 profile。
pub fn apply_direct(provider: &Provider) -> Result<(), String> {
    let (base_url, api_key) = direct_credentials(provider)?;
    apply_gateway(&base_url, &api_key)
}

/// 代理模式：网关指向本地代理端点（token 用占位值，代理会注入真实 key）。
pub fn apply_proxy(local_base: &str) -> Result<(), String> {
    apply_gateway(local_base, GATEWAY_TOKEN)
}

fn apply_gateway(base_url: &str, api_key: &str) -> Result<(), String> {
    let paths = current_paths()?;
    if !desktop_app_present(&paths) {
        return Ok(()); // 桌面 App 未安装，无需写盘
    }
    let profile = build_profile(base_url, api_key);
    with_rollback(&paths, |paths| {
        write_deployment_mode(&paths.normal_config, "3p")?;
        write_deployment_mode(&paths.threep_config, "3p")?;
        write_json_file(&paths.profile, &profile)?;
        write_meta(&paths.meta, Some(PROFILE_ID))
    })
}

/// 恢复桌面版官方（1p）模式：两个 config 回 1p、删 profile、清 _meta、抹 enterpriseConfig。
pub fn restore_official() -> Result<(), String> {
    let paths = current_paths()?;
    if !desktop_app_present(&paths) {
        return Ok(()); // 桌面 App 未安装，无需回滚
    }
    with_rollback(&paths, |paths| {
        write_deployment_mode(&paths.normal_config, "1p")?;
        write_deployment_mode(&paths.threep_config, "1p")?;
        remove_enterprise_config(&paths.threep_config)?;
        if paths.profile.exists() {
            fs::remove_file(&paths.profile)
                .map_err(|e| format!("删除 {} 失败: {e}", paths.profile.display()))?;
        }
        write_meta(&paths.meta, None)
    })
}

// ---------- profile / 凭据 ----------

fn build_profile(base_url: &str, api_key: &str) -> Value {
    // labelOverride 与 name 同值：桌面版菜单直接显示官方模型名。真机上每个
    // inferenceModels 条目都带 labelOverride，这里对齐已验证的 profile 形状；
    // supports1m 仅在支持的档位（opus）出现，其它档不写该键。
    let models: Vec<Value> = DEFAULT_ROUTES
        .iter()
        .map(|(id, one_m)| {
            let mut m = json!({ "name": id, "labelOverride": id });
            if *one_m {
                m["supports1m"] = json!(true);
            }
            m
        })
        .collect();
    json!({
        "coworkEgressAllowedHosts": ["*"],
        "disableDeploymentModeChooser": true,
        "inferenceGatewayApiKey": api_key,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceGatewayBaseUrl": base_url,
        "inferenceModels": models,
        "inferenceProvider": "gateway"
    })
}

/// 从供应商 env 取 Base URL + key（key 字段读 meta.apiKeyField，缺省 ANTHROPIC_AUTH_TOKEN）。
/// 桌面版 profile 只有 bearer 方案，key 恒以 bearer 写入。缺 Base URL 或 key → Err。
fn direct_credentials(provider: &Provider) -> Result<(String, String), String> {
    let env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .ok_or("供应商缺少 env 配置")?;
    let base_url = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or("供应商缺少 ANTHROPIC_BASE_URL")?
        .to_string();
    let key_field = provider
        .meta
        .get("apiKeyField")
        .and_then(Value::as_str)
        .unwrap_or("ANTHROPIC_AUTH_TOKEN");
    let api_key = env
        .get(key_field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("供应商缺少 {key_field}"))?
        .to_string();
    Ok((base_url, api_key))
}

// ---------- 读改写 helper ----------

/// 读出 JSON 对象；不存在/空/非对象 → 空对象（绝不整体覆盖已有非对象内容之外的字段）。
fn read_obj_or_empty(path: &Path) -> Map<String, Value> {
    if !path.exists() {
        return Map::new();
    }
    match read_json_file::<Value>(path) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn write_deployment_mode(path: &Path, mode: &str) -> Result<(), String> {
    let mut obj = read_obj_or_empty(path);
    obj.insert("deploymentMode".into(), Value::String(mode.into()));
    write_json_file(path, &Value::Object(obj))
}

fn write_meta(path: &Path, applied_profile_id: Option<&str>) -> Result<(), String> {
    let mut obj = read_obj_or_empty(path);
    let mut entries = obj
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries.retain(|e| e.get("id").and_then(Value::as_str) != Some(PROFILE_ID));

    match applied_profile_id {
        Some(id) => {
            entries.push(json!({ "id": PROFILE_ID, "name": PROFILE_NAME }));
            obj.insert("appliedId".into(), Value::String(id.into()));
        }
        None => {
            // 仅当 appliedId 指向我们时才清/改，避免动到别的 profile。
            let ours = obj
                .get("appliedId")
                .and_then(Value::as_str)
                .is_some_and(|id| id == PROFILE_ID);
            if ours {
                match entries
                    .iter()
                    .find_map(|e| e.get("id").and_then(Value::as_str))
                {
                    Some(next) => {
                        obj.insert("appliedId".into(), Value::String(next.into()));
                    }
                    None => {
                        obj.remove("appliedId");
                    }
                }
            }
        }
    }

    obj.insert("entries".into(), Value::Array(entries));
    write_json_file(path, &Value::Object(obj))
}

fn remove_enterprise_config(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut obj = read_obj_or_empty(path);
    let Some(enterprise) = obj.get_mut("enterpriseConfig").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    for key in ENTERPRISE_KEYS {
        enterprise.remove(*key);
    }
    if enterprise.is_empty() {
        obj.remove("enterpriseConfig");
    }
    write_json_file(path, &Value::Object(obj))
}

// ---------- 快照 / 回滚 ----------

fn with_rollback<F>(paths: &Paths, op: F) -> Result<(), String>
where
    F: FnOnce(&Paths) -> Result<(), String>,
{
    let snapshots = snapshot(paths)?;
    match op(paths) {
        Ok(()) => Ok(()),
        Err(err) => match restore(&snapshots) {
            Ok(()) => Err(err),
            Err(rollback_err) => Err(format!("{err}; 回滚也失败: {rollback_err}")),
        },
    }
}

fn snapshot(paths: &Paths) -> Result<Vec<FileSnapshot>, String> {
    [
        &paths.normal_config,
        &paths.threep_config,
        &paths.profile,
        &paths.meta,
    ]
    .into_iter()
    .map(|path| {
        let content = if path.exists() {
            Some(fs::read(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?)
        } else {
            None
        };
        Ok(FileSnapshot {
            path: path.clone(),
            content,
        })
    })
    .collect()
}

fn restore(snapshots: &[FileSnapshot]) -> Result<(), String> {
    for snap in snapshots {
        match &snap.content {
            Some(content) => atomic_write(&snap.path, content)?,
            None => {
                if snap.path.exists() {
                    fs::remove_file(&snap.path)
                        .map_err(|e| format!("删除 {} 失败: {e}", snap.path.display()))?;
                }
            }
        }
    }
    Ok(())
}

// ---------- 平台路径 ----------

#[allow(clippy::needless_return)]
fn current_paths() -> Result<Paths, String> {
    #[cfg(target_os = "macos")]
    {
        let app_support = crate::config::get_home_dir()
            .join("Library")
            .join("Application Support");
        return Ok(paths_from_dirs(
            app_support.join("Claude"),
            app_support.join("Claude-3p"),
        ));
    }

    #[cfg(windows)]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                crate::config::get_home_dir()
                    .join("AppData")
                    .join("Local")
            });
        let normal = pick_windows_claude_dir(&local_app_data, false)
            .unwrap_or_else(|| local_app_data.join("Claude"));
        let threep = pick_windows_claude_dir(&local_app_data, true)
            .unwrap_or_else(|| local_app_data.join("Claude-3p"));
        return Ok(paths_from_dirs(normal, threep));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err("当前平台不支持 Claude 桌面版 3p 配置（仅 macOS / Windows）".into())
    }
}

/// Windows 上桌面版安装目录可能带后缀（如 `Claude-3p-abc`）：精确名不存在时扫一遍。
#[cfg(windows)]
fn pick_windows_claude_dir(local_app_data: &Path, threep: bool) -> Option<PathBuf> {
    let exact = local_app_data.join(if threep { "Claude-3p" } else { "Claude" });
    if exact.exists() {
        return Some(exact);
    }
    let mut candidates: Vec<PathBuf> = fs::read_dir(local_app_data)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            name.starts_with("Claude") && name.contains("-3p") == threep
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

#[cfg(any(target_os = "macos", windows, test))]
fn paths_from_dirs(normal_dir: PathBuf, threep_dir: PathBuf) -> Paths {
    let library = threep_dir.join(CONFIG_LIBRARY_DIR);
    Paths {
        normal_config: normal_dir.join(CONFIG_FILE),
        threep_config: threep_dir.join(CONFIG_FILE),
        profile: library.join(format!("{PROFILE_ID}.json")),
        meta: library.join("_meta.json"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 自建临时目录（无 tempfile 依赖，Drop 时清理）。每个用例用独立目录 +
    /// 显式 Paths，互不干扰，无需全局锁。
    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "zsw-claude-desktop-{}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_paths(root: &Path) -> Paths {
        paths_from_dirs(root.join("Claude"), root.join("Claude-3p"))
    }

    fn apply_gateway_at(paths: &Paths, base_url: &str, api_key: &str) -> Result<(), String> {
        let profile = build_profile(base_url, api_key);
        with_rollback(paths, |paths| {
            write_deployment_mode(&paths.normal_config, "3p")?;
            write_deployment_mode(&paths.threep_config, "3p")?;
            write_json_file(&paths.profile, &profile)?;
            write_meta(&paths.meta, Some(PROFILE_ID))
        })
    }

    fn restore_at(paths: &Paths) -> Result<(), String> {
        with_rollback(paths, |paths| {
            write_deployment_mode(&paths.normal_config, "1p")?;
            write_deployment_mode(&paths.threep_config, "1p")?;
            remove_enterprise_config(&paths.threep_config)?;
            if paths.profile.exists() {
                fs::remove_file(&paths.profile).map_err(|e| e.to_string())?;
            }
            write_meta(&paths.meta, None)
        })
    }

    fn third_party_provider() -> Provider {
        Provider {
            id: "relay".into(),
            name: "Relay".into(),
            category: Some("custom".into()),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://relay.example.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-real-token"
                }
            }),
            meta: json!({}),
            failover: json!({}),
        }
    }

    #[test]
    fn desktop_app_present_reflects_data_dir() {
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        assert!(!desktop_app_present(&paths)); // Claude / Claude-3p 均不存在
        fs::create_dir_all(paths.threep_config.parent().unwrap()).unwrap();
        assert!(desktop_app_present(&paths)); // 3p 目录存在即视为已安装
    }

    #[test]
    fn direct_credentials_reads_base_and_key() {
        let (base, key) = direct_credentials(&third_party_provider()).unwrap();
        assert_eq!(base, "https://relay.example.com");
        assert_eq!(key, "sk-real-token");
    }

    #[test]
    fn direct_credentials_honors_api_key_field() {
        let mut p = third_party_provider();
        p.settings_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://relay.example.com",
                "ANTHROPIC_API_KEY": "sk-xkey"
            }
        });
        p.meta = json!({ "apiKeyField": "ANTHROPIC_API_KEY" });
        let (_, key) = direct_credentials(&p).unwrap();
        assert_eq!(key, "sk-xkey");
    }

    #[test]
    fn direct_credentials_missing_base_url_errors() {
        let mut p = third_party_provider();
        p.settings_config = json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "x" } });
        assert!(direct_credentials(&p).is_err());
    }

    #[test]
    fn apply_direct_writes_3p_profile_meta_with_default_models() {
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        let (base, key) = direct_credentials(&third_party_provider()).unwrap();
        apply_gateway_at(&paths, &base, &key).unwrap();

        let normal: Value = read_json_file(&paths.normal_config).unwrap();
        let threep: Value = read_json_file(&paths.threep_config).unwrap();
        let profile: Value = read_json_file(&paths.profile).unwrap();
        let meta: Value = read_json_file(&paths.meta).unwrap();

        assert_eq!(normal["deploymentMode"], json!("3p"));
        assert_eq!(threep["deploymentMode"], json!("3p"));
        assert_eq!(profile["inferenceProvider"], json!("gateway"));
        assert_eq!(profile["inferenceGatewayAuthScheme"], json!("bearer"));
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            json!("https://relay.example.com")
        );
        assert_eq!(profile["inferenceGatewayApiKey"], json!("sk-real-token"));
        assert_eq!(profile["disableDeploymentModeChooser"], json!(true));
        assert_eq!(profile["coworkEgressAllowedHosts"], json!(["*"]));

        let models = profile["inferenceModels"].as_array().unwrap();
        assert_eq!(models.len(), 4);
        // 每档 name==labelOverride；supports1m 仅出现在 opus 档（对齐真机配置）。
        assert!(models.iter().all(|m| m["name"] == m["labelOverride"]));
        let find = |id: &str| models.iter().find(|m| m["name"] == json!(id)).unwrap();
        assert_eq!(find("claude-opus-4-8")["supports1m"], json!(true));
        assert!(find("claude-sonnet-4-6").get("supports1m").is_none());
        assert!(find("claude-haiku-4-5").get("supports1m").is_none());
        assert!(find("claude-fable-5").get("supports1m").is_none());
        assert_eq!(models[0]["name"], json!("claude-sonnet-4-6"));

        assert_eq!(meta["appliedId"], json!(PROFILE_ID));
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == json!(PROFILE_ID) && e["name"] == json!(PROFILE_NAME)));
    }

    #[test]
    fn apply_proxy_writes_localhost_base_and_placeholder_token() {
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        apply_gateway_at(&paths, "http://127.0.0.1:8899/claude", GATEWAY_TOKEN).unwrap();

        let profile: Value = read_json_file(&paths.profile).unwrap();
        assert_eq!(
            profile["inferenceGatewayBaseUrl"],
            json!("http://127.0.0.1:8899/claude")
        );
        assert_eq!(profile["inferenceGatewayApiKey"], json!(GATEWAY_TOKEN));
    }

    #[test]
    fn restore_switches_to_1p_and_removes_profile() {
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        apply_gateway_at(&paths, "https://relay.example.com", "sk-real-token").unwrap();
        restore_at(&paths).unwrap();

        let normal: Value = read_json_file(&paths.normal_config).unwrap();
        let threep: Value = read_json_file(&paths.threep_config).unwrap();
        let meta: Value = read_json_file(&paths.meta).unwrap();

        assert_eq!(normal["deploymentMode"], json!("1p"));
        assert_eq!(threep["deploymentMode"], json!("1p"));
        assert!(!paths.profile.exists());
        assert!(meta.get("appliedId").is_none());
        assert!(!meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == json!(PROFILE_ID)));
    }

    #[test]
    fn restore_preserves_foreign_profile_and_applied_id() {
        // _meta 已被别的 profile 占用 appliedId 时，恢复不能动它。
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        apply_gateway_at(&paths, "https://relay.example.com", "sk-real-token").unwrap();

        // 模拟另一个工具的 profile 抢占 appliedId。
        let mut meta = read_obj_or_empty(&paths.meta);
        let mut entries = meta["entries"].as_array().unwrap().clone();
        entries.push(json!({ "id": "foreign-id", "name": "Other" }));
        meta.insert("entries".into(), Value::Array(entries));
        meta.insert("appliedId".into(), json!("foreign-id"));
        write_json_file(&paths.meta, &Value::Object(meta)).unwrap();

        restore_at(&paths).unwrap();

        let meta: Value = read_json_file(&paths.meta).unwrap();
        assert_eq!(meta["appliedId"], json!("foreign-id")); // 未被清掉
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == json!("foreign-id")));
        assert!(!meta["entries"] // 我们的 entry 已移除
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == json!(PROFILE_ID)));
    }

    #[test]
    fn remove_enterprise_config_strips_only_our_keys() {
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        write_json_file(
            &paths.threep_config,
            &json!({
                "deploymentMode": "3p",
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://x",
                    "keepMe": true
                }
            }),
        )
        .unwrap();

        remove_enterprise_config(&paths.threep_config).unwrap();
        let cfg: Value = read_json_file(&paths.threep_config).unwrap();
        assert_eq!(cfg["enterpriseConfig"]["keepMe"], json!(true));
        assert!(cfg["enterpriseConfig"].get("inferenceProvider").is_none());
    }

    #[test]
    fn apply_rolls_back_both_configs_when_profile_write_fails() {
        let temp = Tmp::new();
        let paths = test_paths(temp.path());
        write_json_file(&paths.normal_config, &json!({"deploymentMode": "1p", "keep": 1})).unwrap();
        write_json_file(&paths.threep_config, &json!({"deploymentMode": "1p", "keep": 2})).unwrap();
        // 用一个文件占住 profile 的父目录，令写 profile 失败。
        let library = paths.profile.parent().unwrap();
        fs::create_dir_all(library.parent().unwrap()).unwrap();
        fs::write(library, "not a dir").unwrap();

        apply_gateway_at(&paths, "https://relay.example.com", "sk").unwrap_err();

        let normal: Value = read_json_file(&paths.normal_config).unwrap();
        let threep: Value = read_json_file(&paths.threep_config).unwrap();
        assert_eq!(normal, json!({"deploymentMode": "1p", "keep": 1}));
        assert_eq!(threep, json!({"deploymentMode": "1p", "keep": 2}));
    }
}
