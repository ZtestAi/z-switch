//! providers.json 数据模型 + 读写。
//! 只存耐久配置；测速等 runtime 状态不进此文件。
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
    /// 全局设置（主题 / 备份 / 可靠性层）
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
        // Grok 无本机官方登录态，纯第三方：初始为空列表，无当前项。
        apps.insert("grok".into(), AppData::default());
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
                "applyClaudeDesktop": false,
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
                }
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

        // 迁移：老装机没有 grok 分区，补一个空列表（无官方卡）。
        if !self.apps.contains_key("grok") {
            self.apps.insert("grok".into(), AppData::default());
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
                    // 官方卡片 meta 以种子为准，但保留用户设置的网络代理。
                    let mut expected_meta = seed.meta.clone();
                    if let (Some(proxy), Some(meta)) = (
                        existing.meta.get("proxy").cloned(),
                        expected_meta.as_object_mut(),
                    ) {
                        meta.insert("proxy".into(), proxy);
                    }
                    if existing.meta != expected_meta {
                        existing.meta = expected_meta;
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
                if migrate_codex_provider_wire_api(provider) {
                    changed = true;
                }
            }
        }

        // 清理旧版本写入的 ztest.ai 验真预留字段（功能已取消）。
        if let Some(settings) = self.settings.as_object_mut() {
            if settings.remove("ztest").is_some() {
                changed = true;
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

    /// 导入冲突：本机各客户端正在使用的中转站，在导入数据（`self`）里有同 id 但内容不同的卡片。
    /// 由用户逐个选择保留本机版本还是改用导入版本。
    pub fn import_conflicts(&self, local: &Root) -> Vec<ImportConflict> {
        let mut conflicts: Vec<ImportConflict> = local
            .apps
            .keys()
            .filter_map(|app| {
                let active = local_active_relay(local, app)?;
                let other = self.apps.get(app)?.providers.get(&active.id)?;
                (other.settings_config != active.settings_config).then(|| ImportConflict {
                    app: app.clone(),
                    id: active.id.clone(),
                    local_name: active.name.clone(),
                    imported_name: other.name.clone(),
                    local: endpoint_summary(app, active),
                    imported: endpoint_summary(app, other),
                })
            })
            .collect();
        conflicts.sort_by(|a, b| a.app.cmp(&b.app));
        conflicts
    }

    /// 导入整份配置前，把 `self`（导入数据）与本机状态对齐。
    ///
    /// 「当前项」必须始终描述本机 live：若沿用导入数据里的 current，下次切换会把本机 live
    /// 回填进那张卡片，冲掉它的地址与密钥。
    /// - 本机各客户端正在使用的卡片保留并继续作为当前项；`use_imported` 中的冲突（按客户端 +
    ///   卡片 id 匹配，期间本机已切到别的卡片则不生效）改用导入版本作为当前项（记入 `adopted`，
    ///   调用方必须随后写入客户端配置）；
    /// - 本机无当前项（如 Grok 未接管）时，导入后也无当前项；
    /// - 与本机文件 / 运行状态绑定的设置保留本机值（本地路由开关与端口、开机自启、Claude 联动开关、
    ///   全局网络代理、首次导入标记）。
    pub fn align_import_with_local(
        &mut self,
        local: &Root,
        use_imported: &[ImportChoice],
    ) -> ImportAlignment {
        let mut kept = Vec::new();
        let mut adopted = Vec::new();
        let apps: std::collections::HashSet<String> =
            self.apps.keys().chain(local.apps.keys()).cloned().collect();
        for app in apps {
            let chosen = use_imported.iter().any(|choice| {
                choice.app == app
                    && local_active_relay(local, &app).is_some_and(|active| active.id == choice.id)
            });
            match chosen
                .then(|| self.adopt_imported_active(&app, local))
                .flatten()
            {
                Some(adoption) => adopted.push(adoption),
                None => kept.extend(self.keep_local_active(&app, local)),
            }
        }

        if !self.settings.is_object() {
            self.settings = json!({});
        }
        let settings = self.settings.as_object_mut().unwrap();
        for key in [
            "autoLaunch",
            "applyClaudePlugin",
            "applyClaudeDesktop",
            "skipClaudeOnboarding",
            "networkProxy",
            "initialImportDone",
            "grokImportDone",
        ] {
            match local.settings.get(key) {
                Some(value) => settings.insert(key.into(), value.clone()),
                None => settings.remove(key),
            };
        }
        let reliability = settings.entry("reliability").or_insert_with(|| json!({}));
        if !reliability.is_object() {
            *reliability = json!({});
        }
        let reliability = reliability.as_object_mut().unwrap();
        for key in ["proxyEnabled", "proxyApps", "proxyPort"] {
            match local.settings.get("reliability").and_then(|r| r.get(key)) {
                Some(value) => reliability.insert(key.into(), value.clone()),
                None => reliability.remove(key),
            };
        }
        kept.sort();
        adopted.sort_by(|a, b| a.app.cmp(&b.app));
        ImportAlignment { kept, adopted }
    }

    /// 存在导入冲突时，改用导入数据里的同 id 卡片作为当前项。本机那张不是空卡片时，
    /// 改用新 id 一并保留（防止用户选错后无从找回）。
    fn adopt_imported_active(&mut self, app: &str, local: &Root) -> Option<Adoption> {
        let active = local_active_relay(local, app)?;
        let data = self.apps.get_mut(app)?;
        let other = data.providers.get(&active.id)?;
        if other.settings_config == active.settings_config {
            return None;
        }
        let local_copy = (!endpoint_summary(app, active).empty).then(|| {
            let mut copy = active.clone();
            copy.id = free_id(data, &active.id);
            data.order.push(copy.id.clone());
            data.providers.insert(copy.id.clone(), copy.clone());
            copy.id
        });
        data.current = Some(active.id.clone());
        Some(Adoption {
            app: app.to_string(),
            local_copy,
        })
    }

    /// 导入版本写入客户端配置失败：撤销 `adopt_imported_active`（移除本机副本），回退为保留本机版本。
    pub fn revert_adoption(&mut self, adoption: &Adoption, local: &Root) -> Option<String> {
        if let (Some(copy), Some(data)) = (&adoption.local_copy, self.apps.get_mut(&adoption.app)) {
            data.providers.remove(copy);
            data.order.retain(|entry| entry != copy);
        }
        self.keep_local_active(&adoption.app, local)
    }

    /// 把本机某客户端正在使用的卡片原样放回导入数据并设为当前项，返回保留的中转站名称
    /// （官方账号或无当前项时返回 None）。
    pub fn keep_local_active(&mut self, app: &str, local: &Root) -> Option<String> {
        let active = local
            .apps
            .get(app)
            .and_then(|data| data.current.as_ref().and_then(|id| data.providers.get(id)));
        let data = self.apps.entry(app.to_string()).or_default();
        let Some(provider) = active else {
            data.current = None;
            return None;
        };
        let id = provider.id.clone();
        let official = is_official_provider(provider);
        if !official {
            // 同 id 但内容不同（如两台电脑首启各自导入的 imported-current）：
            // 导入文件里的那张改用新 id 保留，不能被本机卡片顶掉。
            let differs = data
                .providers
                .get(&id)
                .is_some_and(|other| other.settings_config != provider.settings_config);
            if differs {
                let new_id = free_id(data, &id);
                let mut other = data.providers.remove(&id).unwrap();
                other.id = new_id.clone();
                for entry in data.order.iter_mut().filter(|entry| **entry == id) {
                    *entry = new_id.clone();
                }
                if !data.order.contains(&new_id) {
                    data.order.push(new_id.clone());
                }
                data.providers.insert(new_id, other);
            }
        }
        data.providers.insert(id.clone(), provider.clone());
        if !data.order.contains(&id) {
            data.order.push(id.clone());
        }
        data.current = Some(id);
        (!official).then(|| provider.name.clone())
    }
}

/// 生成不与现有卡片冲突的新 id：`{id}-2`、`{id}-3`……
fn free_id(data: &AppData, id: &str) -> String {
    (2..)
        .map(|n| format!("{id}-{n}"))
        .find(|candidate| !data.providers.contains_key(candidate))
        .unwrap()
}

/// 用户在导入冲突中选择「使用导入版本」的卡片（客户端 + 本机当前卡片 id）。
#[derive(Deserialize, Clone)]
pub struct ImportChoice {
    pub app: String,
    pub id: String,
}

/// 改用导入版本的客户端；`local_copy` 为本机版本另存的新 id（本机那张为空卡片时不保留）。
pub struct Adoption {
    pub app: String,
    pub local_copy: Option<String>,
}

/// 导入对齐结果：`kept` 为保留本机版本的中转站名称，`adopted` 为改用导入版本的客户端。
pub struct ImportAlignment {
    pub kept: Vec<String>,
    pub adopted: Vec<Adoption>,
}

/// 导入冲突详情（供前端让用户选择）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportConflict {
    pub app: String,
    pub id: String,
    pub local_name: String,
    pub imported_name: String,
    pub local: EndpointSummary,
    pub imported: EndpointSummary,
}

