//! Claude Code 生态增强（VS Code 扩展放行 + 跳过初次确认）。
//!
//! 与 live 写盘分开：这里只对两个「Claude Code 自身」的 JSON 文件做**增量单字段**
//! 读改写，保留其它字段，绝不整体覆盖。均为 best-effort 的体验优化：
//! - `~/.claude/config.json` 的 `primaryApiKey`：VS Code Claude Code 扩展有独立鉴权
//!   门槛，光改 settings.json 的 env 它不认；写 `"any"` 相当于告诉扩展「已有 key，
//!   别拦」，从而随第三方供应商走；官方时删除该键，让扩展回到原生登录。
//! - `~/.claude.json` 的 `hasCompletedOnboarding`：写 `true` 跳过 Claude Code 首次
//!   运行的引导/确认。
use crate::config;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// VS Code 扩展读的配置：~/.claude/config.json
fn claude_config_path() -> PathBuf {
    config::get_home_dir().join(".claude").join("config.json")
}

/// Claude Code 根配置：~/.claude.json
fn claude_json_path() -> PathBuf {
    config::get_home_dir().join(".claude.json")
}

/// 读出 JSON 对象；文件不存在/空 → 空对象；存在但非法或非对象 → Err（中止，绝不覆盖）。
fn read_obj_or_empty(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let value: Value = config::read_json_file(path)?;
    match value {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(Map::new()),
        _ => Err(format!("{} 不是 JSON 对象，已跳过", path.display())),
    }
}

/// 应用/清除 VS Code 扩展放行标记。
/// `managed=true` → 写 `primaryApiKey="any"`；`false` → 删除该键。保留其它字段。
pub fn apply_primary_api_key(managed: bool) -> Result<(), String> {
    let path = claude_config_path();
    let mut obj = read_obj_or_empty(&path)?;
    if managed {
        obj.insert("primaryApiKey".into(), Value::String("any".into()));
    } else if obj.remove("primaryApiKey").is_none() {
        // 本就没有该键，且文件原本不存在时，不必凭空创建文件。
        if !path.exists() {
            return Ok(());
        }
    }
    config::write_json_file(&path, &Value::Object(obj))
}

/// 应用/清除「跳过初次安装确认」。
/// `enabled=true` → 写 `hasCompletedOnboarding=true`；`false` → 删除该键。保留其它字段。
pub fn apply_onboarding_completed(enabled: bool) -> Result<(), String> {
    let path = claude_json_path();
    let mut obj = read_obj_or_empty(&path)?;
    if enabled {
        obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));
    } else if obj.remove("hasCompletedOnboarding").is_none() {
        if !path.exists() {
            return Ok(());
        }
    }
    config::write_json_file(&path, &Value::Object(obj))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce(&Path)>(f: F) {
        // 与其它改 Z_SWITCH_TEST_HOME 的测试共用一把锁，避免并发污染 home。
        let _guard = config::TEST_HOME_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "zsw-claude-ext-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("Z_SWITCH_TEST_HOME", &dir);
        f(&dir);
        std::env::remove_var("Z_SWITCH_TEST_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn primary_api_key_set_and_clear_preserves_other_keys() {
        with_temp_home(|home| {
            let path = home.join(".claude").join("config.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();

            apply_primary_api_key(true).unwrap();
            let v: Value = config::read_json_file(&path).unwrap();
            assert_eq!(v["primaryApiKey"], Value::String("any".into()));
            assert_eq!(v["theme"], Value::String("dark".into())); // 其它字段保留

            apply_primary_api_key(false).unwrap();
            let v: Value = config::read_json_file(&path).unwrap();
            assert!(v.get("primaryApiKey").is_none());
            assert_eq!(v["theme"], Value::String("dark".into()));
        });
    }

    #[test]
    fn clear_on_missing_file_is_noop() {
        with_temp_home(|home| {
            let path = home.join(".claude").join("config.json");
            apply_primary_api_key(false).unwrap();
            assert!(!path.exists()); // 不凭空创建文件
        });
    }

    #[test]
    fn onboarding_completed_toggles() {
        with_temp_home(|home| {
            let path = home.join(".claude.json");
            std::fs::write(&path, r#"{"numStartups":3}"#).unwrap();

            apply_onboarding_completed(true).unwrap();
            let v: Value = config::read_json_file(&path).unwrap();
            assert_eq!(v["hasCompletedOnboarding"], Value::Bool(true));
            assert_eq!(v["numStartups"], serde_json::json!(3));

            apply_onboarding_completed(false).unwrap();
            let v: Value = config::read_json_file(&path).unwrap();
            assert!(v.get("hasCompletedOnboarding").is_none());
        });
    }
}
