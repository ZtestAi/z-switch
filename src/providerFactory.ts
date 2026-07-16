import type { Provider } from "./types";

export const DEFAULT_CODEX_WIRE_API = "responses" as const;

export interface ClaudeProviderInput {
  id: string;
  name: string;
  category: string;
  baseUrl: string;
  apiKeyField: "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";
  model?: string;
  haiku?: string;
  sonnet?: string;
  opus?: string;
  fable?: string;
  extraEnv?: Record<string, string | number>;
}

export interface CodexProviderInput {
  id: string;
  name: string;
  category: string;
  baseUrl: string;
  model: string;
  wireApi: "responses" | "chat";
  reasoningEffort?: "low" | "medium" | "high";
  disableResponseStorage?: boolean;
  requiresOpenaiAuth?: boolean;
  contextWindow?: number;
}

export function inferWireApi(baseUrl: string): "responses" | "chat" | null {
  const url = baseUrl.toLowerCase();
  if (!url) return null;

  const responsesHosts = [
    "xiaomimimo.com",
    "minimaxi.com",
    "longcat.chat",
    "dashscope.aliyuncs.com",
    "/api/v3",
  ];
  if (responsesHosts.some((host) => url.includes(host))) return "responses";

  const chatHosts = [
    "deepseek.com",
    "bigmodel.cn",
    "moonshot.cn",
    "stepfun.com",
    "qianfan.baidubce.com",
  ];
  if (chatHosts.some((host) => url.includes(host))) return "chat";
  return null;
}

/**
 * Codex base URL 疑似缺少 /v1 版本后缀时返回 true（仅用于非阻断提醒）。
 * 已含任意版本段（/v1、/v2、/api/v3 等）或还不是合法 http(s) 地址时不提醒。
 */
export function needsV1Suffix(baseUrl: string): boolean {
  const raw = baseUrl.trim();
  if (!raw) return false;
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return false; // 还没输完整地址，不打扰
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return false;
  if (!url.hostname) return false;
  if (/\/v\d+(\/|$)/.test(url.pathname)) return false; // 已有版本段
  return true;
}

/**
 * Claude 1M 长上下文标记：给模型名追加 `[1M]` 后缀，Claude Code 据此对该
 * 模型启用 100 万 token 上下文（照 cc-switch 原版做法——不写 ANTHROPIC_BETA
 * 之类 env，标记就内嵌在模型名字符串里）。仅 sonnet/opus/fable 三档适用。
 */
export const CLAUDE_ONE_M_MARKER = "[1M]";

export function hasClaudeOneMMarker(model: string): boolean {
  return model.trimEnd().toLowerCase().endsWith("[1m]");
}

export function stripClaudeOneMMarker(model: string): string {
  const trimmed = model.trimEnd();
  if (!trimmed.toLowerCase().endsWith("[1m]")) return model;
  return trimmed.slice(0, -CLAUDE_ONE_M_MARKER.length).trimEnd();
}

export function setClaudeOneMMarker(model: string, enabled: boolean): string {
  const base = stripClaudeOneMMarker(model).trim();
  if (!base) return ""; // 空模型名不产生只有 [1M] 的脏值
  return enabled ? `${base}${CLAUDE_ONE_M_MARKER}` : base;
}

export function inferClaudeKeyField(
  baseUrl: string,
): "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY" | null {
  if (baseUrl.toLowerCase().includes("/anthropic")) {
    return "ANTHROPIC_AUTH_TOKEN";
  }
  return null;
}

export function buildClaudeProvider(
  input: ClaudeProviderInput,
  apiKey: string,
): Provider {
  const env: Record<string, string | number> = {};
  if (input.baseUrl) env.ANTHROPIC_BASE_URL = input.baseUrl;
  env[input.apiKeyField] = apiKey;
  if (input.model) env.ANTHROPIC_MODEL = input.model;
  if (input.haiku) env.ANTHROPIC_DEFAULT_HAIKU_MODEL = input.haiku;
  if (input.sonnet) env.ANTHROPIC_DEFAULT_SONNET_MODEL = input.sonnet;
  if (input.opus) env.ANTHROPIC_DEFAULT_OPUS_MODEL = input.opus;
  if (input.fable) env.ANTHROPIC_DEFAULT_FABLE_MODEL = input.fable;
  Object.assign(env, input.extraEnv ?? {});

  return {
    id: input.id,
    name: input.name,
    category: input.category,
    settingsConfig: { env },
    meta: { apiKeyField: input.apiKeyField },
  };
}

const tomlString = (value: string) => JSON.stringify(value);

export function buildCodexProvider(
  input: CodexProviderInput,
  apiKey: string,
): Provider {
  const lines = [
    'model_provider = "custom"',
    `model = ${tomlString(input.model)}`,
    `model_reasoning_effort = ${tomlString(input.reasoningEffort ?? "high")}`,
    `disable_response_storage = ${input.disableResponseStorage ?? true}`,
    "",
    "[model_providers.custom]",
    `name = ${tomlString(input.name)}`,
    `base_url = ${tomlString(input.baseUrl)}`,
    `wire_api = ${tomlString(input.wireApi)}`,
    `requires_openai_auth = ${input.requiresOpenaiAuth ?? false}`,
  ];
  if (input.contextWindow && input.contextWindow > 0) {
    lines.push(`model_context_window = ${input.contextWindow}`);
  }

  return {
    id: input.id,
    name: input.name,
    category: input.category,
    settingsConfig: {
      auth: { OPENAI_API_KEY: apiKey },
      config: lines.join("\n") + "\n",
    },
    meta: { wireApi: input.wireApi },
  };
}
