//! 网络代理（出站 HTTP 代理）：供应商级设置 + 全局默认，供应商设置优先。
//!
//! - 数据：全局默认 `settings.networkProxy`（字符串）；供应商 `meta.proxy`：
//!   缺省 = 跟随全局、`{ "mode": "custom", "url": "..." }` = 自定义、`{ "mode": "off" }` = 不使用。
//! - 直连模式：写进客户端 live 配置——Claude 注入 settings.json 的 env，
//!   Codex 写 `~/.codex/.env` 里由 z-switch 托管的区块（Codex 启动时加载该文件，后定义的覆盖先定义的）。
//! - 本地路由模式：客户端 → 127.0.0.1 不走代理，由路由按上游目标使用代理转发。
//! - 「不使用」只表示 z-switch 不注入，不会去清理用户自己在 shell / .env 里设的代理。
use serde_json::{Map, Value};
use std::fs;
use std::time::{Duration, Instant};

use crate::config;
use crate::store::Provider;

/// 支持网络代理的客户端（Grok 暂不支持）。
pub fn supports(app: &str) -> bool {
    matches!(app, "claude" | "codex")
}

/// Claude Code 注入的环境变量：Node 生态有的先读小写，大小写都写。
pub const CLAUDE_PROXY_KEYS: &[&str] = &["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"];

/// Codex（reqwest）优先读大写，且 .env 的值会覆盖 shell 环境，写大写即可。
const CODEX_PROXY_KEYS: &[&str] = &["HTTPS_PROXY", "HTTP_PROXY"];

const ENV_BLOCK_BEGIN: &str = "# >>> z-switch 网络代理（自动生成，请勿手改；在 z-switch 中修改） >>>";
const ENV_BLOCK_END: &str = "# <<< z-switch 网络代理 <<<";

