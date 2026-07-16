//! 本地路由错误日志：只记录失败、限制体积、写盘前脱敏。
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::config;

const LOG_FILE: &str = "proxy-errors.jsonl";
const ROTATED_FILE: &str = "proxy-errors.jsonl.1";
const MAX_DETAIL_CHARS: usize = 16_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyErrorEntry<'a> {
    pub timestamp_ms: u128,
    pub app: &'a str,
    pub status: Option<u16>,
    pub url: &'a str,
    pub phase: &'a str,
    pub detail: &'a str,
}

pub fn log_dir() -> PathBuf {
    config::get_app_config_dir().join("logs")
}

fn log_path() -> PathBuf {
    log_dir().join(LOG_FILE)
}

fn rotated_path() -> PathBuf {
    log_dir().join(ROTATED_FILE)
}

pub fn sanitize_url(raw: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        return raw.split('?').next().unwrap_or(raw).to_string();
    };
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.to_string()
}

pub fn redact_and_truncate(raw: &str, secrets: &[String]) -> String {
    let mut out = raw.to_string();
    for secret in secrets {
        let secret = secret.trim();
        if secret.len() >= 4 {
            out = out.replace(secret, "[REDACTED]");
        }
        if let Some(token) = secret.strip_prefix("Bearer ") {
            if token.len() >= 4 {
                out = out.replace(token, "[REDACTED]");
            }
        }
    }
    if out.chars().count() > MAX_DETAIL_CHARS {
        out = out.chars().take(MAX_DETAIL_CHARS).collect::<String>();
        out.push_str("…[truncated]");
    }
    out
}

pub fn append(entry: &ProxyErrorEntry<'_>, max_mb: u64) -> Result<(), String> {
    let dir = log_dir();
    fs::create_dir_all(&dir)
        .map_err(|error| format!("创建路由日志目录 {} 失败：{error}", dir.display()))?;
    let path = log_path();
    let max_bytes = max_mb.clamp(1, 100) * 1024 * 1024;
    let mut line = serde_json::to_string(entry).map_err(|error| error.to_string())?;
    line.push('\n');

    let current_size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    if current_size.saturating_add(line.len() as u64) > max_bytes {
        let rotated = rotated_path();
        let _ = fs::remove_file(&rotated);
        if path.exists() {
            fs::rename(&path, &rotated)
                .map_err(|error| format!("轮转路由错误日志失败：{error}"))?;
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("打开路由错误日志 {} 失败：{error}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("写入路由错误日志失败：{error}"))
}

pub fn clear() -> Result<(), String> {
    for path in [log_path(), rotated_path()] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("删除日志 {} 失败：{error}", path.display())),
        }
    }
    Ok(())
}

pub fn open_folder() -> Result<(), String> {
    let path = log_dir();
    fs::create_dir_all(&path)
        .map_err(|error| format!("创建路由日志目录 {} 失败：{error}", path.display()))?;
    let path = fs::canonicalize(&path).unwrap_or(path);

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
        .map_err(|error| format!("打开路由日志目录 {} 失败：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_url_and_secrets() {
        assert_eq!(
            sanitize_url("https://user:pass@example.com/v1?token=secret#x"),
            "https://example.com/v1"
        );
        let text = redact_and_truncate(
            "authorization failed for Bearer sk-secret and sk-secret",
            &["Bearer sk-secret".into()],
        );
        assert!(!text.contains("sk-secret"));
        assert!(text.contains("[REDACTED]"));
    }
}
