//! 阶段 2：切换时写 live 配置。移植 cc-switch 的安全机制。
//! - Claude：把 env 合并进 ~/.claude/settings.json，保留用户其它顶层字段。
//! - Codex：先写 auth.json 再写 config.toml，config 失败则回滚 auth（双文件原子）。
//! - 写前备份到 ~/.z-switch/backups/。
//! - 切换前 backfill：把当前 live 配置回写给旧 provider，避免用户手改丢失。
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

use crate::config;
use crate::store::{self, Provider};

const CLAUDE_RELAY_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
];

/// 读取 JSON 文件为对象。
/// - 文件不存在 / 空文件 → 空对象（正常首次写入）。
/// - 存在但**解析失败或非对象** → 返回 Err（B1：中止写入，绝不覆盖，防止丢失用户其它字段）。
fn read_obj(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read_to_string(path) {
        Ok(s) => {
            if s.trim().is_empty() {
                return Ok(Map::new());
            }
            let v: Value = serde_json::from_str(&s).map_err(|e| {
                format!(
                    "现有文件 {} 不是合法 JSON，已中止写入以防丢失你的其它配置：{e}",
                    path.display()
                )
            })?;
            match v {
                Value::Object(m) => Ok(m),
                _ => Err(format!(
                    "现有文件 {} 不是 JSON 对象，已中止写入以防覆盖",
                    path.display()
                )),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("读取 {} 失败: {e}", path.display())),
    }
}

/// 写前备份：把现有文件复制到 ~/.z-switch/backups/{tag}-{纳秒}.bak
fn backup_file(path: &Path, tag: &str) {
    if !path.exists() {
        return;
    }
    let dir = config::get_app_config_dir().join("backups");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dest = dir.join(format!("{tag}-{ts}.bak"));
    let _ = fs::copy(path, dest);
}

/// 在恢复原始配置或解除 z-switch 管理前，保留当前 live 文件。
pub fn backup_current_app(app: &str) {
    match app {
        "claude" => backup_file(&config::get_claude_settings_path(), "claude-settings"),
        "codex" => {
            backup_file(&config::get_codex_auth_path(), "codex-auth");
            backup_file(&config::get_codex_config_path(), "codex-config");
        }
        "grok" => backup_file(&config::get_grok_config_path(), "grok-config"),
        _ => {}
    }
}

// ---------- Claude ----------

/// 读取当前 live settings.json 的 env（backfill 用，best-effort：解析失败则跳过不报错）
fn read_claude_live_env() -> Option<Value> {
    let path = config::get_claude_settings_path();
    read_obj(&path).ok().and_then(|o| o.get("env").cloned())
}

fn sanitize_claude_official_env(env: &Value) -> Value {
    let mut object = env.as_object().cloned().unwrap_or_default();
    for key in CLAUDE_RELAY_ENV_KEYS {
        object.remove(*key);
    }
    Value::Object(object)
}

/// 把 provider 的 env 写进 settings.json，保留其它顶层字段。
/// 若现有文件无法解析，read_obj 会返回 Err → 中止，绝不覆盖。
fn write_claude_live(env: &Value, backup: bool) -> Result<(), String> {
    let path = config::get_claude_settings_path();
    let mut settings = read_obj(&path)?;
    if backup {
        backup_file(&path, "claude-settings");
    }
    settings.insert("env".into(), env.clone());
    config::write_json_file(&path, &Value::Object(settings))
}

/// 写入干净的官方账号基线，**即使现有文件已损坏也绝不失败**。
/// - 写前先备份现有文件（含非法 JSON / 损坏 toml）。
/// - Claude：能解析则只清掉中转 env、保留其它顶层字段；不能解析则硬重置为 `{"env":{}}`。
/// - Codex：整份覆盖写官方 auth 快照 + 无中转的 config（本来就不依赖解析旧文件）。
pub fn write_official_baseline(app: &str, backup: bool) -> Result<(), String> {
    match app {
        "claude" => {
            let path = config::get_claude_settings_path();
            if backup {
                backup_file(&path, "claude-settings");
            }
            match read_obj(&path) {
                Ok(mut settings) => {
                    let env = settings
                        .get("env")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new()));
                    settings.insert("env".into(), sanitize_claude_official_env(&env));
                    config::write_json_file(&path, &Value::Object(settings))
                }
                Err(_) => {
                    // 损坏文件：硬重置为最小合法官方配置（坏文件已备份）。
                    config::write_json_file(&path, &serde_json::json!({ "env": {} }))
                }
            }
        }
        "codex" => {
            let auth = crate::official::codex_auth_for_restore()?;
            // 空 config = 官方默认（无 model_provider / base_url）。
            write_codex_live(&auth, "", backup)
        }
        other => Err(format!("未知应用: {other}")),
    }
}

