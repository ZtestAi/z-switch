import { useState } from "react";
import type { AppType, Provider } from "./types";
import { fetchModels, speedtest, testConnectivity } from "./api";
import {
  buildClaudeProvider,
  buildCodexProvider,
  DEFAULT_CODEX_WIRE_API,
  inferWireApi,
  inferClaudeKeyField,
  needsV1Suffix,
} from "./providerFactory";
import {
  AlertIcon,
  BoltIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CloseIcon,
  DownloadIcon,
  LinkIcon,
  PlusIcon,
  TrashIcon,
} from "./Icons";
import { ModelPicker } from "./ModelPicker";

function slug(s: string): string {
  const t = s.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return t || `custom-${Date.now().toString(36)}`;
}

function uniqueId(base: string, existing: string[], keep?: string): string {
  if (base === keep) return base;
  let id = base;
  let n = 2;
  while (existing.includes(id)) id = `${base}-${n++}`;
  return id;
}

interface SecretInputProps {
  value: string;
  onChange: (value: string) => void;
}

function SecretInput({ value, onChange }: SecretInputProps) {
  const [visible, setVisible] = useState(false);
  return (
    <div className="secret-input">
      <input
        className="mono"
        type={visible ? "text" : "password"}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder="sk-…"
        autoComplete="off"
      />
      <button
        type="button"
        className="secret-toggle"
        aria-pressed={visible}
        onClick={() => setVisible((current) => !current)}
      >
        {visible ? "隐藏" : "显示"}
      </button>
    </div>
  );
}

interface ActionStatusProps {
  tone: "success" | "warning" | "danger";
  title: string;
  detail: string;
}

function ActionStatus({ tone, title, detail }: ActionStatusProps) {
  return (
    <div className={`action-status ${tone}`} role="status">
      <span className="action-status-icon">{tone === "success" ? <CheckIcon /> : <AlertIcon />}</span>
      <span className="action-status-copy">
        <strong>{title}</strong>
        <span>{detail}</span>
      </span>
    </div>
  );
}

// Claude 表单直接管理的 env 键（其余进「自定义 env」）
const CLAUDE_KNOWN = new Set([
  "ANTHROPIC_BASE_URL",
  "ANTHROPIC_AUTH_TOKEN",
  "ANTHROPIC_API_KEY",
  "ANTHROPIC_MODEL",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL",
  "ANTHROPIC_DEFAULT_FABLE_MODEL",
  "API_TIMEOUT_MS",
  "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
  "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
]);

interface Row {
  key: string;
  value: string;
}

interface Props {
  app: AppType;
  initial?: Provider;
  existingIds: string[];
  onClose: () => void;
  onSave: (p: Provider) => void;
}

