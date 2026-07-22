//! 供应商真实流式调用测试。
//! 固定发送最小提示 `Hi`，限制输出长度，通过 Tauri Channel 实时回传文本增量。
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

const MAX_OUTPUT_TOKENS: u32 = 32;
const MAX_CAPTURE_CHARS: usize = 2_000;
const MAX_ERROR_CHARS: usize = 240;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTestEvent {
    pub kind: String,
    pub text: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTestResult {
    pub text: String,
    pub first_token_ms: u64,
    pub total_ms: u64,
    pub streamed: bool,
}

struct SseParser {
    buffer: Vec<u8>,
    event_data: String,
}

impl SseParser {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            event_data: String::new(),
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = self.buffer.drain(..=index).collect();
            let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]);
            self.consume_line(line.trim_end_matches('\r'), &mut events);
        }
        events
    }

    fn finish(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&remaining);
            self.consume_line(line.trim_end_matches('\r'), &mut events);
        }
        if !self.event_data.is_empty() {
            events.push(std::mem::take(&mut self.event_data));
        }
        events
    }

    fn consume_line(&mut self, line: &str, events: &mut Vec<String>) {
        if line.is_empty() {
            if !self.event_data.is_empty() {
                events.push(std::mem::take(&mut self.event_data));
            }
            return;
        }
        if let Some(data) = line.strip_prefix("data:") {
            if !self.event_data.is_empty() {
                self.event_data.push('\n');
            }
            self.event_data.push_str(data.trim_start());
        }
    }
}

fn endpoint(base_url: &str, app: &str, wire_api: &str) -> Result<String, String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("请先填写 Base URL".into());
    }
    let path = match (app, wire_api) {
        ("claude", _) => "/messages",
        ("codex" | "grok", "chat") => "/chat/completions",
        ("codex" | "grok", "responses") => "/responses",
        ("codex" | "grok", _) => return Err("wire_api 只能是 chat 或 responses".into()),
        _ => return Err(format!("未知应用: {app}")),
    };
    if base.ends_with(path) {
        return Ok(base.to_string());
    }
    if base.ends_with("/v1") {
        Ok(format!("{base}{path}"))
    } else {
        Ok(format!("{base}/v1{path}"))
    }
}

fn request_body(app: &str, wire_api: &str, model: &str) -> Value {
    match (app, wire_api) {
        ("claude", _) => serde_json::json!({
            "model": model,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "stream": true,
            "messages": [{ "role": "user", "content": "Hi" }]
        }),
        ("codex" | "grok", "responses") => serde_json::json!({
            "model": model,
            "input": "Hi",
            "max_output_tokens": MAX_OUTPUT_TOKENS,
            "stream": true
        }),
        _ => serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Hi" }],
            "max_tokens": MAX_OUTPUT_TOKENS,
            "stream": true
        }),
    }
}

