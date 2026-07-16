//! 本机官方账号凭据保护层。
//!
//! 官方登录态不属于普通供应商配置，不能写入 providers.json 或配置导出。
//! Codex 在使用期间会刷新 auth.json；离开官方账号前保存最新副本，切回时恢复。
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::config;

fn codex_snapshot_path() -> PathBuf {
    config::get_official_account_dir().join("codex-auth.json")
}

fn has_login_material(auth: &Value) -> bool {
    let Some(object) = auth.as_object() else {
        return false;
    };
    if object
        .get("tokens")
        .and_then(Value::as_object)
        .is_some_and(|tokens| !tokens.is_empty())
    {
        return true;
    }
    if ["access_token", "refresh_token", "id_token"]
        .iter()
        .any(|key| {
            object
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
    {
        return true;
    }
    object
        .get("auth_mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| {
            !matches!(
                mode.trim().to_ascii_lowercase().as_str(),
                "" | "apikey" | "api_key"
            )
        })
}

fn secure_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    let _ = path;
}

fn save_auth(auth: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(auth).map_err(|error| error.to_string())?;
    let path = codex_snapshot_path();
    config::atomic_write(&path, &bytes)?;
    secure_permissions(&path);
    Ok(())
}

/// 首次启动/升级时只在检测到真实登录材料时建立快照，避免把中转 API Key
/// 误认为本机账号。
pub fn capture_codex_if_logged_in() -> Result<bool, String> {
    let path = config::get_codex_auth_path();
    if !path.exists() {
        return Ok(false);
    }
    let mut auth: Value = config::read_json_file(&path)?;
    if let Some(object) = auth.as_object_mut() {
        object.remove("OPENAI_API_KEY");
    }
    if !has_login_material(&auth) {
        return Ok(false);
    }
    save_auth(&auth)?;
    Ok(true)
}

/// 官方账号处于当前项时，离开前保存 Codex 刚刚刷新过的登录凭据。
pub fn capture_codex_current() -> Result<(), String> {
    let path = config::get_codex_auth_path();
    if !path.exists() {
        let snapshot = codex_snapshot_path();
        if snapshot.exists() {
            fs::remove_file(&snapshot)
                .map_err(|error| format!("移除已退出的 Codex 登录快照失败：{error}"))?;
        }
        return Ok(());
    }
    let mut auth: Value = config::read_json_file(&path)?;
    if let Some(object) = auth.as_object_mut() {
        object.remove("OPENAI_API_KEY");
    }
    if has_login_material(&auth) {
        save_auth(&auth)
    } else {
        let snapshot = codex_snapshot_path();
        if snapshot.exists() {
            fs::remove_file(&snapshot)
                .map_err(|error| format!("移除已退出的 Codex 登录快照失败：{error}"))?;
        }
        Ok(())
    }
}

/// 读取本机账号快照。不存在时返回空对象，让 Codex 在下一次使用时进入登录流程。
pub fn codex_auth_for_restore() -> Result<Value, String> {
    let path = codex_snapshot_path();
    if path.exists() {
        return config::read_json_file(&path);
    }
    Ok(serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_material_is_distinct_from_relay_api_key() {
        let relay_only = serde_json::json!({
            "OPENAI_API_KEY": "relay-secret",
            "auth_mode": "apikey"
        });
        assert!(!has_login_material(&relay_only));

        let mut mixed = serde_json::json!({
            "OPENAI_API_KEY": "relay-secret",
            "auth_mode": "chatgpt",
            "tokens": { "access_token": "oauth-secret" }
        });
        mixed.as_object_mut().unwrap().remove("OPENAI_API_KEY");
        assert!(has_login_material(&mixed));
        assert!(mixed.get("OPENAI_API_KEY").is_none());
    }
}
