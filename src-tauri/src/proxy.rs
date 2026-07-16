//! 第四步 · 可选热切换代理（默认关）。
//!
//! 原理：把 `~/.claude` 的 ANTHROPIC_BASE_URL / `~/.codex` 的 base_url 指向
//! `http://127.0.0.1:<PORT>/{claude,codex}`；本模块起 localhost 服务，收到请求后
//! **原样透传**给「当前 target」的真实 base_url，并把 target 的 key 注入请求头
//! （覆盖客户端携带的任意 key）。切换供应商时只改内存里的 target → 客户端不知道、
//! 无需重启。
//!
//! 纪律：不改 body、不做格式转换、不碰 key 内容、不统计、不上传。纯本地邮差。
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::Response,
    routing::any,
    Router,
};
use futures_util::StreamExt;
use tokio::sync::oneshot;

use crate::proxy_log;

/// 默认监听端口（可被 settings.reliability.proxyPort 覆盖）。
pub const DEFAULT_PORT: u16 = 8899;

/// 本地路由运行参数。缺省值偏保守，所有数值在读取设置时都会限制范围。
#[derive(Clone, Debug)]
pub struct ProxyRuntimeConfig {
    pub connect_timeout: Duration,
    pub streaming_first_byte_timeout: Duration,
    pub streaming_idle_timeout: Duration,
    pub non_streaming_timeout: Duration,
    pub request_body_limit_bytes: usize,
    pub pool_max_idle_per_host: usize,
    pub tcp_keepalive: Duration,
    pub error_log_enabled: bool,
    pub error_log_max_mb: u64,
}

impl Default for ProxyRuntimeConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            streaming_first_byte_timeout: Duration::from_secs(60),
            streaming_idle_timeout: Duration::from_secs(120),
            non_streaming_timeout: Duration::from_secs(600),
            request_body_limit_bytes: 64 * 1024 * 1024,
            pool_max_idle_per_host: 10,
            tcp_keepalive: Duration::from_secs(60),
            error_log_enabled: true,
            error_log_max_mb: 5,
        }
    }
}

impl ProxyRuntimeConfig {
    pub fn from_settings(settings: &serde_json::Value) -> Self {
        let defaults = Self::default();
        let reliability = settings.get("reliability");
        let number = |key: &str, fallback: u64, min: u64, max: u64| {
            reliability
                .and_then(|value| value.get(key))
                .and_then(|value| value.as_u64())
                .unwrap_or(fallback)
                .clamp(min, max)
        };
        let request_body_mb = number(
            "requestBodyLimitMb",
            (defaults.request_body_limit_bytes / 1024 / 1024) as u64,
            1,
            256,
        );
        Self {
            connect_timeout: Duration::from_secs(number(
                "connectTimeoutSeconds",
                defaults.connect_timeout.as_secs(),
                1,
                120,
            )),
            streaming_first_byte_timeout: Duration::from_secs(number(
                "streamingFirstByteTimeoutSeconds",
                defaults.streaming_first_byte_timeout.as_secs(),
                5,
                300,
            )),
            streaming_idle_timeout: Duration::from_secs(number(
                "streamingIdleTimeoutSeconds",
                defaults.streaming_idle_timeout.as_secs(),
                10,
                900,
            )),
            non_streaming_timeout: Duration::from_secs(number(
                "nonStreamingTimeoutSeconds",
                defaults.non_streaming_timeout.as_secs(),
                30,
                3600,
            )),
            request_body_limit_bytes: request_body_mb as usize * 1024 * 1024,
            pool_max_idle_per_host: number(
                "poolMaxIdlePerHost",
                defaults.pool_max_idle_per_host as u64,
                1,
                100,
            ) as usize,
            tcp_keepalive: Duration::from_secs(number(
                "tcpKeepaliveSeconds",
                defaults.tcp_keepalive.as_secs(),
                10,
                600,
            )),
            error_log_enabled: reliability
                .and_then(|value| value.get("proxyErrorLogEnabled"))
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.error_log_enabled),
            error_log_max_mb: number("proxyErrorLogMaxMb", defaults.error_log_max_mb, 1, 100),
        }
    }
}