export default function ProviderModal({ app, initial, existingIds, onClose, onSave }: Props) {
  const editing = !!initial;
  const initEnv: Record<string, any> = (initial?.settingsConfig as any)?.env ?? {};
  const initCfg: any = (initial?.settingsConfig as any) ?? {};
  const initConfigText: string = initCfg.config ?? "";

  const [name, setName] = useState(initial?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(
    initEnv.ANTHROPIC_BASE_URL ??
      (initConfigText ? initConfigText.match(/base_url\s*=\s*"([^"]+)"/)?.[1] ?? "" : ""),
  );
  const [keyField, setKeyField] = useState<"ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY">(
    (initial?.meta as any)?.apiKeyField ?? (initEnv.ANTHROPIC_API_KEY != null ? "ANTHROPIC_API_KEY" : "ANTHROPIC_AUTH_TOKEN"),
  );
  const [apiKey, setApiKey] = useState(
    initEnv.ANTHROPIC_AUTH_TOKEN ?? initEnv.ANTHROPIC_API_KEY ?? initCfg.auth?.OPENAI_API_KEY ?? "",
  );
  const [model, setModel] = useState(
    initEnv.ANTHROPIC_MODEL ??
      (initConfigText ? initConfigText.match(/^model\s*=\s*"([^"]+)"/m)?.[1] ?? "" : ""),
  );
  const [haiku, setHaiku] = useState(initEnv.ANTHROPIC_DEFAULT_HAIKU_MODEL ?? "");
  const [sonnet, setSonnet] = useState(initEnv.ANTHROPIC_DEFAULT_SONNET_MODEL ?? "");
  const [opus, setOpus] = useState(initEnv.ANTHROPIC_DEFAULT_OPUS_MODEL ?? "");
  const [fable, setFable] = useState(initEnv.ANTHROPIC_DEFAULT_FABLE_MODEL ?? "");

  // Claude 高级
  const [apiTimeout, setApiTimeout] = useState(
    initEnv.API_TIMEOUT_MS != null ? String(initEnv.API_TIMEOUT_MS) : "",
  );
  const [maxOutput, setMaxOutput] = useState(
    initEnv.CLAUDE_CODE_MAX_OUTPUT_TOKENS != null ? String(initEnv.CLAUDE_CODE_MAX_OUTPUT_TOKENS) : "",
  );
  const [disableNon, setDisableNon] = useState(!!initEnv.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC);
  const [rows, setRows] = useState<Row[]>(
    Object.entries(initEnv)
      .filter(([k]) => !CLAUDE_KNOWN.has(k))
      .map(([k, v]) => ({ key: k, value: String(v) })),
  );

  // Codex
  const [wireApi, setWireApi] = useState<"chat" | "responses">(
    (initial?.meta as any)?.wireApi ??
      (initConfigText.match(/wire_api\s*=\s*"([^"]+)"/)?.[1] as any) ??
      DEFAULT_CODEX_WIRE_API,
  );
  const [effort, setEffort] = useState<"low" | "medium" | "high">(
    (initConfigText.match(/model_reasoning_effort\s*=\s*"([^"]+)"/)?.[1] as any) ?? "high",
  );
  const [disableRespStore, setDisableRespStore] = useState(
    initConfigText ? /disable_response_storage\s*=\s*true/.test(initConfigText) : true,
  );
  const [requiresAuth, setRequiresAuth] = useState(
    initConfigText ? /requires_openai_auth\s*=\s*true/.test(initConfigText) : false,
  );
  const [ctxWindow, setCtxWindow] = useState(
    initConfigText.match(/model_context_window\s*=\s*(\d+)/)?.[1] ?? "",
  );

  const [showAdv, setShowAdv] = useState(false);
  const [jsonMode, setJsonMode] = useState(false);
  const [jsonText, setJsonText] = useState(() => JSON.stringify(initial?.settingsConfig ?? {}, null, 2));
  const [error, setError] = useState<string | null>(null);

  const [fetchedModels, setFetchedModels] = useState<string[]>([]);
  const [fetching, setFetching] = useState(false);
  const [fetchMsg, setFetchMsg] = useState<string | null>(null);
  const [fetchState, setFetchState] = useState<"idle" | "success" | "error">("idle");

  // 一键获取区：连通测试 + 测速状态
  const [conn, setConn] = useState<"idle" | "testing" | "ok" | "fail" | "unauth">("idle");
  const [connMsg, setConnMsg] = useState<string | null>(null);
  const [speed, setSpeed] = useState<{ ms?: number; loading?: boolean; err?: boolean }>({});
  const [inferHint, setInferHint] = useState<string | null>(null);
  const [v1Hint, setV1Hint] = useState(false);

  // baseUrl 变化时的智能推断（填一个补一片）：不打断用户，仅在能判断时静默修正 + 给提示
  function onBaseUrlChange(v: string) {
    setBaseUrl(v);
    setConn("idle");
    setConnMsg(null);
    setFetchedModels([]);
    setFetchMsg(null);
    setFetchState("idle");
    if (app === "codex") {
      const w = inferWireApi(v);
      if (w && w !== wireApi) {
        setWireApi(w);
        setInferHint(`已按厂商自动选择 wire_api=${w}`);
      } else {
        setInferHint(null);
      }
      setV1Hint(needsV1Suffix(v)); // Codex 疑似缺 /v1 → 非阻断提醒
      return;
    }
    setV1Hint(false);
    const kf = inferClaudeKeyField(v);
    if (kf && kf !== keyField) {
      setKeyField(kf);
      setInferHint(`地址含 /anthropic，已自动选 ${kf}`);
      return;
    }
    setInferHint(null);
  }

  // 一键补全 /v1 后缀
  function applyV1Suffix() {
    setBaseUrl(baseUrl.replace(/\/+$/, "") + "/v1");
    setV1Hint(false);
  }

  async function doTestConn() {
    if (!baseUrl.trim()) return;
    setConn("testing");
    setConnMsg(null);
    try {
      const r = await testConnectivity(baseUrl, apiKey);
      if (r.status === "ok") {
        setConn("ok");
        setConnMsg(r.ms != null ? `${r.detail} · ${r.ms}ms` : r.detail);
      } else if (r.status === "unauthorized") {
        setConn("unauth");
        setConnMsg(r.detail);
      } else {
        setConn("fail");
        setConnMsg(r.detail);
      }
    } catch (e) {
      setConn("fail");
      setConnMsg(String(e));
    }
  }

  async function doSpeedtest() {
    if (!baseUrl.trim()) return;
    setSpeed({ loading: true });
    try {
      const ms = await speedtest(baseUrl);
      setSpeed({ ms });
    } catch {
      setSpeed({ err: true });
    }
  }

  async function doFetchModels() {
    setFetching(true);
    setFetchMsg(null);
    setFetchState("idle");
    try {
      const models = await fetchModels(baseUrl, apiKey, (initial?.meta as any)?.modelsUrl);
      setFetchedModels(models);
      setFetchState("success");
      setFetchMsg(`已载入 ${models.length} 个模型，可在模型输入框中搜索或选择。`);
    } catch (e) {
      setFetchedModels([]);
      setFetchState("error");
      setFetchMsg(String(e));
    } finally {
      setFetching(false);
    }
  }

  function renderTestTools() {
    const missingEndpoint = !baseUrl.trim();
    const missingCredentials = !apiKey.trim();
    const credentialActionDisabled = missingEndpoint || missingCredentials;

    return (
      <>
        <div className="fetch-bar">
          <button
            className={"fetch-btn" + (conn === "ok" ? " ok" : conn === "fail" ? " fail" : conn === "unauth" ? " warn" : "")}
            onClick={doTestConn}
            disabled={conn === "testing" || credentialActionDisabled}
            title={credentialActionDisabled ? "请先填写 Base URL 和 API Key" : "测试地址与密钥是否可用"}
          >
            {conn === "testing" ? "测试中…" : conn === "ok" ? <><CheckIcon />已连通</> : conn === "unauth" ? <><AlertIcon />Key 无效</> : conn === "fail" ? <><AlertIcon />不通</> : <><LinkIcon />测试连通</>}
          </button>
          <button
            className="fetch-btn live"
            onClick={doFetchModels}
            disabled={fetching || credentialActionDisabled}
            title={credentialActionDisabled ? "请先填写 Base URL 和 API Key" : "从供应商拉取模型列表"}
          >
            {fetching ? "拉取中…" : <><DownloadIcon />拉取模型</>}
          </button>
          <button
            className="fetch-btn"
            onClick={doSpeedtest}
            disabled={speed.loading || missingEndpoint}
            title={missingEndpoint ? "请先填写 Base URL" : "测试端点网络延迟"}
          >
            {speed.loading ? "测速中…" : speed.err ? <><BoltIcon />超时</> : speed.ms != null ? <><BoltIcon />{speed.ms}ms</> : <><BoltIcon />测速</>}
          </button>
        </div>
        {(connMsg || fetchMsg) && (
          <div className="action-feedback">
            {connMsg && (
              <ActionStatus
                tone={conn === "ok" ? "success" : conn === "unauth" ? "warning" : "danger"}
                title={conn === "ok" ? "连接可用" : conn === "unauth" ? "API Key 被拒绝" : "无法连接"}
                detail={connMsg}
              />
            )}
            {fetchMsg && (
              <ActionStatus
                tone={fetchState === "success" ? "success" : "danger"}
                title={fetchState === "success" ? "模型列表已更新" : "模型拉取失败"}
                detail={fetchMsg}
              />
            )}
          </div>
        )}
      </>
    );
  }

  function handleSave() {
    if (!name.trim()) {
      setError("请填写名称");
      return;
    }
    const base = editing ? initial!.id : slug(name);
    const id = uniqueId(base, existingIds, initial?.id);

    let provider: Provider;
    if (jsonMode) {
      try {
        provider = {
          id,
          name: name.trim(),
          category: initial?.category ?? "custom",
          settingsConfig: JSON.parse(jsonText),
          meta: { ...(initial?.meta ?? {}) },
        };
      } catch (e) {
        setError("JSON 解析失败：" + String(e));
        return;
      }
    } else if (app === "claude") {
      const extraEnv: Record<string, string | number> = {};
      if (apiTimeout.trim()) extraEnv.API_TIMEOUT_MS = apiTimeout.trim();
      if (maxOutput.trim()) extraEnv.CLAUDE_CODE_MAX_OUTPUT_TOKENS = maxOutput.trim();
      if (disableNon) extraEnv.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = 1;
      for (const r of rows) {
        const k = r.key.trim();
        if (k && !CLAUDE_KNOWN.has(k)) extraEnv[k] = r.value;
      }
      provider = buildClaudeProvider(
        { id, name: name.trim(), category: initial?.category ?? "custom", baseUrl, apiKeyField: keyField, model, haiku, sonnet, opus, fable, extraEnv },
        apiKey,
      );
    } else {
      provider = buildCodexProvider(
        {
          id,
          name: name.trim(),
          category: initial?.category ?? "custom",
          baseUrl,
          model,
          wireApi,
          reasoningEffort: effort,
          disableResponseStorage: disableRespStore,
          requiresOpenaiAuth: requiresAuth,
          contextWindow: ctxWindow.trim() ? Number(ctxWindow.trim()) : undefined,
        },
        apiKey,
      );
    }
    onSave(provider);
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal provider-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{editing ? "编辑供应商" : "添加供应商"} · {app === "claude" ? "Claude Code" : "Codex"}</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>

        <div className="modal-body">
          <div className="mode-tabs">
            <button className={jsonMode ? "" : "on"} onClick={() => setJsonMode(false)}>表单</button>
            <button className={jsonMode ? "on" : ""} onClick={() => setJsonMode(true)}>JSON</button>
          </div>

          {jsonMode ? (
            <div className="field">
              <label>settingsConfig（{app === "claude" ? "{ env }" : "{ auth, config }"}）</label>
              <textarea className="mono" value={jsonText} onChange={(e) => setJsonText(e.target.value)} style={{ minHeight: 220 }} />
              <div className="hint">保存时原子写入，切换时保留 settings.json 中你的其它字段。</div>
            </div>
          ) : (
            <>
              <div className="field">
                <label>名称 <span className="req">必填</span></label>
                <input value={name} onChange={(e) => setName(e.target.value)} placeholder="例如 DeepSeek" />
              </div>
              <div className="field">
                <label>
                  Base URL <span className="req">必填</span>
                  <span className="desc">中转站接口地址，常以 /v1 或 /anthropic 结尾</span>
                </label>
                <input className="mono" value={baseUrl} onChange={(e) => onBaseUrlChange(e.target.value)} placeholder="https://…" />
                {inferHint && <div className="hint inline-hint" style={{ color: "var(--accent-text)" }}><CheckIcon />{inferHint}</div>}
                {v1Hint && (
                  <div className="hint inline-hint" style={{ color: "var(--warning)" }}>
                    <AlertIcon />
                    Codex 供应商地址通常需以 /v1 结尾
                    <button type="button" className="hint-action" onClick={applyV1Suffix}>点此补全</button>
                  </div>
                )}
              </div>

              {app === "claude" ? (
                <>
                  <div className="field">
                    <label>
                      API Key 字段
                    </label>
                    <div className="seg-mini" style={{ marginBottom: 8 }}>
                      <button className={keyField === "ANTHROPIC_AUTH_TOKEN" ? "on" : ""} onClick={() => setKeyField("ANTHROPIC_AUTH_TOKEN")}>ANTHROPIC_AUTH_TOKEN</button>
                      <button className={keyField === "ANTHROPIC_API_KEY" ? "on" : ""} onClick={() => setKeyField("ANTHROPIC_API_KEY")}>ANTHROPIC_API_KEY</button>
                    </div>
                    <SecretInput value={apiKey} onChange={setApiKey} />
                  </div>
                  {renderTestTools()}
                  <div className="field">
                    <label>主模型</label>
                    <ModelPicker
                      value={model}
                      models={fetchedModels}
                      ariaLabel="主模型"
                      onChange={setModel}
                    />
                  </div>
                  <div className="field">
                    <label>默认模型（按级别覆盖，可留空）</label>
                    <div className="lvl-row">
                      <span className="lvl">Haiku</span>
                      <ModelPicker
                        value={haiku}
                        models={fetchedModels}
                        ariaLabel="Haiku 默认模型"
                        placeholder="轻量 / 快速模型"
                        onChange={setHaiku}
                      />
                    </div>
                    <div className="lvl-row">
                      <span className="lvl">Sonnet</span>
                      <ModelPicker
                        value={sonnet}
                        models={fetchedModels}
                        ariaLabel="Sonnet 默认模型"
                        placeholder="默认主力模型"
                        onChange={setSonnet}
                      />
                    </div>
                    <div className="lvl-row">
                      <span className="lvl">Opus</span>
                      <ModelPicker
                        value={opus}
                        models={fetchedModels}
                        ariaLabel="Opus 默认模型"
                        placeholder="最强 / 复杂任务模型"
                        onChange={setOpus}
                      />
                    </div>
                    <div className="lvl-row">
                      <span className="lvl">Fable</span>
                      <ModelPicker
                        value={fable}
                        models={fetchedModels}
                        ariaLabel="Fable 默认模型"
                        placeholder="前沿 / 自主任务模型"
                        onChange={setFable}
                      />
                    </div>
                  </div>
                </>
              ) : (
                <>
                  <div className="field">
                    <label>
                      API Key（OPENAI_API_KEY）
                    </label>
                    <SecretInput value={apiKey} onChange={setApiKey} />
                  </div>
                  {renderTestTools()}
                  <div className="field">
                    <label>模型</label>
                    <ModelPicker
                      value={model}
                      models={fetchedModels}
                      ariaLabel="Codex 模型"
                      onChange={setModel}
                    />
                  </div>
                  <div className="field">
                    <label>wire_api</label>
                    <div className="seg-mini">
                      <button className={wireApi === "chat" ? "on" : ""} onClick={() => setWireApi("chat")}>chat</button>
                      <button className={wireApi === "responses" ? "on" : ""} onClick={() => setWireApi("responses")}>responses</button>
                    </div>
                    <div className="hint">原生支持 Responses 的用 responses，OpenAI-chat 兼容的用 chat。</div>
                  </div>
                </>
              )}

              <button className="adv-toggle" onClick={() => setShowAdv((v) => !v)}>
                {showAdv ? <ChevronDownIcon /> : <ChevronRightIcon />}高级选项
              </button>

              {showAdv && app === "claude" && (
                <div className="adv">
                  <div className="grid2">
                    <div className="field">
                      <label>API_TIMEOUT_MS</label>
                      <input className="mono" value={apiTimeout} onChange={(e) => setApiTimeout(e.target.value)} placeholder="如 3000000" />
                    </div>
                    <div className="field">
                      <label>MAX_OUTPUT_TOKENS</label>
                      <input className="mono" value={maxOutput} onChange={(e) => setMaxOutput(e.target.value)} placeholder="如 6000" />
                    </div>
                  </div>
                  <div className="set-row" style={{ boxShadow: "none", marginBottom: 10 }}>
                    <div>
                      <div className="l">禁用非必要流量</div>
                      <div className="d">CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC</div>
                    </div>
                    <button type="button" className={"switch" + (disableNon ? " on" : "")} role="switch" aria-checked={disableNon} aria-label="禁用非必要流量" onClick={() => setDisableNon((v) => !v)} />
                  </div>
                  <div className="field">
                    <label>自定义 env（其它环境变量）</label>
                    {rows.map((r, i) => (
                      <div className="kv-row" key={i}>
                        <input className="mono" placeholder="KEY" value={r.key}
                          onChange={(e) => setRows((rs) => rs.map((x, j) => (j === i ? { ...x, key: e.target.value } : x)))} />
                        <input className="mono" placeholder="value" value={r.value}
                          onChange={(e) => setRows((rs) => rs.map((x, j) => (j === i ? { ...x, value: e.target.value } : x)))} />
                        <button className="card-icon-btn danger-ghost" onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))} aria-label={`删除第 ${i + 1} 行`}><TrashIcon /></button>
                      </div>
                    ))}
                    <button className="adv-toggle" onClick={() => setRows((rs) => [...rs, { key: "", value: "" }])}><PlusIcon />添加一行</button>
                  </div>
                </div>
              )}

              {showAdv && app === "codex" && (
                <div className="adv">
                  <div className="field">
                    <label>model_reasoning_effort</label>
                    <div className="seg-mini">
                      {(["low", "medium", "high"] as const).map((v) => (
                        <button key={v} className={effort === v ? "on" : ""} onClick={() => setEffort(v)}>{v}</button>
                      ))}
                    </div>
                  </div>
                  <div className="set-row" style={{ boxShadow: "none", marginBottom: 8 }}>
                    <div>
                      <div className="l">disable_response_storage</div>
                      <div className="d">不在服务端保存响应（默认开）</div>
                    </div>
                    <button type="button" className={"switch" + (disableRespStore ? " on" : "")} role="switch" aria-checked={disableRespStore} aria-label="禁用响应存储" onClick={() => setDisableRespStore((v) => !v)} />
                  </div>
                  <div className="set-row" style={{ boxShadow: "none", marginBottom: 10 }}>
                    <div>
                      <div className="l">requires_openai_auth</div>
                      <div className="d">仅特殊兼容站点需要；普通 API Key 中转请关闭</div>
                    </div>
                    <button type="button" className={"switch" + (requiresAuth ? " on" : "")} role="switch" aria-checked={requiresAuth} aria-label="使用 OpenAI 认证" onClick={() => setRequiresAuth((v) => !v)} />
                  </div>
                  <div className="field">
                    <label>model_context_window（可选）</label>
                    <input className="mono" value={ctxWindow} onChange={(e) => setCtxWindow(e.target.value)} placeholder="如 262144" />
                  </div>
                </div>
              )}
            </>
          )}

          {error && <div className="hint" style={{ color: "var(--danger)" }}>{error}</div>}
        </div>

        <div className="modal-foot">
          <span />
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn" onClick={onClose}>取消</button>
            <button className="btn accent" onClick={handleSave}>{editing ? "保存" : "保存并添加"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
