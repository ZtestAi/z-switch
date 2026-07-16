# z-switch 深链接入规范（zswitch://import）

网站可通过深链让用户一键把供应商添加进 z-switch。用户点击链接后，z-switch 会**弹出确认框**预览内容，用户确认后才写入——不会静默导入，也**绝不覆盖**已有供应商。

> **命名即身份 / 同名不覆盖**：以 `name` 为身份。异名 → 各自成卡；同名 → **新增一张并给显示名加数字后缀**（`满血` → `满血 2`），确认框会提示。因此不同分组（如 `满血` / `其他渠道`、或同站不同模型）直接用不同名称即可并存；就算真的重名，也不会丢掉已有配置。

> 生成器：见同目录 [`deeplink.html`](./deeplink.html)（可直接部署到网站，或参考其构造逻辑）。

## URL 格式

```
zswitch://import?app=<claude|codex>&name=<名称>&baseUrl=<http(s) 地址>&key=<密钥>&model=<模型>...
```

- scheme：`zswitch`
- host：`import`
- 参数：见下表，值需 `encodeURIComponent` 编码。

## 参数表

| 参数 | 适用 | 必填 | 说明 |
|------|------|------|------|
| `app` | 通用 | 否 | `claude`（默认）或 `codex` |
| `name` | 通用 | 建议 | 卡片显示名；缺省为「导入的供应商」 |
| `baseUrl` | 通用 | **是** | 接入点，**必须是 http(s)**，否则链接被拒绝 |
| `key` | 通用 | 否 | API Key（明文）。写入 Claude 的 `ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY` 或 Codex 的 `OPENAI_API_KEY` |
| `keyB64` | 通用 | 否 | base64 编码的 Key（与 `key` 二选一，`key` 优先）。仅编码非加密 |
| `model` | 通用 | 否 | 主模型；Codex 缺省 `gpt-5.5` |
| `apiKeyField` | claude | 否 | `ANTHROPIC_AUTH_TOKEN`（默认）或 `ANTHROPIC_API_KEY` |
| `wireApi` | codex | 否 | `responses`（默认）或 `chat` |
| `haiku` / `sonnet` / `opus` / `fable` | claude | 否 | 各级别默认模型 |

## 示例

Claude：
```
zswitch://import?app=claude&name=%E6%88%91%E7%9A%84%E4%B8%AD%E8%BD%AC&baseUrl=https%3A%2F%2Fapi.example.com%2Fv1&key=sk-abcd1234&model=claude-sonnet-4-6
```

Codex：
```
zswitch://import?app=codex&name=MyCodex&baseUrl=https%3A%2F%2Fapi.example.com%2Fv1&keyB64=c2steHh4&wireApi=responses
```

## 安全须知

- 链接可能含 API Key；`keyB64` 只是编码、**非加密**，无法阻止解码。请仅在可信页面/HTTPS 下分发。
- z-switch 侧的保护：接入点强制 http(s)、导入前必经确认框（Key 脱敏预览）、**绝不覆盖**已有供应商（同名自动加后缀新增）、Key 不写入日志/toast。