/// 单个 app（claude/codex）当前要转发到的真实上游 + 需注入的头。
#[derive(Clone, Default)]
pub struct AppTarget {
    /// 真实上游 base_url，例如 https://api.deepseek.com/anthropic
    pub base_url: String,
    /// 需注入/覆盖的请求头（key），例如 ("authorization","Bearer xxx") 或 ("x-api-key","xxx")
    pub headers: Vec<(String, String)>,
}

/// 代理运行期共享状态：claude / codex 各自的当前 target。
#[derive(Default)]
pub struct ProxyTargets {
    pub map: HashMap<String, AppTarget>,
}

/// 可在同步（托盘）与异步（命令、handler）两处访问的共享 target 句柄。
/// 用 std RwLock：handler 读时先取值再 drop guard，绝不跨 await 持有。
pub type SharedTargets = Arc<RwLock<ProxyTargets>>;

/// 轻量句柄：同步/异步两处都能廉价读到「是否在跑 + 当前 targets + 端口」，
/// 无需触碰起停用的 async 锁。存进 Tauri managed state。
#[derive(Clone)]
pub struct ProxyHandle {
    pub targets: SharedTargets,
    pub running: Arc<AtomicBool>,
    pub port: Arc<std::sync::atomic::AtomicU16>,
}

impl Default for ProxyHandle {
    fn default() -> Self {
        Self {
            targets: Arc::new(RwLock::new(ProxyTargets::default())),
            running: Arc::new(AtomicBool::new(false)),
            port: Arc::new(std::sync::atomic::AtomicU16::new(DEFAULT_PORT)),
        }
    }
}

impl ProxyHandle {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    pub fn current_port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }
}

/// 传给 handler 的运行期依赖。
struct Runtime {
    client: reqwest::Client,
    targets: SharedTargets,
    config: ProxyRuntimeConfig,
    error_log_lock: tokio::sync::Mutex<()>,
}

/// 代理生命周期控制器，存进 ProxyState（async 锁保护起停）。
pub struct ProxyControl {
    handle: ProxyHandle,
    /// 触发优雅停机；Some 表示服务在跑。
    shutdown: Option<oneshot::Sender<()>>,
}

impl ProxyControl {
    /// 与 managed ProxyHandle 共享同一 Arc。
    pub fn new(handle: ProxyHandle) -> Self {
        Self {
            handle,
            shutdown: None,
        }
    }

