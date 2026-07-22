//! 环境自检：检测 live 配置里的「本地代理占位残留」。
//!
//! 故障模型：开启本地路由时，live 文件的 base_url 会被指向 127.0.0.1:<port>/<app>，
//! 密钥被替换成占位符（PLACEHOLDER_KEY）。正常情况下代理在跑、由 z-switch 托管；
//! 但如果代理异常退出、z-switch 被卸载、或配置被外部复制，就会遗留 localhost 地址
//! 与占位密钥，导致客户端既连不上代理也连不上真实供应商。
//!
//! 本模块只做「只读检测」；修复（备份后重写为直连）在 lib.rs 的命令里完成。
use serde_json::Value;
use std::fs;

use crate::config;
use crate::proxy::PLACEHOLDER_KEY;

/// 某个 live 配置的关键快照。
pub struct LiveSnapshot {
    pub base_url: Option<String>,
    pub key_is_placeholder: bool,
}

/// base_url 是否指向本机回环地址。
pub fn is_localhost(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.contains("127.0.0.1") || u.contains("localhost") || u.contains("[::1]")
}

/// 读取 Claude live（~/.claude/settings.json）的 base_url 与是否占位密钥。
pub fn read_claude() -> LiveSnapshot {
    let env = fs::read_to_string(config::get_claude_settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("env").cloned());
    let base_url = env
        .as_ref()
        .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let key_is_placeholder = env
        .as_ref()
        .map(|e| {
            ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"].iter().any(|k| {
                e.get(*k)
                    .and_then(|v| v.as_str())
                    .map(|s| s == PLACEHOLDER_KEY)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    LiveSnapshot {
        base_url,
        key_is_placeholder,
    }
}

/// 读取 Codex live（~/.codex/config.toml + auth.json）的 base_url 与是否占位密钥。
pub fn read_codex() -> LiveSnapshot {
    let base_url = fs::read_to_string(config::get_codex_config_path())
        .ok()
        .and_then(|cfg| {
            cfg.lines()
                .find_map(|l| l.trim().strip_prefix("base_url"))
                .and_then(|r| r.split('"').nth(1))
                .map(|s| s.to_string())
        });
    let key_is_placeholder = fs::read_to_string(config::get_codex_auth_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.get("OPENAI_API_KEY")
                .and_then(|k| k.as_str())
                .map(|s| s == PLACEHOLDER_KEY)
        })
        .unwrap_or(false);
    LiveSnapshot {
        base_url,
        key_is_placeholder,
    }
}