/// 校验并规范化代理地址：仅支持 http(s)，不含路径 / 查询；返回去掉尾部 `/` 的地址。
pub fn validate(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("代理地址不能为空".into());
    }
    if s.chars().any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\' | '`')) {
        return Err("代理地址不能包含空格、引号或反斜杠".into());
    }
    let url = reqwest::Url::parse(s)
        .map_err(|_| "代理地址格式不正确，示例：http://127.0.0.1:7890".to_string())?;
    match url.scheme() {
        "http" | "https" => {}
        "socks5" | "socks5h" | "socks4" | "socks" => {
            return Err(
                "暂不支持 SOCKS 代理（Claude Code 不支持），请填写 HTTP 代理端口；Clash 等工具的混合端口同时支持 HTTP"
                    .into(),
            )
        }
        other => return Err(format!("不支持的代理协议：{other}，仅支持 http:// 或 https://")),
    }
    if url.host_str().is_none_or(|h| h.is_empty()) {
        return Err("代理地址缺少主机名".into());
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err("代理地址只需填写协议、主机和端口，例如 http://127.0.0.1:7890".into());
    }
    Ok(s.trim_end_matches('/').to_string())
}

/// 全局默认代理（未设置或不合法 → None）。
pub fn global_proxy(settings: &Value) -> Option<String> {
    settings
        .get("networkProxy")
        .and_then(Value::as_str)
        .and_then(|s| validate(s).ok())
}

/// 供应商最终生效的代理：自定义 > 不使用 > 跟随全局。
pub fn effective(app: &str, settings: &Value, provider: &Provider) -> Option<String> {
    if !supports(app) {
        return None;
    }
    let setting = provider.meta.get("proxy");
    match setting.and_then(|p| p.get("mode")).and_then(Value::as_str) {
        Some("custom") => setting
            .and_then(|p| p.get("url"))
            .and_then(Value::as_str)
            .and_then(|s| validate(s).ok()),
        Some("off") => None,
        _ => global_proxy(settings),
    }
}

/// 规范化供应商代理设置（写入 meta.proxy 前调用）：
/// 跟随全局 → None（不存字段）；自定义 → 校验后的地址；不使用 → `{mode:"off"}`。
pub fn normalize_setting(setting: Option<&Value>) -> Result<Option<Value>, String> {
    let Some(setting) = setting.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    match setting.get("mode").and_then(Value::as_str) {
        None | Some("global") => Ok(None),
        Some("off") => Ok(Some(serde_json::json!({ "mode": "off" }))),
        Some("custom") => {
            let raw = setting.get("url").and_then(Value::as_str).unwrap_or("");
            let url = validate(raw)?;
            Ok(Some(serde_json::json!({ "mode": "custom", "url": url })))
        }
        Some(other) => Err(format!("未知的网络代理模式：{other}")),
    }
}

/// 就地规范化 provider.meta.proxy（跟随全局时移除字段）。
pub fn normalize_provider(app: &str, provider: &mut Provider) -> Result<(), String> {
    let normalized = if supports(app) {
        normalize_setting(provider.meta.get("proxy"))?
    } else {
        None
    };
    if let Some(meta) = provider.meta.as_object_mut() {
        match normalized {
            Some(value) => {
                meta.insert("proxy".into(), value);
            }
            None => {
                meta.remove("proxy");
            }
        }
    } else if let Some(value) = normalized {
        provider.meta = serde_json::json!({ "proxy": value });
    }
    Ok(())
}

/// 某客户端 z-switch 可能注入过的所有代理地址（全局默认 + 各供应商自定义）。
/// 回填 / 改写 live 时只清理值命中这些地址的代理变量，用户手写的其它代理保持不动。
pub fn managed_urls(app: &str, root: &crate::store::Root) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    if !supports(app) {
        return urls;
    }
    if let Some(global) = global_proxy(&root.settings) {
        urls.push(global);
    }
    if let Some(data) = root.apps.get(app) {
        for provider in data.providers.values() {
            if let Some(url) = effective(app, &root.settings, provider) {
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
    }
    urls
}

// ---------- Claude：settings.json env ----------

/// 把代理写进 Claude env（None 时不动）。供应商 env 里已有用户自己写的代理变量时以其为准，不注入
/// （否则回填时注入值被剔除，用户的代理也随之丢失）。调用方需先剔除 z-switch 注入过的旧值。
pub fn inject_claude_env(env: &mut Value, proxy: Option<&str>) {
    let Some(proxy) = proxy else {
        return;
    };
    if !env.is_object() {
        *env = Value::Object(Map::new());
    }
    let object = env.as_object_mut().unwrap();
    if CLAUDE_PROXY_KEYS.iter().any(|key| object.contains_key(*key)) {
        return;
    }
    for key in CLAUDE_PROXY_KEYS {
        object.insert((*key).to_string(), Value::String(proxy.to_string()));
    }
}

/// 移除 Claude env 中值命中 `managed` 的代理变量，返回是否有改动。
pub fn strip_claude_env(env: &mut Value, managed: &[String]) -> bool {
    let Some(object) = env.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for key in CLAUDE_PROXY_KEYS {
        let hit = object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| managed.iter().any(|m| m == value.trim_end_matches('/')));
        if hit {
            object.remove(*key);
            changed = true;
        }
    }
    changed
}

// ---------- Codex：~/.codex/.env 托管区块 ----------

/// 在 .env 文本中替换 z-switch 托管区块：先移除旧区块，proxy 为 Some 时追加到文件末尾
/// （Codex 逐行 set_var，末尾的定义优先）。区块外的用户内容原样保留。
pub fn render_codex_env(existing: &str, proxy: Option<&str>) -> String {
    // 没有托管区块、也不需要代理：用户的文件原样不动。
    if proxy.is_none() && !existing.lines().any(|line| line.trim() == ENV_BLOCK_BEGIN) {
        return existing.to_string();
    }
    let newline = if existing.contains("\r\n") { "\r\n" } else { "\n" };
    let mut kept: Vec<&str> = Vec::new();
    // 区块内的行先暂存：找不到结束标记（被手改）时放回，避免误删用户内容。
    let mut block: Option<Vec<&str>> = None;
    for line in existing.lines() {
        let trimmed = line.trim();
        match block.as_mut() {
            Some(lines) => {
                if trimmed == ENV_BLOCK_END {
                    block = None;
                } else {
                    lines.push(line);
                }
            }
            None if trimmed == ENV_BLOCK_BEGIN => block = Some(Vec::new()),
            None => kept.push(line),
        }
    }
    if let Some(lines) = block {
        kept.extend(lines);
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }

    let mut out = String::new();
    for line in &kept {
        out.push_str(line);
        out.push_str(newline);
    }
    if let Some(proxy) = proxy {
        if !out.is_empty() {
            out.push_str(newline);
        }
        out.push_str(ENV_BLOCK_BEGIN);
        out.push_str(newline);
        for key in CODEX_PROXY_KEYS {
            // 单引号：dotenvy 按字面量解析，不做 `$` 变量替换。
            out.push_str(&format!("{key}='{proxy}'"));
            out.push_str(newline);
        }
        out.push_str(ENV_BLOCK_END);
        out.push_str(newline);
    }
    out
}

/// 写 .env 前的状态，供 Codex 写 live 失败时回滚。
pub struct CodexEnvUndo(Option<Option<Vec<u8>>>);

/// 按目标代理更新 `~/.codex/.env`：内容不变则不写；结果为空则删除文件；
/// 文件不存在且无需代理时不创建。返回回滚句柄。
pub fn write_codex_env(proxy: Option<&str>, backup: bool) -> Result<CodexEnvUndo, String> {
    let path = config::get_codex_env_path();
    let previous = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("读取 {} 失败：{e}", path.display())),
    };
    let existing = match &previous {
        Some(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => text,
            // 非 UTF-8 的文件不可能含 z-switch 区块；不需要代理时无事可做，不阻断切换 / 恢复。
            Err(_) if proxy.is_none() => return Ok(CodexEnvUndo(None)),
            Err(_) => {
                return Err(format!(
                    "{} 不是 UTF-8 文本，已中止写入以防损坏",
                    path.display()
                ))
            }
        },
        None => String::new(),
    };
    let next = render_codex_env(&existing, proxy);
    if next == existing {
        return Ok(CodexEnvUndo(None));
    }
    if backup {
        crate::live::backup_file(&path, "codex-env");
    }
    if next.trim().is_empty() {
        if previous.is_some() {
            fs::remove_file(&path).map_err(|e| format!("删除 {} 失败：{e}", path.display()))?;
        }
    } else {
        config::write_text_file(&path, &next)?;
    }
    Ok(CodexEnvUndo(Some(previous)))
}

/// 回滚 write_codex_env（best-effort）。
pub fn undo_codex_env(undo: CodexEnvUndo) {
    let path = config::get_codex_env_path();
    match undo.0 {
        None => {}
        Some(Some(bytes)) => {
            let _ = config::atomic_write(&path, &bytes);
        }
        Some(None) => {
            let _ = fs::remove_file(&path);
        }
    }
}

// ---------- 测试工具 / 可达性 ----------

/// 给测试类请求（测速 / 拉模型 / 连通性 / 真实测试）挂上代理；None 保持 reqwest 默认行为。
pub fn apply_to_builder(
    builder: reqwest::ClientBuilder,
    proxy: Option<&str>,
) -> Result<reqwest::ClientBuilder, String> {
    match proxy.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            let url = validate(raw)?;
            let proxy = reqwest::Proxy::all(&url).map_err(|e| format!("代理地址无效：{e}"))?;
            Ok(builder.proxy(proxy))
        }
        None => Ok(builder),
    }
}

/// 检测代理端口是否可连接（TCP 握手，3 秒超时），返回耗时毫秒。
/// 只验证代理进程在监听，不经代理访问任何外部地址。
pub async fn check(raw: &str) -> Result<f64, String> {
    let url = reqwest::Url::parse(&validate(raw)?).map_err(|e| e.to_string())?;
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']).to_string();
    let port = url.port_or_known_default().unwrap_or(80);
    let start = Instant::now();
    let connect = tokio::net::TcpStream::connect((host.as_str(), port));
    match tokio::time::timeout(Duration::from_secs(3), connect).await {
        Ok(Ok(_)) => Ok(start.elapsed().as_secs_f64() * 1000.0),
        Ok(Err(e)) => Err(format!("无法连接代理 {host}:{port}：{e}")),
        Err(_) => Err(format!("连接代理 {host}:{port} 超时")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(meta: Value) -> Provider {
        Provider {
            id: "p".into(),
            name: "P".into(),
            category: None,
            settings_config: json!({}),
            meta,
            failover: json!({}),
        }
    }

    #[test]
    fn validate_accepts_http_and_rejects_others() {
        assert_eq!(validate(" http://127.0.0.1:7890/ ").unwrap(), "http://127.0.0.1:7890");
        assert!(validate("https://user:pass@proxy.example:8443").is_ok());
        assert!(validate("socks5://127.0.0.1:7890").unwrap_err().contains("SOCKS"));
        assert!(validate("127.0.0.1:7890").is_err());
        assert!(validate("http://127.0.0.1:7890/path").is_err());
        assert!(validate("http://a b:1").is_err());
        assert!(validate("http://x:1'").is_err());
        assert!(validate("").is_err());
    }

    #[test]
    fn provider_setting_takes_priority_over_global() {
        let settings = json!({ "networkProxy": "http://127.0.0.1:7890" });
        let follow = provider(json!({}));
        let custom = provider(json!({ "proxy": { "mode": "custom", "url": "http://10.0.0.2:3128" } }));
        let off = provider(json!({ "proxy": { "mode": "off" } }));
        assert_eq!(effective("claude", &settings, &follow).as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(effective("codex", &settings, &custom).as_deref(), Some("http://10.0.0.2:3128"));
        assert_eq!(effective("claude", &settings, &off), None);
        assert_eq!(effective("grok", &settings, &follow), None);
        assert_eq!(effective("claude", &json!({}), &follow), None);
        assert_eq!(effective("claude", &json!({ "networkProxy": "bad" }), &follow), None);
    }

    #[test]
    fn claude_inject_and_strip_only_touch_managed_values() {
        let mut env = json!({ "ANTHROPIC_BASE_URL": "https://relay", "NO_PROXY": "localhost" });
        inject_claude_env(&mut env, Some("http://127.0.0.1:7890"));
        for key in CLAUDE_PROXY_KEYS {
            assert_eq!(env[*key], "http://127.0.0.1:7890");
        }
        // 用户手写的不同值不应被清理。
        env["http_proxy"] = json!("http://corp:8080");
        assert!(strip_claude_env(&mut env, &["http://127.0.0.1:7890".to_string()]));
        assert!(env.get("HTTPS_PROXY").is_none());
        assert_eq!(env["http_proxy"], "http://corp:8080");
        assert_eq!(env["NO_PROXY"], "localhost");
        assert!(!strip_claude_env(&mut env, &["http://127.0.0.1:7890".to_string()]));
    }

    #[test]
    fn claude_inject_keeps_user_own_proxy() {
        let mut env = json!({ "https_proxy": "http://corp:8080" });
        inject_claude_env(&mut env, Some("http://127.0.0.1:7890"));
        assert_eq!(env, json!({ "https_proxy": "http://corp:8080" }));
    }

    #[test]
    fn codex_env_without_block_is_untouched_and_unterminated_block_keeps_lines() {
        // 无区块且无需代理：原样返回（包括末尾空行、无换行结尾）。
        let user = "FOO=bar\n\n\nBAZ=1";
        assert_eq!(render_codex_env(user, None), user);
        // 结束标记被手动删掉：只去掉开始标记，后面的行保留。
        let broken = format!("FOO=bar\n{ENV_BLOCK_BEGIN}\nHTTPS_PROXY='http://x:1'\nMINE=1\n");
        assert_eq!(render_codex_env(&broken, None), "FOO=bar\nHTTPS_PROXY='http://x:1'\nMINE=1\n");
    }

    #[test]
    fn codex_env_block_round_trip_preserves_user_lines() {
        let user = "# my env\r\nFOO=bar\r\nHTTPS_PROXY=http://mine:1\r\n";
        let with = render_codex_env(user, Some("http://127.0.0.1:7890"));
        assert!(with.starts_with(user));
        assert!(with.ends_with(&format!(
            "{ENV_BLOCK_BEGIN}\r\nHTTPS_PROXY='http://127.0.0.1:7890'\r\nHTTP_PROXY='http://127.0.0.1:7890'\r\n{ENV_BLOCK_END}\r\n"
        )));
        // 幂等：再次渲染同一代理不变；换代理只替换区块。
        assert_eq!(render_codex_env(&with, Some("http://127.0.0.1:7890")), with);
        let swapped = render_codex_env(&with, Some("http://10.0.0.2:3128"));
        assert!(swapped.starts_with(user) && swapped.contains("10.0.0.2") && !swapped.contains("7890"));
        // 移除区块后恢复用户原文。
        assert_eq!(render_codex_env(&with, None), user);
        assert_eq!(render_codex_env("", None), "");
    }

    #[test]
    fn codex_env_file_is_not_created_without_proxy_and_removed_when_empty() {
        let _lock = config::TEST_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::temp_dir().join(format!(
            "z-switch-netproxy-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&home).unwrap();
        let previous = std::env::var_os("Z_SWITCH_TEST_HOME");
        std::env::set_var("Z_SWITCH_TEST_HOME", &home);
        let path = config::get_codex_env_path();

        write_codex_env(None, false).unwrap();
        assert!(!path.exists());

        let undo = write_codex_env(Some("http://127.0.0.1:7890"), false).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("HTTPS_PROXY='http://127.0.0.1:7890'"));
        undo_codex_env(undo);
        assert!(!path.exists());

        write_codex_env(Some("http://127.0.0.1:7890"), false).unwrap();
        write_codex_env(None, false).unwrap();
        assert!(!path.exists());

        // 非 UTF-8 文件：无需代理时不阻断、不改动；需要代理时拒绝写入。
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, [0xff, 0xfe, b'A']).unwrap();
        assert!(write_codex_env(None, false).is_ok());
        assert!(write_codex_env(Some("http://127.0.0.1:7890"), false).is_err());
        assert_eq!(fs::read(&path).unwrap(), vec![0xff, 0xfe, b'A']);

        match previous {
            Some(value) => std::env::set_var("Z_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("Z_SWITCH_TEST_HOME"),
        }
        let _ = fs::remove_dir_all(&home);
    }
}