// ---------- Codex ----------

/// 读取当前 live 的 auth.json + config.toml（backfill 用）
fn read_codex_live() -> (Option<Value>, Option<String>) {
    let auth_path = config::get_codex_auth_path();
    let cfg_path = config::get_codex_config_path();
    let auth = fs::read_to_string(&auth_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let cfg = fs::read_to_string(&cfg_path).ok();
    (auth, cfg)
}

/// 双文件原子写：先 auth.json 后 config.toml，config 失败回滚 auth。
fn write_codex_live(auth: &Value, config_text: &str, backup: bool) -> Result<(), String> {
    let auth_path = config::get_codex_auth_path();
    let cfg_path = config::get_codex_config_path();
    if backup {
        backup_file(&auth_path, "codex-auth");
        backup_file(&cfg_path, "codex-config");
    }

    let old_auth = fs::read(&auth_path).ok();
    config::write_json_file(&auth_path, auth)?;

    if let Err(e) = config::write_text_file(&cfg_path, config_text) {
        // 回滚 auth.json 到写入前
        match old_auth {
            Some(bytes) => {
                let _ = config::atomic_write(&auth_path, &bytes);
            }
            None => {
                let _ = fs::remove_file(&auth_path);
            }
        }
        return Err(format!("写入 config.toml 失败，已回滚 auth.json：{e}"));
    }
    Ok(())
}

/// 移除当前第三方 model_provider 及其配置表，保留 MCP、沙箱、历史等公共配置。
/// 官方账号使用 Codex 内置 OpenAI provider，不应该携带中转 base_url。
fn sanitize_codex_official_config(config_text: &str) -> String {
    fn model_provider_value(line: &str) -> Option<&str> {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            return None;
        }
        let (key, value) = trimmed.split_once('=')?;
        (key.trim() == "model_provider").then_some(value.trim())
    }

    let provider_id = config_text
        .lines()
        .find_map(model_provider_value)
        .map(|value| value.trim_matches(['\"', '\'']).to_string())
        .filter(|value| !value.is_empty() && value != "openai");

    let mut result = Vec::new();
    let mut skip_provider_table = false;
    for line in config_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed[1..trimmed.len() - 1].replace(['\"', '\''], "");
            skip_provider_table = provider_id.as_ref().is_some_and(|id| {
                let provider_section = format!("model_providers.{id}");
                section == provider_section || section.starts_with(&(provider_section + "."))
            });
            if skip_provider_table {
                continue;
            }
        }
        if skip_provider_table {
            continue;
        }
        if model_provider_value(trimmed).is_some() {
            continue;
        }
        result.push(line);
    }

    while result.last().is_some_and(|line| line.trim().is_empty()) {
        result.pop();
    }
    if result.is_empty() {
        String::new()
    } else {
        result.join("\n") + "\n"
    }
}

