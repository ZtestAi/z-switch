//! 文件路径 + 原子读写。安全写盘核心，移植自 cc-switch 的 `config.rs::atomic_write`。
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 用户主目录。测试可用 `Z_SWITCH_TEST_HOME` 覆盖。
/// 注意：不读 `HOME` 环境变量（可能被 Git/MSYS 注入，导致路径漂移）。
pub fn get_home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("Z_SWITCH_TEST_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// z-switch 自身数据目录：~/.z-switch
pub fn get_app_config_dir() -> PathBuf {
    get_home_dir().join(".z-switch")
}

/// providers.json 路径
pub fn get_store_path() -> PathBuf {
    get_app_config_dir().join("providers.json")
}

/// z-switch 管理的本机账号凭据快照目录。它不属于 providers.json，
/// 不会进入普通配置导出；当前仅用于保留 Codex 客户端刷新后的登录态。
pub fn get_official_account_dir() -> PathBuf {
    get_app_config_dir().join("official")
}

/// Claude Code 主配置：~/.claude/settings.json（阶段 2 live 写盘用）
#[allow(dead_code)]
pub fn get_claude_settings_path() -> PathBuf {
    get_home_dir().join(".claude").join("settings.json")
}

/// Codex auth：~/.codex/auth.json（阶段 2 live 写盘用）
#[allow(dead_code)]
pub fn get_codex_auth_path() -> PathBuf {
    get_home_dir().join(".codex").join("auth.json")
}

/// Codex config：~/.codex/config.toml（阶段 2 live 写盘用）
#[allow(dead_code)]
pub fn get_codex_config_path() -> PathBuf {
    get_home_dir().join(".codex").join("config.toml")
}

/// 递归按字母排序对象的键，保证序列化输出确定性（数组顺序不变）。
fn sort_json_keys(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), sort_json_keys(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_json_keys).collect()),
        other => other.clone(),
    }
}

/// 读取并反序列化 JSON 文件
pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    serde_json::from_str(&content).map_err(|e| format!("解析 {} 失败: {e}", path.display()))
}

/// 序列化并原子写入 JSON 文件（键排序，确定性输出）
pub fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let value = serde_json::to_value(data).map_err(|e| e.to_string())?;
    let sorted = sort_json_keys(&value);
    let json = serde_json::to_string_pretty(&sorted).map_err(|e| e.to_string())?;
    atomic_write(path, json.as_bytes())
}

/// 原子写入文本文件（Codex config.toml 等，阶段 2 用）
#[allow(dead_code)]
pub fn write_text_file(path: &Path, data: &str) -> Result<(), String> {
    atomic_write(path, data.as_bytes())
}

/// 原子写入：写临时文件（带纳秒后缀）→ rename 替换，避免半写状态。
/// Windows 上 rename 目标存在会失败，先删再重命名（尽量接近原子）。
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "无效的路径（无父目录）".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("创建目录 {} 失败: {e}", parent.display()))?;

    let file_name = path
        .file_name()
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string_lossy()
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = parent.join(format!("{file_name}.tmp.{ts}"));

    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {e}"))?;
        f.write_all(data)
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        f.flush().map_err(|e| format!("flush 失败: {e}"))?;
    }

    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("原子替换失败 {} -> {}: {e}", tmp.display(), path.display())
    })
}

/// 测试专用：串行化所有会改动进程级 `Z_SWITCH_TEST_HOME` 的用例。
/// get_home_dir 读进程级环境变量，多个测试并发改它会互相看到对方的 home
/// （甚至被 remove_var 打回真实 home），因此凡是设置该变量的测试都要先拿这把锁。
#[cfg(test)]
pub(crate) static TEST_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