fn text_delta(data: &str) -> Option<String> {
    if data.trim() == "[DONE]" {
        return None;
    }
    let value: Value = serde_json::from_str(data).ok()?;
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

    if event_type == "response.output_text.delta" {
        return value
            .get("delta")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    value
        .pointer("/delta/text")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/content_block/text").and_then(Value::as_str))
        .or_else(|| {
            value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
        })
        .or_else(|| value.pointer("/choices/0/text").and_then(Value::as_str))
        .map(str::to_string)
}

fn full_text(value: &Value) -> Option<String> {
    if let Some(text) = value.pointer("/content/0/text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    let output = value.get("output")?.as_array()?;
    let mut text = String::new();
    for item in output {
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for part in content {
                if let Some(piece) = part.get("text").and_then(Value::as_str) {
                    text.push_str(piece);
                }
            }
        }
    }
    (!text.is_empty()).then_some(text)
}

fn error_excerpt(body: &str, api_key: &str) -> String {
    let flattened = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let redacted = if api_key.trim().is_empty() {
        flattened
    } else {
        flattened.replace(api_key.trim(), "***")
    };
    redacted.chars().take(MAX_ERROR_CHARS).collect()
}

fn http_error(status: reqwest::StatusCode, body: &str, api_key: &str) -> String {
    let category = match status.as_u16() {
        400 => "请求格式、模型名称或协议不兼容",
        401 | 403 => "API Key 无效或没有模型权限",
        404 => "接口路径、模型名称或 wire_api 不匹配",
        408 => "供应商处理超时",
        429 => "余额不足、额度耗尽或请求频率受限",
        500..=599 => "供应商服务异常",
        _ => "供应商拒绝了请求",
    };
    let detail = error_excerpt(body, api_key);
    if detail.is_empty() {
        format!("{category}（HTTP {}）", status.as_u16())
    } else {
        format!("{category}（HTTP {}）：{detail}", status.as_u16())
    }
}

fn append_text(
    piece: &str,
    output: &mut String,
    first_token_ms: &mut Option<u64>,
    start: Instant,
    on_event: &Channel<StreamTestEvent>,
) {
    if piece.is_empty() {
        return;
    }
    if first_token_ms.is_none() {
        *first_token_ms = Some(start.elapsed().as_millis() as u64);
    }
    if output.chars().count() < MAX_CAPTURE_CHARS {
        let remaining = MAX_CAPTURE_CHARS.saturating_sub(output.chars().count());
        output.extend(piece.chars().take(remaining));
    }
    let _ = on_event.send(StreamTestEvent {
        kind: "delta".into(),
        text: Some(piece.to_string()),
        endpoint: None,
    });
}

pub async fn run(
    app: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
    api_key_field: Option<&str>,
    on_event: Channel<StreamTestEvent>,
) -> Result<StreamTestResult, String> {
    if api_key.trim().is_empty() {
        return Err("请先填写 API Key".into());
    }
    if model.trim().is_empty() {
        return Err("请先选择或填写模型".into());
    }
    let url = endpoint(base_url, app, wire_api)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("创建测试客户端失败：{error}"))?;

    let mut request = client
        .post(&url)
        .json(&request_body(app, wire_api, model.trim()));
    if app == "claude" {
        request = request.header("anthropic-version", "2023-06-01");
        if api_key_field == Some("ANTHROPIC_API_KEY") {
            request = request.header("x-api-key", api_key.trim());
        } else {
            request = request.bearer_auth(api_key.trim());
        }
    } else {
        request = request.bearer_auth(api_key.trim());
    }

    let _ = on_event.send(StreamTestEvent {
        kind: "started".into(),
        text: None,
        endpoint: Some(url.clone()),
    });
    let start = Instant::now();
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            "真实调用超时，请检查供应商状态或网络".to_string()
        } else if error.is_connect() {
            "无法连接供应商，请检查 Base URL 和网络".to_string()
        } else {
            format!("真实调用失败：{error}")
        }
    })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(http_error(status, &body, api_key));
    }

    let advertised_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false);
    let mut stream = response.bytes_stream();
    let mut parser = SseParser::new();
    let mut raw = Vec::new();
    let mut output = String::new();
    let mut first_token_ms = None;
    let mut saw_sse_data = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("读取流式回复失败：{error}"))?;
        if raw.len() < 64 * 1024 {
            let remaining = 64 * 1024 - raw.len();
            raw.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        for data in parser.push(&chunk) {
            saw_sse_data = true;
            if let Some(piece) = text_delta(&data) {
                append_text(&piece, &mut output, &mut first_token_ms, start, &on_event);
            }
        }
    }
    for data in parser.finish() {
        saw_sse_data = true;
        if let Some(piece) = text_delta(&data) {
            append_text(&piece, &mut output, &mut first_token_ms, start, &on_event);
        }
    }

    let streamed = advertised_stream || saw_sse_data;
    if output.trim().is_empty() {
        let body = String::from_utf8_lossy(&raw);
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            if let Some(text) = full_text(&value) {
                append_text(&text, &mut output, &mut first_token_ms, start, &on_event);
            }
        }
    }
    if output.trim().is_empty() {
        return Err("供应商返回成功，但没有可显示的模型文本；请检查模型名称和协议".into());
    }

    Ok(StreamTestResult {
        text: output,
        first_token_ms: first_token_ms.unwrap_or_else(|| start.elapsed().as_millis() as u64),
        total_ms: start.elapsed().as_millis() as u64,
        streamed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_expected_endpoints() {
        assert_eq!(
            endpoint("https://example.com/anthropic", "claude", "chat").unwrap(),
            "https://example.com/anthropic/v1/messages"
        );
        assert_eq!(
            endpoint("https://example.com/v1", "codex", "responses").unwrap(),
            "https://example.com/v1/responses"
        );
        assert_eq!(
            endpoint("https://example.com/v1", "codex", "chat").unwrap(),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn parses_chunked_sse_and_protocol_deltas() {
        let mut parser = SseParser::new();
        assert!(parser.push(b"data: {\"choices\":[{\"delta\":{").is_empty());
        let events = parser.push(b"\"content\":\"Hi\"}}]}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(text_delta(&events[0]).as_deref(), Some("Hi"));
        assert_eq!(
            text_delta(
                r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"!"}}"#
            )
            .as_deref(),
            Some("!")
        );
        assert_eq!(
            text_delta(r#"{"type":"response.output_text.delta","delta":"OK"}"#).as_deref(),
            Some("OK")
        );
    }

    #[test]
    fn redacts_api_key_from_error_details() {
        let message = http_error(
            reqwest::StatusCode::BAD_REQUEST,
            "request contained sk-secret-value",
            "sk-secret-value",
        );
        assert!(!message.contains("sk-secret-value"));
        assert!(message.contains("***"));
    }

    #[test]
    fn performs_real_sse_request_against_local_provider() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            use axum::{
                body::Body,
                http::{header, Response},
                routing::post,
                Router,
            };

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = Router::new().route(
                "/v1/chat/completions",
                post(|| async {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n\
                             data: {\"choices\":[{\"delta\":{\"content\":\"!\"}}]}\n\n\
                             data: [DONE]\n\n",
                        ))
                        .unwrap()
                }),
            );
            tauri::async_runtime::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            let channel = Channel::new(|_| Ok(()));
            let result = run(
                "codex",
                &format!("http://{address}"),
                "test-key",
                "test-model",
                "chat",
                None,
                channel,
            )
            .await
            .unwrap();

            assert_eq!(result.text, "Hi!");
            assert!(result.streamed);
            assert!(result.total_ms >= result.first_token_ms);
        });
    }
}