/// 官方卡片首次出现时，从 live 中提取非敏感、非中转的公共配置。
/// 只填充空白种子，避免每次启动时被当前中转配置覆盖。
pub fn hydrate_official_provider(app: &str, provider: &mut Provider) -> bool {
    if !store::is_official_provider(provider) {
        return false;
    }
    match app {
        "claude" => {
            let current = provider
                .settings_config
                .get("env")
                .and_then(Value::as_object);
            if current.is_some_and(|env| !env.is_empty()) {
                return false;
            }
            let Some(live) = read_claude_live_env() else {
                return false;
            };
            let sanitized = sanitize_claude_official_env(&live);
            if sanitized.as_object().is_none_or(|env| env.is_empty()) {
                return false;
            }
            provider.settings_config = serde_json::json!({ "env": sanitized });
            true
        }
        "codex" => {
            let current = provider
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !current.trim().is_empty() {
                return false;
            }
            let (_, live_config) = read_codex_live();
            let sanitized = sanitize_codex_official_config(live_config.as_deref().unwrap_or(""));
            if sanitized.trim().is_empty() {
                return false;
            }
            provider.settings_config = serde_json::json!({ "auth": {}, "config": sanitized });
            true
        }
        _ => false,
    }
}

// ---------- Grok ----------

/// 读取当前 live 的 ~/.grok/config.toml 文本（backfill / 导入用）。
fn read_grok_live() -> Option<String> {
    fs::read_to_string(config::get_grok_config_path()).ok()
}

/// 单文件写：整份 config.toml 原样落盘（写前备份）。
/// Grok 无「官方账号」概念，不做 sanitize；停止管理走 original 快照恢复。
fn write_grok_live(config_text: &str, backup: bool) -> Result<(), String> {
    let path = config::get_grok_config_path();
    if backup {
        backup_file(&path, "grok-config");
    }
    config::write_text_file(&path, config_text)
}

// ---------- 对外统一入口 ----------

/// 把目标 provider 写进 live 配置。
pub fn write_live(app: &str, provider: &Provider, backup: bool) -> Result<(), String> {
    let official = store::is_official_provider(provider);
    match app {
        "claude" => {
            let mut env = provider
                .settings_config
                .get("env")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            if official {
                env = sanitize_claude_official_env(&env);
            }
            write_claude_live(&env, backup)
        }
        "codex" => {
            let auth = if official {
                crate::official::codex_auth_for_restore()?
            } else {
                provider
                    .settings_config
                    .get("auth")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Map::new()))
            };
            let mut cfg = provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if official {
                cfg = sanitize_codex_official_config(&cfg);
            }
            write_codex_live(&auth, &cfg, backup)
        }
        "grok" => {
            let cfg = provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            write_grok_live(cfg, backup)
        }
        other => Err(format!("未知应用: {other}")),
    }
}

/// 切换前 backfill：把当前 live 配置回写进旧 provider 的 settings_config，
/// 避免用户在配置文件里的手改在切换时丢失。
pub fn backfill(app: &str, provider: &mut Provider) {
    let official = store::is_official_provider(provider);
    let obj = match provider.settings_config.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    match app {
        "claude" => {
            if let Some(env) = read_claude_live_env() {
                obj.insert(
                    "env".into(),
                    if official {
                        sanitize_claude_official_env(&env)
                    } else {
                        env
                    },
                );
            }
        }
        "codex" => {
            let (auth, cfg) = read_codex_live();
            if official {
                if let Err(error) = crate::official::capture_codex_current() {
                    eprintln!("[z-switch] 保存 Codex 官方登录态失败：{error}");
                }
                obj.insert("auth".into(), serde_json::json!({}));
            } else if let Some(auth) = auth {
                let key = auth
                    .get("OPENAI_API_KEY")
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
                obj.insert("auth".into(), serde_json::json!({ "OPENAI_API_KEY": key }));
            }
            if let Some(cfg) = cfg {
                obj.insert(
                    "config".into(),
                    Value::String(if official {
                        sanitize_codex_official_config(&cfg)
                    } else {
                        cfg
                    }),
                );
            }
        }
        "grok" => {
            if let Some(cfg) = read_grok_live() {
                obj.insert("config".into(), Value::String(cfg));
            }
        }
        _ => {}
    }
}

// ---------- 首次导入：从现有 live 配置反向生成 Provider ----------

