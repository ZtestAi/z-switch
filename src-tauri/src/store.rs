//! providers.json 数据模型 + 读写。
//! 只存耐久配置；测速/验真等 runtime 状态不进此文件（见 design/数据结构.md）。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::config;

pub const CLAUDE_OFFICIAL_PROVIDER_ID: &str = "claude-official";
pub const CODEX_OFFICIAL_PROVIDER_ID: &str = "codex-official";

pub fn official_provider_id(app: &str) -> Option<&'static str> {
    match app {
        "claude" => Some(CLAUDE_OFFICIAL_PROVIDER_ID),
        "codex" => Some(CODEX_OFFICIAL_PROVIDER_ID),
        _ => None,
    }
}

pub fn is_official_provider(provider: &Provider) -> bool {
    matches!(
        provider.id.as_str(),
        CLAUDE_OFFICIAL_PROVIDER_ID | CODEX_OFFICIAL_PROVIDER_ID
    )
}

fn official_provider(app: &str) -> Provider {
    match app {
        "claude" => Provider {
            id: CLAUDE_OFFICIAL_PROVIDER_ID.into(),
            name: "Claude 官方账号".into(),
            category: Some("official".into()),
            settings_config: json!({ "env": {} }),
            meta: json!({
                "kind": "officialLocal",
                "system": true,
                "iconColor": "#D4915D"
            }),
            failover: json!({ "enabled": false }),
        },
        "codex" => Provider {
            id: CODEX_OFFICIAL_PROVIDER_ID.into(),
            name: "OpenAI 官方账号".into(),
            category: Some("official".into()),
            settings_config: json!({ "auth": {}, "config": "" }),
            meta: json!({
                "kind": "officialLocal",
                "system": true,
                "iconColor": "#10A37F",
                "wireApi": "responses"
            }),
            failover: json!({ "enabled": false }),
        },
        _ => unreachable!("unsupported official provider app"),
    }
}

/// 单个供应商。`settings_config` 是唯一按 app 类型分叉的字段：
/// Claude = `{ env: {...} }`；Codex = `{ auth: {...}, config: "toml" }`。
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    /// 原样写进 live 配置文件的内容
    pub settings_config: Value,
    /// 图标 / apiKeyField / wireApi 等元数据（不写 live）
    #[serde(default)]
    pub meta: Value,
    /// 故障转移偏好（可靠性层 V1.5 预留）
    #[serde(default)]
    pub failover: Value,
}

/// 单个工具（claude / codex）的数据。
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
    /// 当前激活的 provider id（激活项不可删）
    #[serde(default)]
    pub current: Option<String>,
    /// 手动拖拽排序
    #[serde(default)]
    pub order: Vec<String>,
    /// id -> provider
    #[serde(default)]
    pub providers: HashMap<String, Provider>,
}

/// providers.json 根结构。
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub version: u32,
    /// "claude" / "codex"
    pub apps: HashMap<String, AppData>,
    /// 全局设置（主题 / 备份 / 可靠性层 / 验真预留）
    #[serde(default)]
    pub settings: Value,
}

impl Root {
    pub fn default_seeded() -> Self {
        let mut apps: HashMap<String, AppData> = HashMap::new();
        for app in ["claude", "codex"] {
            let provider = official_provider(app);
            let id = provider.id.clone();
            apps.insert(
                app.into(),
                AppData {
                    current: Some(id.clone()),
                    order: vec![id.clone()],
                    providers: HashMap::from([(id, provider)]),
                },
            );
        }
        Root {
            version: 3,
            apps,
            settings: json!({
                "theme": "light",
                "autoLaunch": false,
                "backupBeforeWrite": true,
                "initialImportDone": false,
                "applyClaudePlugin": false,
                "skipClaudeOnboarding": false,
                "reliability": {
                    "proxyEnabled": false,
                    "failoverEnabled": false,
                    "circuitBreaker": true,
                    "connectTimeoutSeconds": 10,
                    "streamingFirstByteTimeoutSeconds": 60,
                    "streamingIdleTimeoutSeconds": 120,
                    "nonStreamingTimeoutSeconds": 600,
                    "requestBodyLimitMb": 64,
                    "poolMaxIdlePerHost": 10,
                    "tcpKeepaliveSeconds": 60,
                    "proxyErrorLogEnabled": true,
                    "proxyErrorLogMaxMb": 5
                },
                "ztest": { "connected": false }
            }),
        }
    }

