//! 端点测速：TCP 连接计时，无需 reqwest/tokio。仅测到端点的可达延迟（不做 TLS 握手）。
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// 从 base_url 解析出 (host, port)。缺省 https→443、http→80。
fn host_port(url: &str) -> Option<(String, u16)> {
    let s = url.trim();
    let (is_http, rest) = if let Some(r) = s.strip_prefix("https://") {
        (false, r)
    } else if let Some(r) = s.strip_prefix("http://") {
        (true, r)
    } else {
        (false, s)
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
        None => Some((authority.to_string(), if is_http { 80 } else { 443 })),
    }
}

/// 返回 TCP 连接耗时（毫秒）。失败返回错误信息。
pub fn tcp_latency(url: &str) -> Result<u64, String> {
    let (host, port) = host_port(url).ok_or_else(|| "无法解析地址".to_string())?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS 解析失败: {e}"))?
        .next()
        .ok_or_else(|| "无可用地址".to_string())?;
    let start = Instant::now();
    TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("连接失败: {e}"))?;
    Ok(start.elapsed().as_millis() as u64)
}