/// 从 config.toml 文本里取 `base_url = "..."` 的主机名，做个可读的名字回退。
fn host_of(url: &str) -> Option<String> {
    let s = url.split("://").last()?;
    let host = s.split('/').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// 读取当前 `~/.claude/settings.json` 的 env，若存在有效 base_url 则生成一个 Provider。
/// 返回 None 表示无可导入内容（文件缺失/无 env/无 base_url）。
pub fn import_claude() -> Option<Provider> {
    let env = read_claude_live_env()?;
    let env_obj = env.as_object()?;
    let base = env_obj.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str());
    // 没有 base_url 视为无意义（可能是官方直连或空配置），不导入
    let base = base?;
    if base.trim().is_empty() {
        return None;
    }
    let key_field = if env_obj.contains_key("ANTHROPIC_API_KEY") {
        "ANTHROPIC_API_KEY"
    } else {
        "ANTHROPIC_AUTH_TOKEN"
    };
    let name = host_of(base).unwrap_or_else(|| "导入的 Claude 供应商".to_string());
    Some(Provider {
        id: "imported-current".to_string(),
        name,
        category: Some("imported".into()),
        settings_config: serde_json::json!({ "env": env.clone() }),
        meta: serde_json::json!({ "apiKeyField": key_field, "imported": true }),
        failover: serde_json::json!({ "enabled": false }),
    })
}

/// 读取当前 `~/.codex/{auth.json,config.toml}`，若存在有效 config 则生成一个 Provider。
pub fn import_codex() -> Option<Provider> {
    let (auth, cfg) = read_codex_live();
    let cfg = cfg?;
    if cfg.trim().is_empty() {
        return None;
    }
    // 从 config.toml 抓 base_url / wire_api 做元数据与命名（best-effort 正则式扫描）
    let base = cfg
        .lines()
        .find_map(|l| l.trim().strip_prefix("base_url"))
        .and_then(|r| r.split('"').nth(1))
        .map(|s| s.to_string());
    // 没有第三方 base_url 说明当前是 Codex 官方登录/默认配置，交给内置
    // 官方账号卡片管理，绝不能把 OAuth auth.json 导入普通供应商。
    let base = base.filter(|value| !value.trim().is_empty())?;
    let wire = cfg
        .lines()
        .find_map(|l| l.trim().strip_prefix("wire_api"))
        .and_then(|r| r.split('"').nth(1))
        .unwrap_or("responses")
        .to_string();
    let name = host_of(&base).unwrap_or_else(|| "导入的 Codex 供应商".to_string());
    let key = auth
        .as_ref()
        .and_then(|value| value.get("OPENAI_API_KEY"))
        .cloned()
        .unwrap_or(Value::String(String::new()));
    Some(Provider {
        id: "imported-current".to_string(),
        name,
        category: Some("imported".into()),
        settings_config: serde_json::json!({
            "auth": { "OPENAI_API_KEY": key },
            "config": cfg
        }),
        meta: serde_json::json!({ "wireApi": wire, "imported": true }),
        failover: serde_json::json!({ "enabled": false }),
    })
}

/// 读取当前 `~/.grok/config.toml`，若存在有效 models_base_url 则生成一个 Provider。
/// 返回 None 表示无可导入内容（文件缺失 / 无 base_url）。
pub fn import_grok() -> Option<Provider> {
    let cfg = read_grok_live()?;
    if cfg.trim().is_empty() {
        return None;
    }
    // 从 [endpoints] 抓 models_base_url 做命名；无 base_url 视为无意义配置。
    let base = cfg
        .lines()
        .find_map(|l| l.trim().strip_prefix("models_base_url"))
        .and_then(|r| r.split('"').nth(1))
        .map(|s| s.to_string())
        .filter(|value| !value.trim().is_empty())?;
    let name = host_of(&base).unwrap_or_else(|| "导入的 Grok 供应商".to_string());
    Some(Provider {
        id: "imported-current".to_string(),
        name,
        category: Some("imported".into()),
        settings_config: serde_json::json!({ "config": cfg }),
        meta: serde_json::json!({ "imported": true }),
        failover: serde_json::json!({ "enabled": false }),
    })
}

#[cfg(test)]
mod official_config_tests {
    use super::*;

