import { useEffect, useRef, useState } from "react";
import { fetchModels, testStream } from "./api";
import type { AppType, Provider } from "./types";
import { AlertIcon, CloseIcon, MessageIcon, RefreshIcon } from "./Icons";
import { DEFAULT_CODEX_WIRE_API } from "./providerFactory";
import { ModelPicker } from "./ModelPicker";

export interface StreamTestSummary {
  status: "success" | "warning" | "error";
  firstTokenMs?: number;
  totalMs?: number;
}

interface Props {
  app: AppType;
  provider: Provider;
  onClose: () => void;
  onResult: (result: StreamTestSummary) => void;
}

type TestState = {
  phase: "idle" | "running" | "success" | "warning" | "error";
  text: string;
  firstTokenMs?: number;
  totalMs?: number;
  error?: string;
};

function configOf(app: AppType, provider: Provider) {
  const settings = provider.settingsConfig as any;
  if (app === "claude") {
    const env = settings?.env ?? {};
    const apiKeyField = ((provider.meta as any)?.apiKeyField ??
      (env.ANTHROPIC_API_KEY != null ? "ANTHROPIC_API_KEY" : "ANTHROPIC_AUTH_TOKEN")) as
      | "ANTHROPIC_AUTH_TOKEN"
      | "ANTHROPIC_API_KEY";
    return {
      baseUrl: String(env.ANTHROPIC_BASE_URL ?? ""),
      apiKey: String(env[apiKeyField] ?? ""),
      model: String(env.ANTHROPIC_MODEL ?? ""),
      wireApi: "chat" as const,
      apiKeyField,
    };
  }

  const config = String(settings?.config ?? "");

  if (app === "grok") {
    // Grok 单文件 TOML：base 在 models_base_url、key 内嵌 api_key、协议看 api_backend。
    return {
      baseUrl: config.match(/models_base_url\s*=\s*"([^"]+)"/)?.[1] ?? "",
      apiKey: config.match(/api_key\s*=\s*"([^"]+)"/)?.[1] ?? "",
      model: config.match(/^model\s*=\s*"([^"]+)"/m)?.[1] ?? "",
      wireApi: (config.match(/api_backend\s*=\s*"([^"]+)"/)?.[1] === "chat"
        ? "chat"
        : "responses") as "chat" | "responses",
      apiKeyField: undefined,
    };
  }

  return {
    baseUrl: config.match(/base_url\s*=\s*"([^"]+)"/)?.[1] ?? "",
    apiKey: String(settings?.auth?.OPENAI_API_KEY ?? ""),
    model: config.match(/^model\s*=\s*"([^"]+)"/m)?.[1] ?? "",
    wireApi: (((provider.meta as any)?.wireApi ??
      config.match(/wire_api\s*=\s*"([^"]+)"/)?.[1] ??
      DEFAULT_CODEX_WIRE_API) === "responses"
      ? "responses"
      : "chat") as "chat" | "responses",
    apiKeyField: undefined,
  };
}