    /// 启动 localhost 服务。已在跑则先停再起。
    pub async fn start(&mut self, port: u16, config: ProxyRuntimeConfig) -> Result<(), String> {
        self.stop();
        let addr = format!("127.0.0.1:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("代理端口 {port} 绑定失败（可能被占用）：{e}"))?;

        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .pool_max_idle_per_host(config.pool_max_idle_per_host)
            .tcp_keepalive(config.tcp_keepalive)
            .build()
            .map_err(|e| format!("构建代理 HTTP 客户端失败：{e}"))?;
        let runtime = Arc::new(Runtime {
            client,
            targets: self.handle.targets.clone(),
            config,
            error_log_lock: tokio::sync::Mutex::new(()),
        });

        let app = Router::new()
            .route("/{app}/{*rest}", any(forward))
            .route("/{app}", any(forward))
            .with_state(runtime);

        let (tx, rx) = oneshot::channel::<()>();
        self.shutdown = Some(tx);
        self.handle.port.store(port, Ordering::SeqCst);
        self.handle.running.store(true, Ordering::SeqCst);

        let running = self.handle.running.clone();
        tauri::async_runtime::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            if let Err(e) = server.await {
                eprintln!("[z-switch] 代理服务退出：{e}");
            }
            running.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    /// 停止服务（若在跑）。
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        self.handle.running.store(false, Ordering::SeqCst);
    }
}

/// 逐跳头（RFC 7230），转发时必须剔除。
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

/// 鉴权相关头：转发前一律剔除客户端原值，改由 target 注入真实 key。
const AUTH_HEADERS: &[&str] = &["authorization", "x-api-key", "api-key"];
const ERROR_BODY_CAPTURE_BYTES: usize = 64 * 1024;

#[derive(serde::Deserialize, Default)]
struct StreamHint {
    #[serde(default)]
    stream: bool,
}

fn target_secrets(target: &AppTarget) -> Vec<String> {
    target
        .headers
        .iter()
        .map(|(_, value)| value.clone())
        .collect()
}

fn safe_error_detail(raw: &str, url: &str, secrets: &[String]) -> String {
    let safe_url = proxy_log::sanitize_url(url);
    proxy_log::redact_and_truncate(&raw.replace(url, &safe_url), secrets)
}

async fn write_proxy_error(
    rt: &Arc<Runtime>,
    app: &str,
    status: Option<u16>,
    url: &str,
    phase: &str,
    detail: &str,
    secrets: &[String],
) {
    if !rt.config.error_log_enabled {
        return;
    }
    let safe_url = proxy_log::sanitize_url(url);
    let detail = safe_error_detail(detail, url, secrets);
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let entry = proxy_log::ProxyErrorEntry {
        timestamp_ms,
        app,
        status,
        url: &safe_url,
        phase,
        detail: &detail,
    };
    let _guard = rt.error_log_lock.lock().await;
    if let Err(error) = proxy_log::append(&entry, rt.config.error_log_max_mb) {
        eprintln!("[z-switch] 写入路由错误日志失败：{error}");
    }
}

/// 核心转发：/{app}/{rest} → target.base_url + /rest（+ query），注入 target 头，
/// 响应流式直通（SSE 不缓冲）。
async fn forward(
    State(rt): State<Arc<Runtime>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, (StatusCode, String)> {
    // 路径形如 /claude/v1/messages → app=claude, rest=/v1/messages
    let path = uri.path();
    let trimmed = path.trim_start_matches('/');
    let (app, rest) = match trimmed.split_once('/') {
        Some((a, r)) => (a.to_string(), format!("/{r}")),
        None => (trimmed.to_string(), String::new()),
    };

    let target = {
        let guard = rt.targets.read().unwrap();
        guard.map.get(&app).cloned()
    };
    let target = target.ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            format!("代理未配置目标：{app}（请在 z-switch 里选择一个供应商）"),
        )
    })?;
    if target.base_url.trim().is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("代理目标 {app} 的 base_url 为空"),
        ));
    }

    // 拼上游 URL：base_url 去尾斜杠 + rest + 原 query
    let base = target.base_url.trim_end_matches('/');
    let mut url = format!("{base}{rest}");
    if let Some(q) = uri.query() {
        url.push('?');
        url.push_str(q);
    }

    // 请求体必须有硬上限：AI 上下文可能很大，并发时无限制读入会耗尽内存。
    let body_limit = rt.config.request_body_limit_bytes;
    if headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > body_limit)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "请求体超过本地路由限制（最大 {}MB）",
                body_limit / 1024 / 1024
            ),
        ));
    }
    let body_bytes = axum::body::to_bytes(body, body_limit)
        .await
        .map_err(|error| {
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "读取请求体失败或超过本地路由限制（最大 {}MB）：{error}",
                    body_limit / 1024 / 1024
                ),
            )
        })?;

    let is_streaming = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"))
        || serde_json::from_slice::<StreamHint>(&body_bytes)
            .map(|hint| hint.stream)
            .unwrap_or(false);

    // 组装转发请求头：透传客户端头（剔除逐跳头 + 一切鉴权头），再统一注入 target 头。
    // 鉴权头一律剔除：客户端里带的是 localhost 占位 key，真实 key 由 target 注入。
    let mut fwd = reqwest::header::HeaderMap::new();
    for (name, value) in headers.iter() {
        let lname = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lname.as_str()) {
            continue;
        }
        if AUTH_HEADERS.contains(&lname.as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            fwd.insert(n, v);
        }
    }
    for (k, v) in &target.headers {
        if let (Ok(n), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(v),
        ) {
            fwd.insert(n, val);
        }
    }

    let mut request = rt
        .client
        .request(method, &url)
        .headers(fwd)
        .body(body_bytes);
    if !is_streaming {
        request = request.timeout(rt.config.non_streaming_timeout);
    }
    let send = request.send();
    let send_result = if is_streaming {
        match tokio::time::timeout(rt.config.streaming_first_byte_timeout, send).await {
            Ok(result) => result,
            Err(_) => {
                let detail = format!(
                    "等待上游响应头超时（{}秒）",
                    rt.config.streaming_first_byte_timeout.as_secs()
                );
                let secrets = target_secrets(&target);
                write_proxy_error(
                    &rt,
                    &app,
                    None,
                    &url,
                    "response_header_timeout",
                    &detail,
                    &secrets,
                )
                .await;
                return Err((StatusCode::GATEWAY_TIMEOUT, detail));
            }
        }
    } else {
        send.await
    };
    let secrets = target_secrets(&target);
    let upstream = match send_result {
        Ok(response) => response,
        Err(error) => {
            let detail =
                safe_error_detail(&format!("连接或发送上游请求失败：{error}"), &url, &secrets);
            write_proxy_error(&rt, &app, None, &url, "request", &detail, &secrets).await;
            return Err((StatusCode::BAD_GATEWAY, detail));
        }
    };

    // 回传：状态码 + 响应头（剔除逐跳头）+ 流式 body 直通
    let status = upstream.status();
    let mut builder = Response::builder().status(status.as_u16());
    for (name, value) in upstream.headers().iter() {
        let lname = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lname.as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            builder = builder.header(n, v);
        }
    }

    let status_code = status.as_u16();
    let log_upstream_error = !status.is_success();
    let stream_rt = rt.clone();
    let stream_app = app.clone();
    let stream_url = url.clone();
    let stream_secrets = secrets.clone();
    let first_timeout = rt.config.streaming_first_byte_timeout;
    let idle_timeout = rt.config.streaming_idle_timeout;
    let non_streaming_timeout = rt.config.non_streaming_timeout;
    let upstream_stream = Box::pin(upstream.bytes_stream());
    let stream = futures_util::stream::unfold(
        (upstream_stream, true, false, Vec::<u8>::new()),
        move |(mut upstream_stream, first_chunk, finished, mut capture)| {
            let rt = stream_rt.clone();
            let app = stream_app.clone();
            let url = stream_url.clone();
            let secrets = stream_secrets.clone();
            async move {
                if finished {
                    return None;
                }
                let timeout = if is_streaming {
                    if first_chunk {
                        first_timeout
                    } else {
                        idle_timeout
                    }
                } else {
                    non_streaming_timeout
                };
                match tokio::time::timeout(timeout, upstream_stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        if log_upstream_error && capture.len() < ERROR_BODY_CAPTURE_BYTES {
                            let remaining = ERROR_BODY_CAPTURE_BYTES - capture.len();
                            capture.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                        }
                        Some((Ok(bytes), (upstream_stream, false, false, capture)))
                    }
                    Ok(Some(Err(error))) => {
                        let detail = safe_error_detail(
                            &format!("读取上游响应流失败：{error}"),
                            &url,
                            &secrets,
                        );
                        write_proxy_error(
                            &rt,
                            &app,
                            Some(status_code),
                            &url,
                            "response_stream",
                            &detail,
                            &secrets,
                        )
                        .await;
                        Some((
                            Err(std::io::Error::other(detail)),
                            (upstream_stream, false, true, capture),
                        ))
                    }
                    Ok(None) => {
                        if log_upstream_error {
                            let detail = if capture.is_empty() {
                                "上游返回错误状态，但响应体为空".to_string()
                            } else {
                                String::from_utf8_lossy(&capture).into_owned()
                            };
                            write_proxy_error(
                                &rt,
                                &app,
                                Some(status_code),
                                &url,
                                "upstream",
                                &detail,
                                &secrets,
                            )
                            .await;
                        }
                        None
                    }
                    Err(_) => {
                        let phase = if first_chunk {
                            "first_byte_timeout"
                        } else {
                            "stream_idle_timeout"
                        };
                        let detail = format!("上游响应等待超时（{}秒）", timeout.as_secs());
                        write_proxy_error(
                            &rt,
                            &app,
                            Some(status_code),
                            &url,
                            phase,
                            &detail,
                            &secrets,
                        )
                        .await;
                        Some((
                            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, detail)),
                            (upstream_stream, false, true, capture),
                        ))
                    }
                }
            }
        },
    );
    let resp = builder.body(Body::from_stream(stream)).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("构建响应失败：{e}"),
        )
    })?;
    Ok(resp)
}

