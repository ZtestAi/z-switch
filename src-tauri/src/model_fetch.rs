//! 拉取供应商可用模型列表（OpenAI 兼容 GET /v1/models）。
//! 试多个候选端点：优先 models_url 覆写；否则对 base_url 生成候选，
//! 含「剥离 Anthropic 兼容子路径」兜底（如 …/anthropic → 根域 /v1/models）。
use serde::Deserialize;

#[derive(Deserialize)]
struct ModelsResp {
    data: Vec<ModelEntry>,
}
#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// 生成候选 models URL 列表。
fn candidates(base_url: &str, models_url: Option<&str>) -> Vec<String> {
    if let Some(u) = models_url {
        let u = u.trim();
        if !u.is_empty() {
            return vec![u.to_string()];
        }
    }
    let b = base_url.trim().trim_end_matches('/');
    if b.is_empty() {
        return vec![];
    }
    let mut out = vec![format!("{b}/v1/models"), format!("{b}/models")];
    // 剥离已知兼容子路径，回退到根域的 /v1/models
    for suffix in [
        "/anthropic",
        "/v1",
        "/compatible",
        "/api/anthropic",
        "/openai",
    ] {
        if let Some(root) = b.strip_suffix(suffix) {
            let root = root.trim_end_matches('/');
            if !root.is_empty() {
                out.push(format!("{root}/v1/models"));
                out.push(format!("{root}/models"));
            }
        }
    }
    out.dedup();
    out
}

/// 拉取模型 id 列表。任一候选返回合法 {data:[{id}]} 即成功。
pub async fn fetch_models(
    base_url: &str,
    api_key: &str,
    models_url: Option<&str>,
) -> Result<Vec<String>, String> {
    let urls = candidates(base_url, models_url);
    if urls.is_empty() {
        return Err("请先填写 Base URL".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let mut last_err = String::from("无可用端点");
    for url in urls {
        let mut req = client.get(&url);
        if !api_key.trim().is_empty() {
            req = req.bearer_auth(api_key.trim());
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    last_err = format!("{url} 返回 {status}");
                    continue;
                }
                match resp.json::<ModelsResp>().await {
                    Ok(m) if !m.data.is_empty() => {
                        let mut ids: Vec<String> = m.data.into_iter().map(|e| e.id).collect();
                        ids.sort();
                        ids.dedup();
                        return Ok(ids);
                    }
                    Ok(_) => {
                        last_err = format!("{url} 返回空列表");
                    }
                    Err(e) => {
                        last_err = format!("{url} 解析失败: {e}");
                    }
                }
            }
            Err(e) => {
                last_err = format!("{url} 请求失败: {e}");
            }
        }
    }
    Err(last_err)
}