    /// 补齐不可删除的本机官方账号卡片，并清理旧版本可能写入供应商数据的
    /// Codex OAuth 字段。官方凭据始终由 Codex 自己的 auth.json/keyring 持有。
    pub fn ensure_official_providers(&mut self) -> bool {
        let mut changed = false;
        if self.version < 3 {
            self.version = 3;
            changed = true;
        }

        for app in ["claude", "codex"] {
            let seed = official_provider(app);
            let official_id = seed.id.clone();
            let data = self.apps.entry(app.into()).or_default();

            match data.providers.get_mut(&official_id) {
                Some(existing) => {
                    if existing.name != seed.name {
                        existing.name = seed.name.clone();
                        changed = true;
                    }
                    if existing.category.as_deref() != Some("official") {
                        existing.category = Some("official".into());
                        changed = true;
                    }
                    let config = existing.settings_config.clone();
                    let normalized = if app == "claude" {
                        json!({ "env": config.get("env").cloned().unwrap_or_else(|| json!({})) })
                    } else {
                        json!({
                            "auth": {},
                            "config": config.get("config").and_then(Value::as_str).unwrap_or("")
                        })
                    };
                    if existing.settings_config != normalized {
                        existing.settings_config = normalized;
                        changed = true;
                    }
                    if existing.meta != seed.meta {
                        existing.meta = seed.meta.clone();
                        changed = true;
                    }
                    if existing.failover != seed.failover {
                        existing.failover = seed.failover.clone();
                        changed = true;
                    }
                }
                None => {
                    data.providers.insert(official_id.clone(), seed);
                    changed = true;
                }
            }

            if !data.order.contains(&official_id) {
                data.order.insert(0, official_id.clone());
                changed = true;
            }
            data.order.retain(|id| data.providers.contains_key(id));
            if data
                .current
                .as_ref()
                .is_none_or(|id| !data.providers.contains_key(id))
            {
                data.current = Some(official_id);
                changed = true;
            }
        }

        // 第三方 Codex provider 只应保存自己的 API Key；历史版本若把整个
        // auth.json 回填进 providers.json，在这里移除 OAuth/账号材料。
        if let Some(data) = self.apps.get_mut("codex") {
            for provider in data.providers.values_mut() {
                if is_official_provider(provider) {
                    continue;
                }
                let Some(root) = provider.settings_config.as_object_mut() else {
                    continue;
                };
                let key = root
                    .get("auth")
                    .and_then(Value::as_object)
                    .and_then(|auth| auth.get("OPENAI_API_KEY"))
                    .cloned();
                let sanitized = match key {
                    Some(key) => json!({ "OPENAI_API_KEY": key }),
                    None => json!({}),
                };
                if root.get("auth") != Some(&sanitized) {
                    root.insert("auth".into(), sanitized);
                    changed = true;
                }
            }
        }

        changed
    }

    pub fn has_non_official_provider(&self) -> bool {
        self.apps.values().any(|data| {
            data.providers
                .values()
                .any(|provider| !is_official_provider(provider))
        })
    }
}

/// 载入 providers.json；不存在则创建空列表，首次导入由启动流程处理。
pub fn load() -> Root {
    let path = config::get_store_path();
    if path.exists() {
        match config::read_json_file::<Root>(&path) {
            Ok(root) => return root,
            Err(e) => log_warn(&format!("providers.json 解析失败，改用空数据: {e}")),
        }
    }
    let root = Root::default_seeded();
    let _ = save(&root);
    root
}

/// 原子保存 providers.json
pub fn save(root: &Root) -> Result<(), String> {
    config::write_json_file(&config::get_store_path(), root)
}

fn log_warn(msg: &str) {
    eprintln!("[z-switch] {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_contains_active_official_cards() {
        let root = Root::default_seeded();
        for (app, id) in [
            ("claude", CLAUDE_OFFICIAL_PROVIDER_ID),
            ("codex", CODEX_OFFICIAL_PROVIDER_ID),
        ] {
            let data = &root.apps[app];
            assert_eq!(data.current.as_deref(), Some(id));
            assert_eq!(data.order.first().map(String::as_str), Some(id));
            assert!(is_official_provider(&data.providers[id]));
        }
    }

    #[test]
    fn migration_keeps_oauth_material_out_of_provider_rows() {
        let mut root = Root::default_seeded();
        root.apps.get_mut("codex").unwrap().providers.insert(
            "relay".into(),
            Provider {
                id: "relay".into(),
                name: "Relay".into(),
                category: Some("custom".into()),
                settings_config: json!({
                    "auth": {
                        "OPENAI_API_KEY": "relay-key",
                        "tokens": { "access_token": "must-not-persist" }
                    },
                    "config": ""
                }),
                meta: json!({}),
                failover: json!({}),
            },
        );

        assert!(root.ensure_official_providers());
        assert_eq!(
            auth_of(&root, "relay"),
            json!({ "OPENAI_API_KEY": "relay-key" })
        );
        assert_eq!(auth_of(&root, CODEX_OFFICIAL_PROVIDER_ID), json!({}));
    }

    fn auth_of(root: &Root, id: &str) -> Value {
        root.apps["codex"].providers[id].settings_config["auth"].clone()
    }
}