    #[test]
    fn official_codex_config_removes_relay_table_and_keeps_common_sections() {
        let input = r#"model_provider = "custom"
model = "gpt-5"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"

[mcp_servers.docs]
command = "docs"
"#;
        let output = sanitize_codex_official_config(input);
        assert!(!output.contains("model_provider ="));
        assert!(!output.contains("relay.example"));
        assert!(output.contains("model = \"gpt-5\""));
        assert!(output.contains("[mcp_servers.docs]"));
    }

    #[test]
    fn official_claude_env_only_removes_relay_owned_fields() {
        let output = sanitize_claude_official_env(&serde_json::json!({
            "ANTHROPIC_BASE_URL": "https://relay.example",
            "ANTHROPIC_AUTH_TOKEN": "secret",
            "API_TIMEOUT_MS": "600000"
        }));
        assert!(output.get("ANTHROPIC_BASE_URL").is_none());
        assert!(output.get("ANTHROPIC_AUTH_TOKEN").is_none());
        assert_eq!(output["API_TIMEOUT_MS"], "600000");
    }
}

#[cfg(test)]
mod official_baseline_tests {
    use super::*;
    use std::sync::MutexGuard;

    struct TestHome {
        path: std::path::PathBuf,
        previous: Option<std::ffi::OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl TestHome {
        fn new() -> Self {
            let lock = config::TEST_HOME_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let path = std::env::temp_dir().join(format!(
                "z-switch-official-baseline-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            let previous = std::env::var_os("Z_SWITCH_TEST_HOME");
            std::env::set_var("Z_SWITCH_TEST_HOME", &path);
            Self {
                path,
                previous,
                _lock: lock,
            }
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
    fn official_baseline_rewrites_corrupt_claude_settings() {
        let _home = TestHome::new();
        let path = config::get_claude_settings_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{not-json").unwrap();

        write_official_baseline("claude", true).unwrap();

        let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(restored.get("env").and_then(Value::as_object).is_some());
        assert!(restored["env"].get("ANTHROPIC_BASE_URL").is_none());
    }

    #[test]
    fn official_baseline_keeps_non_relay_fields_when_claude_parseable() {
        let _home = TestHome::new();
        let path = config::get_claude_settings_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            br#"{"env":{"ANTHROPIC_BASE_URL":"https://relay","API_TIMEOUT_MS":"1"},"keep":true}"#,
        )
        .unwrap();

        write_official_baseline("claude", false).unwrap();

        let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(restored["keep"], true);
        assert!(restored["env"].get("ANTHROPIC_BASE_URL").is_none());
        assert_eq!(restored["env"]["API_TIMEOUT_MS"], "1");
    }

    #[test]
    fn grok_write_live_and_import_round_trip() {
        let _home = TestHome::new();
        let toml = "[endpoints]\nmodels_base_url = \"https://relay.example/v1\"\n\n\
                    [model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\napi_key = \"sk-test\"\n";
        let provider = Provider {
            id: "relay".into(),
            name: "Relay".into(),
            category: Some("custom".into()),
            settings_config: serde_json::json!({ "config": toml }),
            meta: serde_json::json!({}),
            failover: serde_json::json!({}),
        };

        // 写 live → ~/.grok/config.toml 内容与 settings_config.config 一致。
        write_live("grok", &provider, false).unwrap();
        let on_disk = fs::read_to_string(config::get_grok_config_path()).unwrap();
        assert_eq!(on_disk, toml);

        // 反向导入 → 从 models_base_url 生成一张可用的候选卡。
        let imported = import_grok().expect("应能从现有 grok 配置导入");
        assert_eq!(
            imported.settings_config.get("config").and_then(Value::as_str),
            Some(toml)
        );
        assert_eq!(imported.name, "relay.example");
    }

    #[test]
    fn grok_import_skips_config_without_base_url() {
        let _home = TestHome::new();
        let path = config::get_grok_config_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 无 models_base_url = 无意义配置，不应导入。
        fs::write(&path, b"[models]\ndefault = \"grok-4.5\"\n").unwrap();
        assert!(import_grok().is_none());
    }
}
