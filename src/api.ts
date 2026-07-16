// Tauri 命令封装：前端只跟这里打交道。
import { Channel, invoke } from "@tauri-apps/api/core";
import type { AppType, Provider, Root } from "./types";

export const getConfig = () => invoke<Root>("get_config");

export const saveProvider = (app: AppType, provider: Provider) =>
  invoke<Root>("save_provider", { app, provider });

export type ActiveDeleteMode = "keep" | "restore";

export const deleteProvider = (app: AppType, id: string, activeMode?: ActiveDeleteMode) =>
  invoke<Root>("delete_provider", { app, id, activeMode: activeMode ?? null });

export const switchProvider = (app: AppType, id: string) =>
  invoke<Root>("switch_provider", { app, id });

export const reorderProviders = (app: AppType, order: string[]) =>
  invoke<Root>("reorder_providers", { app, order });

export const importConfig = (rootIn: Root) =>
  invoke<Root>("import_config", { rootIn });

export const exportJson = () => invoke<string>("export_json");

export const saveSettings = (settings: Record<string, unknown>) =>
  invoke<Root>("save_settings", { settings });

/** 「应用到 Claude Code 插件」的文件副作用：写/删 ~/.claude/config.json 的 primaryApiKey。
 *  设置持久化另走 saveSettings。 */
export const setClaudePluginEnabled = (enabled: boolean) =>
  invoke<void>("set_claude_plugin_enabled", { enabled });

/** 「跳过 Claude Code 初次安装确认」的文件副作用：写/删 ~/.claude.json 的 hasCompletedOnboarding。 */
export const setClaudeOnboardingSkip = (enabled: boolean) =>
  invoke<void>("set_claude_onboarding_skip", { enabled });

export const speedtest = (url: string) => invoke<number>("speedtest", { url });

export const fetchModels = (baseUrl: string, apiKey: string, modelsUrl?: string) =>
  invoke<string[]>("fetch_models", { baseUrl, apiKey, modelsUrl: modelsUrl ?? null });

/** 从现有 ~/.claude、~/.codex 反向导入为供应商 */
export const importLive = () => invoke<Root>("import_live");

export interface OriginalConfigStatus {
  captured: boolean;
  capturedAt: number | null;
  claudeHadConfig: boolean;
  codexHadConfig: boolean;
}

/** 首次保存的本机原始配置状态 */
export const originalConfigStatus = () =>
  invoke<OriginalConfigStatus>("original_config_status");

/** 创建并使用系统文件管理器打开 ~/.z-switch/backups */
export const openBackupsFolder = () => invoke<void>("open_backups_folder");

/** 打开指定配置文件所在目录：claude=~/.claude、codex=~/.codex、app=~/.z-switch */
export const openConfigDir = (kind: "claude" | "codex" | "app") =>
  invoke<void>("open_config_dir", { kind });

/** 使用系统默认浏览器打开项目使用帮助 */
export const openHelpDocument = () => invoke<void>("open_help_document");

/** 打开本地路由错误日志目录 */
export const openProxyLogFolder = () => invoke<void>("open_proxy_log_folder");

/** 清空当前及上一份轮转后的本地路由错误日志 */
export const clearProxyErrorLog = () => invoke<void>("clear_proxy_error_log");

/** 恢复指定应用的首次原始配置，并解除当前供应商关联 */
export const restoreOriginal = (app: AppType) =>
  invoke<Root>("restore_original", { app });

export interface ConnResult {
  status: "ok" | "unauthorized" | "unreachable";
  detail: string;
  ms: number | null;
}

/** 连通性测试：地址通不通 / key 对不对 */
export const testConnectivity = (baseUrl: string, apiKey: string) =>
  invoke<ConnResult>("test_connectivity", { baseUrl, apiKey });

export interface StreamTestEvent {
  kind: "started" | "delta";
  text: string | null;
  endpoint: string | null;
}

export interface StreamTestResult {
  text: string;
  firstTokenMs: number;
  totalMs: number;
  streamed: boolean;
}

interface StreamTestInput {
  app: AppType;
  baseUrl: string;
  apiKey: string;
  model: string;
  wireApi: "chat" | "responses";
  apiKeyField?: "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";
}

/** 真实发送一条 `Hi` 并实时接收供应商的流式模型回复 */
export function testStream(
  input: StreamTestInput,
  onEvent: (event: StreamTestEvent) => void,
) {
  const channel = new Channel<StreamTestEvent>(onEvent);
  return invoke<StreamTestResult>("test_stream", {
    ...input,
    apiKeyField: input.apiKeyField ?? null,
    onEvent: channel,
  });
}

/** 设置开机自启（同步系统 + 持久化） */
export const setAutoLaunch = (enabled: boolean) =>
  invoke<Root>("set_auto_launch", { enabled });

export interface ProxyStatus {
  enabled: boolean;
  port: number;
  /** 本地活跃度计数：仅事件次数，不碰请求内容、不上传 */
  inFlight: number;
  total: number;
  lastActivityMs: number;
}

/** 查询本地热切换代理是否在跑 + 端口 + 本地活跃度计数 */
export const proxyStatus = () => invoke<ProxyStatus>("proxy_status");

/** 开启/关闭本地热切换代理（开=起服务并把 live 指向 localhost；关=恢复直连） */
export const setProxyEnabled = (enabled: boolean) =>
  invoke<Root>("set_proxy_enabled", { enabled });
