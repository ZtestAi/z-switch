//! 首次运行时保存 Claude/Codex 原始配置，并提供可重复的一键恢复。
//! 原始快照独立于供应商列表和普通导出，避免删除供应商时丢失恢复基线。
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{config, live};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    captured_at: u64,
    claude_settings_existed: bool,
    codex_auth_existed: bool,
    codex_config_existed: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OriginalConfigStatus {
    pub captured: bool,
    pub captured_at: Option<u64>,
    pub claude_had_config: bool,
    pub codex_had_config: bool,
}

fn dir() -> PathBuf {
    config::get_app_config_dir().join("original")
}

fn manifest_path() -> PathBuf {
    dir().join("manifest.json")
}

fn snapshot_path(name: &str) -> PathBuf {
    dir().join(name)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn capture_file(source: &Path, snapshot_name: &str) -> Result<bool, String> {
    if !source.exists() {
        return Ok(false);
    }
    let bytes = fs::read(source).map_err(|e| format!("读取 {} 失败: {e}", source.display()))?;
    config::atomic_write(&snapshot_path(snapshot_name), &bytes)?;
    Ok(true)
}

fn load_manifest() -> Result<Manifest, String> {
    config::read_json_file(&manifest_path())
}

fn to_status(manifest: &Manifest) -> OriginalConfigStatus {
    OriginalConfigStatus {
        captured: true,
        captured_at: Some(manifest.captured_at),
        claude_had_config: manifest.claude_settings_existed,
        codex_had_config: manifest.codex_auth_existed || manifest.codex_config_existed,
    }
}

/// 仅在快照不存在时创建一次。返回 true 表示本次刚刚创建。
pub fn capture_once() -> Result<bool, String> {
    if manifest_path().exists() {
        load_manifest()?;
        return Ok(false);
    }

    let manifest = Manifest {
        version: 1,
        captured_at: now_millis(),
        claude_settings_existed: capture_file(
            &config::get_claude_settings_path(),
            "claude-settings.json",
        )?,
        codex_auth_existed: capture_file(&config::get_codex_auth_path(), "codex-auth.json")?,
        codex_config_existed: capture_file(&config::get_codex_config_path(), "codex-config.toml")?,
    };
    config::write_json_file(&manifest_path(), &manifest)?;
    Ok(true)
}

pub fn status() -> OriginalConfigStatus {
    match load_manifest() {
        Ok(manifest) => to_status(&manifest),
        Err(_) => OriginalConfigStatus {
            captured: false,
            captured_at: None,
            claude_had_config: false,
            codex_had_config: false,
        },
    }
}

fn apply_file(destination: &Path, snapshot_name: &str, existed: bool) -> Result<(), String> {
    if existed {
        let snapshot = snapshot_path(snapshot_name);
        let bytes = fs::read(&snapshot)
            .map_err(|e| format!("读取原始快照 {} 失败: {e}", snapshot.display()))?;
        config::atomic_write(destination, &bytes)
    } else if destination.exists() {
        fs::remove_file(destination)
            .map_err(|e| format!("移除 {} 失败: {e}", destination.display()))
    } else {
        Ok(())
    }
}

/// 恢复指定应用的原始文件。恢复前仍会写一份常规时间戳备份。
pub fn restore_app(app: &str) -> Result<(), String> {
    let manifest = load_manifest().map_err(|_| "尚未保存本机原始配置".to_string())?;
    live::backup_current_app(app);

    match app {
        "claude" => apply_file(
            &config::get_claude_settings_path(),
            "claude-settings.json",
            manifest.claude_settings_existed,
        ),
        "codex" => {
            let auth_path = config::get_codex_auth_path();
            let config_path = config::get_codex_config_path();
            let old_auth = fs::read(&auth_path).ok();

            apply_file(&auth_path, "codex-auth.json", manifest.codex_auth_existed)?;
            if let Err(error) = apply_file(
                &config_path,
                "codex-config.toml",
                manifest.codex_config_existed,
            ) {
                match old_auth {
                    Some(bytes) => {
                        let _ = config::atomic_write(&auth_path, &bytes);
                    }
                    None => {
                        let _ = fs::remove_file(&auth_path);
                    }
                }
                return Err(format!("恢复 config.toml 失败，auth.json 已回滚：{error}"));
            }
            Ok(())
        }
        other => Err(format!("未知应用: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHome {
        path: PathBuf,
        previous: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "z-switch-original-test-{}-{}",
                std::process::id(),
                now_millis()
            ));
            fs::create_dir_all(&path).unwrap();
            let previous = std::env::var_os("Z_SWITCH_TEST_HOME");
            std::env::set_var("Z_SWITCH_TEST_HOME", &path);
            Self { path, previous }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("Z_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("Z_SWITCH_TEST_HOME"),
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn capture_once_restores_exact_files_and_original_absence() {
        let _home = TestHome::new();
        let claude_path = config::get_claude_settings_path();
        fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
        fs::write(
            &claude_path,
            b"{\"env\":{\"KEY\":\"original\"},\"keep\":true}",
        )
        .unwrap();

        assert!(capture_once().unwrap());
        assert!(!capture_once().unwrap());
        assert!(status().captured);
        assert!(status().claude_had_config);
        assert!(!status().codex_had_config);

        fs::write(&claude_path, b"changed").unwrap();
        let codex_auth = config::get_codex_auth_path();
        let codex_config = config::get_codex_config_path();
        fs::create_dir_all(codex_auth.parent().unwrap()).unwrap();
        fs::write(&codex_auth, b"created later").unwrap();
        fs::write(&codex_config, b"created later").unwrap();

        restore_app("claude").unwrap();
        restore_app("codex").unwrap();

        assert_eq!(
            fs::read(&claude_path).unwrap(),
            b"{\"env\":{\"KEY\":\"original\"},\"keep\":true}"
        );
        assert!(!codex_auth.exists());
        assert!(!codex_config.exists());
    }
}