// ---------- 从 Provider 推导代理 target ----------

/// 从一个 provider 的 settings_config 推导出「真实上游 + 需注入的鉴权头」。
/// 返回 None 表示该 provider 无法用于代理（缺 base_url）。
pub fn target_from_provider(app: &str, provider: &crate::store::Provider) -> Option<AppTarget> {
    match app {
        "claude" => {
            let env = provider.settings_config.get("env")?.as_object()?;
            let base = env.get("ANTHROPIC_BASE_URL")?.as_str()?.trim().to_string();
            if base.is_empty() {
                return None;
            }
            let key_field = provider
                .meta
                .get("apiKeyField")
                .and_then(|v| v.as_str())
                .unwrap_or("ANTHROPIC_AUTH_TOKEN");
            let key = env.get(key_field).and_then(|v| v.as_str()).unwrap_or("");
            let mut headers = Vec::new();
            if !key.is_empty() {
                if key_field == "ANTHROPIC_API_KEY" {
                    headers.push(("x-api-key".to_string(), key.to_string()));
                } else {
                    headers.push(("authorization".to_string(), format!("Bearer {key}")));
                }
            }
            Some(AppTarget {
                base_url: base,
                headers,
            })
        }
        "codex" => {
            let cfg = provider.settings_config.get("config")?.as_str()?;
            // 从 config.toml 抓第一个 base_url = "..."
            let base = cfg
                .lines()
                .find_map(|l| l.trim().strip_prefix("base_url"))
                .and_then(|r| r.split('"').nth(1))
                .map(|s| s.trim().to_string())?;
            if base.is_empty() {
                return None;
            }
            let key = provider
                .settings_config
                .get("auth")
                .and_then(|a| a.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut headers = Vec::new();
            if !key.is_empty() {
                headers.push(("authorization".to_string(), format!("Bearer {key}")));
            }
            Some(AppTarget {
                base_url: base,
                headers,
            })
        }
        _ => None,
    }
}

/// 本地端点占位 key（会被代理剔除，仅为让 CLI 不报「缺 key」）。
pub const PLACEHOLDER_KEY: &str = "z-switch-proxy";

/// 更新某个 app 的当前转发 target（热切换核心：只改内存，不重写 live 文件）。
pub fn set_target(targets: &SharedTargets, app: &str, target: AppTarget) {
    if let Ok(mut g) = targets.write() {
        g.map.insert(app.to_string(), target);
    }
}

/// 某个应用恢复为非托管配置后，移除它的代理目标；代理仍可服务另一个应用。
pub fn clear_target(targets: &SharedTargets, app: &str) {
    if let Ok(mut guard) = targets.write() {
        guard.map.remove(app);
    }
}

/// 本地端点：http://127.0.0.1:<port>/<app>
pub fn local_base(port: u16, app: &str) -> String {
    format!("http://127.0.0.1:{port}/{app}")
}

/// 生成一个「指向本地端点」的 provider 副本，用于代理开启时写 live。
/// 真实 provider 不变（仍存 providers.json），只是 live 文件里的 base_url 换成 localhost。
pub fn proxied_provider(
    app: &str,
    provider: &crate::store::Provider,
    port: u16,
) -> crate::store::Provider {
    // 官方账号始终由客户端直连；调用方应跳过，这里再做一次防御。
    if crate::store::is_official_provider(provider) {
        return provider.clone();
    }
    let mut p = provider.clone();
    let local = local_base(port, app);
    match app {
        "claude" => {
            if let Some(env) = p
                .settings_config
                .get_mut("env")
                .and_then(|v| v.as_object_mut())
            {
                env.insert(
                    "ANTHROPIC_BASE_URL".into(),
                    serde_json::Value::String(local),
                );
                let key_field = provider
                    .meta
                    .get("apiKeyField")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ANTHROPIC_AUTH_TOKEN")
                    .to_string();
                env.insert(
                    key_field,
                    serde_json::Value::String(PLACEHOLDER_KEY.to_string()),
                );
            }
        }
        "codex" => {
            if let Some(cfg) = p.settings_config.get("config").and_then(|v| v.as_str()) {
                let rewritten: String = cfg
                    .lines()
                    .map(|line| {
                        if line.trim().starts_with("base_url") {
                            format!("base_url = \"{local}\"")
                        } else {
                            line.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(o) = p.settings_config.as_object_mut() {
                    o.insert("config".into(), serde_json::Value::String(rewritten));
                }
            }
            if let Some(auth) = p
                .settings_config
                .get_mut("auth")
                .and_then(|v| v.as_object_mut())
            {
                auth.insert(
                    "OPENAI_API_KEY".into(),
                    serde_json::Value::String(PLACEHOLDER_KEY.to_string()),
                );
            }
        }
        _ => {}
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_uses_defaults_and_clamps_values() {
        let defaults = ProxyRuntimeConfig::from_settings(&serde_json::json!({}));
        assert_eq!(defaults.request_body_limit_bytes, 64 * 1024 * 1024);
        assert_eq!(defaults.streaming_idle_timeout, Duration::from_secs(120));
        assert!(defaults.error_log_enabled);

        let clamped = ProxyRuntimeConfig::from_settings(&serde_json::json!({
            "reliability": {
                "connectTimeoutSeconds": 0,
                "requestBodyLimitMb": 9999,
                "poolMaxIdlePerHost": 0,
                "proxyErrorLogMaxMb": 9999,
                "proxyErrorLogEnabled": false
            }
        }));
        assert_eq!(clamped.connect_timeout, Duration::from_secs(1));
        assert_eq!(clamped.request_body_limit_bytes, 256 * 1024 * 1024);
        assert_eq!(clamped.pool_max_idle_per_host, 1);
        assert_eq!(clamped.error_log_max_mb, 100);
        assert!(!clamped.error_log_enabled);
    }

    #[tokio::test]
    async fn rejects_oversized_body_before_contacting_upstream() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let handle = ProxyHandle::default();
        set_target(
            &handle.targets,
            "codex",
            AppTarget {
                base_url: "http://127.0.0.1:9".into(),
                headers: vec![],
            },
        );
        let mut control = ProxyControl::new(handle);
        let config = ProxyRuntimeConfig {
            request_body_limit_bytes: 8,
            error_log_enabled: false,
            ..ProxyRuntimeConfig::default()
        };
        control.start(port, config).await.unwrap();

        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/codex/responses"))
            .body("123456789")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        control.stop();
    }
}