export default function StreamTestModal({ app, provider, onClose, onResult }: Props) {
  const config = configOf(app, provider);
  const [model, setModel] = useState(config.model);
  const [models, setModels] = useState<string[]>([]);
  const [modelFetch, setModelFetch] = useState<"idle" | "loading" | "success" | "error">("idle");
  const [modelFetchError, setModelFetchError] = useState("");
  const [test, setTest] = useState<TestState>({ phase: "idle", text: "" });
  const mounted = useRef(true);
  const modelRequest = useRef(0);

  useEffect(() => {
    mounted.current = true;
    if (config.baseUrl.trim() && config.apiKey.trim()) void loadModels();
    return () => {
      mounted.current = false;
      modelRequest.current += 1;
    };
  }, []);

  async function loadModels() {
    if (!config.baseUrl.trim() || !config.apiKey.trim()) return;
    const request = ++modelRequest.current;
    setModelFetch("loading");
    setModelFetchError("");
    try {
      const available = await fetchModels(
        config.baseUrl,
        config.apiKey,
        (provider.meta as any)?.modelsUrl,
      );
      if (!mounted.current || request !== modelRequest.current) return;
      setModels(available);
      setModelFetch("success");
    } catch (error) {
      if (!mounted.current || request !== modelRequest.current) return;
      setModelFetch("error");
      setModelFetchError(String(error));
    }
  }

  const missing = [
    !config.baseUrl.trim() && "Base URL",
    !config.apiKey.trim() && "API Key",
    !model.trim() && "模型",
  ].filter(Boolean) as string[];

  async function runTest() {
    if (missing.length || test.phase === "running") return;
    setTest({ phase: "running", text: "" });
    try {
      const result = await testStream(
        {
          app,
          baseUrl: config.baseUrl,
          apiKey: config.apiKey,
          model: model.trim(),
          wireApi: config.wireApi,
          apiKeyField: config.apiKeyField,
        },
        (event) => {
          if (!mounted.current || event.kind !== "delta" || !event.text) return;
          setTest((current) => ({
            ...current,
            text: (current.text + event.text).slice(0, 2_000),
          }));
        },
      );
      if (!mounted.current) return;
      const status = result.streamed ? "success" : "warning";
      setTest({
        phase: status,
        text: result.text,
        firstTokenMs: result.firstTokenMs,
        totalMs: result.totalMs,
      });
      onResult({ status, firstTokenMs: result.firstTokenMs, totalMs: result.totalMs });
    } catch (error) {
      if (!mounted.current) return;
      setTest({ phase: "error", text: "", error: String(error) });
      onResult({ status: "error" });
    }
  }

  const statusText =
    test.phase === "success"
      ? `流式调用成功 · 首字 ${test.firstTokenMs}ms · 总耗时 ${test.totalMs}ms`
      : test.phase === "warning"
        ? `调用成功，但未检测到流式响应 · 总耗时 ${test.totalMs}ms`
        : test.phase === "error"
          ? "真实调用失败"
          : test.text
            ? "正在接收流式回复…"
            : "正在等待模型首段回复…";

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className={"modal stream-test-modal" + (test.phase !== "idle" ? " with-output" : "")} onMouseDown={(event) => event.stopPropagation()}>
        <div className="modal-head">
          <h3>真实流式测试 · {provider.name}</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body stream-test-body">
          <div className="stream-test-facts">
            <span>{app === "claude" ? "Claude Messages" : `${app === "grok" ? "Grok" : "Codex"} ${config.wireApi === "responses" ? "Responses" : "Chat"}`}</span>
          </div>
          <div className="field stream-test-model-field">
            <label>
              测试模型
              <span className="desc">
                {modelFetch === "loading"
                  ? "正在拉取…"
                  : modelFetch === "success"
                    ? `${models.length} 个可用模型`
                    : modelFetch === "error"
                      ? "拉取失败，可手动输入"
                      : "可手动输入"}
              </span>
            </label>
            <div className="stream-test-model-control">
              <ModelPicker
                value={model}
                models={models}
                placeholder="输入或选择本次测试使用的模型"
                ariaLabel="本次测试使用的模型"
                disabled={test.phase === "running"}
                onChange={(value) => {
                  setModel(value);
                  setTest({ phase: "idle", text: "" });
                }}
              />
              <button
                type="button"
                className={"card-icon-btn stream-model-refresh" + (modelFetch === "loading" ? " loading" : "")}
                onClick={loadModels}
                disabled={modelFetch === "loading" || test.phase === "running" || !config.baseUrl.trim() || !config.apiKey.trim()}
                title="重新拉取可用模型"
                aria-label="重新拉取可用模型"
              >
                <RefreshIcon />
              </button>
            </div>
            <div className="hint">
              仅用于本次真实测试，不会修改供应商配置。
              {modelFetchError && <span className="stream-model-error" title={modelFetchError}> 模型列表未更新，可继续手动输入。</span>}
            </div>
          </div>
          <p className="stream-test-note">
            将向当前供应商发送“Hi”，最多输出 32 tokens，可能产生极少量费用。测试内容、回复和密钥不会保存。
          </p>
          {missing.length > 0 && (
            <div className="action-status warning">
              <span className="action-status-icon"><AlertIcon /></span>
              <div className="action-status-copy">
                <strong>配置不完整</strong>
                <span>请先编辑并补充：{missing.join("、")}</span>
              </div>
            </div>
          )}
          {test.phase !== "idle" && (
            <div className={`real-test-output ${test.phase}`} aria-live="polite">
              <div className="real-test-meta">
                <span className="real-test-dot" />
                {statusText}
              </div>
              <div className="real-test-response">
                {test.phase === "error" ? test.error : test.text || "…"}
              </div>
            </div>
          )}
        </div>
        <div className="modal-foot">
          <span />
          <div className="stream-test-actions">
            <button className="btn" onClick={onClose}>关闭</button>
            <button
              className="btn accent"
              onClick={runTest}
              disabled={missing.length > 0 || test.phase === "running"}
            >
              {test.phase === "running" ? "测试中…" : <><MessageIcon />{test.phase === "idle" ? "发送 Hi" : "再次测试"}</>}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