/// 供应商接入点概要：地址 + 打码后的密钥；`empty` 表示缺地址或缺密钥（可能已损坏）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSummary {
    pub url: String,
    pub key: String,
    pub empty: bool,
}

/// 本机某客户端正在使用的中转站（官方账号卡片不算）。
fn local_active_relay<'a>(local: &'a Root, app: &str) -> Option<&'a Provider> {
    let data = local.apps.get(app)?;
    let provider = data.providers.get(data.current.as_ref()?)?;
    (!is_official_provider(provider)).then_some(provider)
}

/// 取 TOML 文本里第一处 `key = "value"` 的值（仅用于概要展示）。
fn toml_string(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix(key)?
                .trim_start()
                .strip_prefix('=')?
                .split('"')
                .nth(1)
        })
        .unwrap_or("")
        .trim()
        .to_string()
}

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    match chars.len() {
        0 => String::new(),
        1..=12 => "****".into(),
        n => format!(
            "{}…{}",
            chars[..4].iter().collect::<String>(),
            chars[n - 4..].iter().collect::<String>()
        ),
    }
}

pub fn endpoint_summary(app: &str, provider: &Provider) -> EndpointSummary {
    let config = &provider.settings_config;
    let text = config.get("config").and_then(Value::as_str).unwrap_or("");
    let string_at = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let (url, key) = match app {
        "claude" => {
            let env = config.get("env");
            let field = provider
                .meta
                .get("apiKeyField")
                .and_then(Value::as_str)
                .unwrap_or("ANTHROPIC_AUTH_TOKEN");
            let key = [field, "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]
                .iter()
                .map(|name| string_at(env.and_then(|env| env.get(*name))))
                .find(|key| !key.is_empty())
                .unwrap_or_default();
            (
                string_at(env.and_then(|env| env.get("ANTHROPIC_BASE_URL"))),
                key,
            )
        }
        "codex" => (
            toml_string(text, "base_url"),
            string_at(
                config
                    .get("auth")
                    .and_then(|auth| auth.get("OPENAI_API_KEY")),
            ),
        ),
        _ => (
            toml_string(text, "models_base_url"),
            toml_string(text, "api_key"),
        ),
    };
    EndpointSummary {
        empty: url.is_empty() || key.is_empty(),
        key: mask_key(&key),
        url,
    }
}

/// Codex 已移除 `wire_api = "chat"`：把供应商 config 与 meta 中的 chat 统一改为
/// responses。返回是否有改动。用于启动迁移、导入以及保存（JSON 模式可能手填 chat）。
pub fn migrate_codex_provider_wire_api(provider: &mut Provider) -> bool {
    let mut changed = false;
    if let Some(root) = provider.settings_config.as_object_mut() {
        let migrated = root
            .get("config")
            .and_then(Value::as_str)
            .and_then(crate::live::migrate_codex_wire_api);
        if let Some(config) = migrated {
            root.insert("config".into(), Value::String(config));
            changed = true;
        }
    }
    if let Some(meta) = provider.meta.as_object_mut() {
        if meta.get("wireApi").and_then(Value::as_str) == Some("chat") {
            meta.insert("wireApi".into(), json!("responses"));
            changed = true;
        }
    }
    changed
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
    fn official_card_keeps_network_proxy_but_resets_other_meta() {
        let mut root = Root::default_seeded();
        let proxy = json!({ "mode": "custom", "url": "http://127.0.0.1:7890" });
        {
            let card = root
                .apps
                .get_mut("codex")
                .unwrap()
                .providers
                .get_mut(CODEX_OFFICIAL_PROVIDER_ID)
                .unwrap();
            card.meta["proxy"] = proxy.clone();
            card.meta["tampered"] = json!(true);
        }
        root.ensure_official_providers();
        let card = &root.apps["codex"].providers[CODEX_OFFICIAL_PROVIDER_ID];
        assert_eq!(card.meta["proxy"], proxy);
        assert!(card.meta.get("tampered").is_none());
        assert!(is_official_provider(card));
        // 已规范化后再跑不应报告变更。
        assert!(!root.ensure_official_providers());
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

    #[test]
    fn migration_rewrites_codex_chat_wire_api_and_drops_ztest() {
        let mut root = Root::default_seeded();
        root.settings["ztest"] = json!({ "connected": false });
        root.apps.get_mut("codex").unwrap().providers.insert(
            "deepseek".into(),
            Provider {
                id: "deepseek".into(),
                name: "DeepSeek".into(),
                category: Some("custom".into()),
                settings_config: json!({
                    "auth": { "OPENAI_API_KEY": "k" },
                    "config": "[model_providers.custom]\nwire_api = \"chat\"\n"
                }),
                meta: json!({ "wireApi": "chat" }),
                failover: json!({}),
            },
        );

        assert!(root.ensure_official_providers());
        let provider = &root.apps["codex"].providers["deepseek"];
        assert_eq!(
            provider.settings_config["config"],
            "[model_providers.custom]\nwire_api = \"responses\"\n"
        );
        assert_eq!(provider.meta["wireApi"], "responses");
        assert!(root.settings.get("ztest").is_none());
        // 迁移是幂等的：再跑一次不应报告改动。
        assert!(!root.ensure_official_providers());
    }

    fn auth_of(root: &Root, id: &str) -> Value {
        root.apps["codex"].providers[id].settings_config["auth"].clone()
    }

    fn relay(id: &str, name: &str, url: &str) -> Provider {
        Provider {
            id: id.into(),
            name: name.into(),
            category: Some("custom".into()),
            settings_config: json!({
                "env": { "ANTHROPIC_BASE_URL": url, "ANTHROPIC_AUTH_TOKEN": "sk-x" }
            }),
            meta: json!({}),
            failover: json!({}),
        }
    }

    fn add(root: &mut Root, app: &str, provider: Provider, current: bool) {
        let data = root.apps.get_mut(app).unwrap();
        data.order.push(provider.id.clone());
        if current {
            data.current = Some(provider.id.clone());
        }
        data.providers.insert(provider.id.clone(), provider);
    }

    #[test]
    fn export_import_round_trip_keeps_relay_url_and_key() {
        let mut exported = Root::default_seeded();
        add(
            &mut exported,
            "claude",
            relay("a", "A", "https://a.example"),
            true,
        );
        let text = serde_json::to_string_pretty(&exported).unwrap();
        let mut incoming: Root = serde_json::from_str(&text).unwrap();
        incoming.ensure_official_providers();
        assert_eq!(
            incoming.apps["claude"].providers["a"].settings_config,
            exported.apps["claude"].providers["a"].settings_config
        );
    }

    #[test]
    fn import_keeps_local_current_instead_of_exported_current() {
        // 导出机正在用中转站 A；本机正在用官方账号。
        let mut incoming = Root::default_seeded();
        add(
            &mut incoming,
            "claude",
            relay("a", "A", "https://a.example"),
            true,
        );
        add(
            &mut incoming,
            "grok",
            relay("g", "G", "https://g.example"),
            true,
        );
        let local = Root::default_seeded();

        assert!(incoming
            .align_import_with_local(&local, &[])
            .kept
            .is_empty());
        // 当前项仍是本机官方账号：下次切换回填的是官方卡片，A 的地址与密钥不受影响。
        assert_eq!(
            incoming.apps["claude"].current.as_deref(),
            Some(CLAUDE_OFFICIAL_PROVIDER_ID)
        );
        assert_eq!(
            incoming.apps["claude"].providers["a"].settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://a.example"
        );
        // 本机 Grok 未接管：导入后也不应有当前项。
        assert_eq!(incoming.apps["grok"].current, None);
        assert!(incoming.apps["grok"].providers.contains_key("g"));
    }

    #[test]
    fn import_preserves_local_active_relay_and_machine_settings() {
        let mut local = Root::default_seeded();
        add(
            &mut local,
            "claude",
            relay("mine", "Mine", "https://mine.example"),
            true,
        );
        local.settings["reliability"]["proxyApps"] = json!({ "claude": true, "codex": false });
        local.settings["reliability"]["proxyPort"] = json!(9900);
        local.settings["applyClaudeDesktop"] = json!(true);
        local.settings["networkProxy"] = json!("http://127.0.0.1:7890");

        let mut incoming = Root::default_seeded();
        add(
            &mut incoming,
            "claude",
            relay("mine", "Old", "https://old.example"),
            false,
        );
        add(
            &mut incoming,
            "claude",
            relay("b", "B", "https://b.example"),
            true,
        );
        incoming.settings["theme"] = json!("dark");
        incoming.settings["networkProxy"] = json!("http://10.0.0.2:3128");
        incoming.settings["reliability"]["connectTimeoutSeconds"] = json!(30);

        assert_eq!(
            incoming.align_import_with_local(&local, &[]).kept,
            vec!["Mine".to_string()]
        );
        let data = &incoming.apps["claude"];
        assert_eq!(data.current.as_deref(), Some("mine"));
        // 正在使用的卡片以本机版本为准（与 live 一致）。
        assert_eq!(data.providers["mine"].name, "Mine");
        assert!(data.providers.contains_key("b"));
        assert_eq!(data.order.iter().filter(|id| *id == "mine").count(), 1);
        // 导入文件里同 id、内容不同的卡片改用新 id 保留，地址与密钥不丢。
        let renamed = &data.providers["mine-2"];
        assert_eq!(renamed.id, "mine-2");
        assert_eq!(renamed.name, "Old");
        assert_eq!(
            renamed.settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://old.example"
        );
        assert_eq!(
            data.order,
            vec![CLAUDE_OFFICIAL_PROVIDER_ID, "mine-2", "b", "mine"]
        );
        // 普通设置来自导入，本机状态相关的设置保留本机值。
        assert_eq!(incoming.settings["theme"], "dark");
        assert_eq!(
            incoming.settings["reliability"]["connectTimeoutSeconds"],
            30
        );
        assert_eq!(
            incoming.settings["reliability"]["proxyApps"]["claude"],
            true
        );
        assert_eq!(incoming.settings["reliability"]["proxyPort"], 9900);
        assert_eq!(incoming.settings["applyClaudeDesktop"], true);
        assert_eq!(incoming.settings["networkProxy"], "http://127.0.0.1:7890");
    }

    #[test]
    fn import_same_active_card_is_not_duplicated() {
        let mut local = Root::default_seeded();
        add(
            &mut local,
            "claude",
            relay("mine", "Mine", "https://mine.example"),
            true,
        );
        let mut incoming = Root::default_seeded();
        add(
            &mut incoming,
            "claude",
            relay("mine", "Renamed", "https://mine.example"),
            false,
        );
        add(
            &mut incoming,
            "claude",
            relay("b", "B", "https://b.example"),
            true,
        );
        incoming.align_import_with_local(&local, &[]);
        let data = &incoming.apps["claude"];
        assert_eq!(data.providers.len(), 3);
        assert_eq!(data.order, vec![CLAUDE_OFFICIAL_PROVIDER_ID, "mine", "b"]);
        assert_eq!(data.current.as_deref(), Some("mine"));
    }

    #[test]
    fn import_adds_local_active_relay_missing_from_file() {
        let mut local = Root::default_seeded();
        add(
            &mut local,
            "claude",
            relay("mine", "Mine", "https://mine.example"),
            true,
        );
        let mut incoming = Root::default_seeded();
        incoming.align_import_with_local(&local, &[]);
        let data = &incoming.apps["claude"];
        assert_eq!(data.current.as_deref(), Some("mine"));
        assert_eq!(data.order.last().map(String::as_str), Some("mine"));
    }

    /// 本机正在用的 mine 已被清空，导入文件里的 mine 完整。
    fn corrupted_local_and_intact_incoming() -> (Root, Root) {
        let mut local = Root::default_seeded();
        let mut broken = relay("mine", "Mine", "");
        broken.settings_config = json!({ "env": {} });
        add(&mut local, "claude", broken, true);
        let mut incoming = Root::default_seeded();
        add(
            &mut incoming,
            "claude",
            relay("mine", "Mine", "https://mine.example"),
            false,
        );
        (local, incoming)
    }

    #[test]
    fn import_conflicts_report_differing_active_card() {
        let (local, incoming) = corrupted_local_and_intact_incoming();
        let conflicts = incoming.import_conflicts(&local);
        assert_eq!(conflicts.len(), 1);
        let conflict = &conflicts[0];
        assert_eq!(
            (conflict.app.as_str(), conflict.id.as_str()),
            ("claude", "mine")
        );
        assert!(conflict.local.empty);
        assert!(!conflict.imported.empty);
        assert_eq!(conflict.imported.url, "https://mine.example");
        assert_eq!(conflict.imported.key, "****");

        // 内容相同、或本机用的是官方账号时不算冲突。
        assert!(incoming.import_conflicts(&incoming).is_empty());
        assert!(incoming
            .import_conflicts(&Root::default_seeded())
            .is_empty());
    }

    fn choose(app: &str, id: &str) -> Vec<ImportChoice> {
        vec![ImportChoice {
            app: app.into(),
            id: id.into(),
        }]
    }

    #[test]
    fn import_can_adopt_imported_version_of_active_card() {
        let (local, mut incoming) = corrupted_local_and_intact_incoming();
        let alignment = incoming.align_import_with_local(&local, &choose("claude", "mine"));
        assert_eq!(alignment.adopted.len(), 1);
        assert_eq!(alignment.adopted[0].app, "claude");
        // 本机那张是空卡片，不另存副本。
        assert_eq!(alignment.adopted[0].local_copy, None);
        assert!(alignment.kept.is_empty());
        let data = &incoming.apps["claude"];
        assert_eq!(data.current.as_deref(), Some("mine"));
        assert_eq!(
            data.providers["mine"].settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://mine.example"
        );
        assert!(!data.providers.contains_key("mine-2"));

        // 写入客户端配置失败时回退为本机版本，导入的那张改用新 id 保留。
        assert_eq!(
            incoming.revert_adoption(&alignment.adopted[0], &local),
            Some("Mine".to_string())
        );
        let data = &incoming.apps["claude"];
        assert_eq!(data.providers["mine"].settings_config, json!({ "env": {} }));
        assert_eq!(
            data.providers["mine-2"].settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://mine.example"
        );
    }

    #[test]
    fn adopting_keeps_non_empty_local_version_as_copy() {
        let mut local = Root::default_seeded();
        add(
            &mut local,
            "claude",
            relay("mine", "Mine", "https://local.example"),
            true,
        );
        let mut incoming = Root::default_seeded();
        add(
            &mut incoming,
            "claude",
            relay("mine", "Mine", "https://imported.example"),
            false,
        );
        let base = |root: &Root, id: &str| {
            root.apps["claude"].providers[id].settings_config["env"]["ANTHROPIC_BASE_URL"]
                .as_str()
                .unwrap()
                .to_string()
        };

        let mut adopted = incoming.clone();
        let alignment = adopted.align_import_with_local(&local, &choose("claude", "mine"));
        assert_eq!(alignment.adopted[0].local_copy.as_deref(), Some("mine-2"));
        assert_eq!(base(&adopted, "mine"), "https://imported.example");
        assert_eq!(base(&adopted, "mine-2"), "https://local.example");
        assert_eq!(
            adopted.apps["claude"].order,
            vec![CLAUDE_OFFICIAL_PROVIDER_ID, "mine", "mine-2"]
        );

        // 回退：移除本机副本后与「保留本机版本」结果一致，不出现重复卡片。
        adopted.revert_adoption(&alignment.adopted[0], &local);
        let mut kept = incoming.clone();
        kept.align_import_with_local(&local, &[]);
        assert_eq!(
            serde_json::to_value(&adopted.apps["claude"]).unwrap(),
            serde_json::to_value(&kept.apps["claude"]).unwrap()
        );

        // 选择时的卡片已不是本机当前项（期间从托盘切换过）：不生效，保留本机版本。
        let mut stale = incoming.clone();
        let alignment = stale.align_import_with_local(&local, &choose("claude", "other"));
        assert!(alignment.adopted.is_empty());
        assert_eq!(base(&stale, "mine"), "https://local.example");
    }

    #[test]
    fn use_imported_without_conflict_keeps_local() {
        let mut local = Root::default_seeded();
        add(
            &mut local,
            "claude",
            relay("mine", "Mine", "https://mine.example"),
            true,
        );
        let mut incoming = Root::default_seeded();
        let alignment = incoming.align_import_with_local(&local, &choose("claude", "mine"));
        assert!(alignment.adopted.is_empty());
        assert_eq!(alignment.kept, vec!["Mine".to_string()]);
    }

    #[test]
    fn endpoint_summary_reads_codex_and_grok() {
        let codex = Provider {
            id: "c".into(),
            name: "C".into(),
            category: None,
            settings_config: json!({
                "auth": { "OPENAI_API_KEY": "sk-1234567890abcdef" },
                "config": "model_provider = \"r\"\n[model_providers.r]\nbase_url = \"https://c.example/v1\"\n"
            }),
            meta: json!({}),
            failover: json!({}),
        };
        let summary = endpoint_summary("codex", &codex);
        assert_eq!(summary.url, "https://c.example/v1");
        assert_eq!(summary.key, "sk-1…cdef");
        assert!(!summary.empty);

        let grok = Provider {
            settings_config: json!({
                "config": "[endpoints]\nmodels_base_url = \"https://g.example/v1\"\n"
            }),
            ..codex
        };
        let summary = endpoint_summary("grok", &grok);
        assert_eq!(summary.url, "https://g.example/v1");
        assert!(summary.empty);
    }
}
