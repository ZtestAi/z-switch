// 网络代理（出站 HTTP 代理）：供应商单独设置 + 全局默认，单独设置优先。
// 校验 / 生效规则镜像 Rust netproxy.rs；最终以后端校验为准。
import { useRef, useState } from "react";
import { checkNetworkProxy, type ProviderProxySetting } from "./api";
import type { AppType, Provider } from "./types";
import { AlertIcon, CheckIcon, CloseIcon, LinkIcon } from "./Icons";

export type ProxyMode = "global" | "custom" | "off";

/** 编辑中的代理设置（自定义地址可能尚未通过校验） */
export interface ProxyDraft {
  mode: ProxyMode;
  url: string;
}

export const PROXY_PLACEHOLDER = "http://127.0.0.1:7890";

const MODES: [ProxyMode, string][] = [
  ["global", "跟随全局"],
  ["custom", "自定义"],
  ["off", "不使用"],
];

/** 支持网络代理的客户端（Grok 暂不支持） */
export function supportsNetworkProxy(app: AppType): boolean {
  return app === "claude" || app === "codex";
}

/** 校验并规范化代理地址：仅 http(s)，只含协议 / 主机 / 端口；返回去掉尾部 `/` 的地址。 */
export function validateProxyUrl(raw: string): { url: string } | { error: string } {
  const s = raw.trim();
  if (!s) return { error: "代理地址不能为空" };
  if (/[\s'"\\`]/.test(s)) return { error: "代理地址不能包含空格、引号或反斜杠" };
  const scheme = s.match(/^([a-z][a-z0-9+.-]*):\/\//i)?.[1]?.toLowerCase();
  if (!scheme) return { error: `请以 http:// 开头，例如 ${PROXY_PLACEHOLDER}` };
  if (scheme.startsWith("socks")) {
    return { error: "暂不支持 SOCKS 代理（Claude Code 不支持），请填写 HTTP 代理端口；Clash 等工具的混合端口同时支持 HTTP" };
  }
  if (scheme !== "http" && scheme !== "https") {
    return { error: `不支持的代理协议：${scheme}，仅支持 http:// 或 https://` };
  }
  let url: URL;
  try {
    url = new URL(s);
  } catch {
    return { error: `代理地址格式不正确，示例：${PROXY_PLACEHOLDER}` };
  }
  if (!url.hostname) return { error: "代理地址缺少主机名" };
  if ((url.pathname !== "" && url.pathname !== "/") || s.includes("?") || s.includes("#")) {
    return { error: `代理地址只需填写协议、主机和端口，例如 ${PROXY_PLACEHOLDER}` };
  }
  return { url: s.replace(/\/+$/, "") };
}

/** 全局默认代理（未设置或不合法 → null） */
export function globalProxyOf(settings?: Record<string, unknown>): string | null {
  const raw = settings?.networkProxy;
  if (typeof raw !== "string" || !raw.trim()) return null;
  const result = validateProxyUrl(raw);
  return "url" in result ? result.url : null;
}

/** 展示用：隐藏代理地址里的账号密码（user:pass@） */
export function displayProxy(url: string): string {
  return url.replace(/^([a-z][a-z0-9+.-]*:\/\/)[^@/]*@/i, "$1***@");
}

export function proxyDraftOf(provider?: Provider): ProxyDraft {
  const setting = provider?.meta?.proxy as { mode?: unknown; url?: unknown } | undefined;
  if (setting?.mode === "custom") return { mode: "custom", url: typeof setting.url === "string" ? setting.url : "" };
  if (setting?.mode === "off") return { mode: "off", url: "" };
  return { mode: "global", url: "" };
}

/** 草稿 → 存进 meta.proxy 的值（跟随全局 = null，即不存字段） */
export function settingOfDraft(draft: ProxyDraft): { setting: ProviderProxySetting } | { error: string } {
  if (draft.mode === "off") return { setting: { mode: "off" } };
  if (draft.mode === "global") return { setting: null };
  const result = validateProxyUrl(draft.url);
  return "url" in result ? { setting: { mode: "custom", url: result.url } } : result;
}

/** 供应商最终生效的代理：自定义 > 不使用 > 跟随全局 */
export function effectiveProxy(
  app: AppType,
  settings: Record<string, unknown> | undefined,
  provider: Provider,
): string | null {
  if (!supportsNetworkProxy(app)) return null;
  const draft = proxyDraftOf(provider);
  if (draft.mode === "custom") {
    const result = validateProxyUrl(draft.url);
    return "url" in result ? result.url : null;
  }
  if (draft.mode === "off") return null;
  return globalProxyOf(settings);
}

/** 编辑中草稿的生效代理（测试工具用）：自定义地址原样交给后端，由后端给出校验错误 */
export function draftEffectiveProxy(draft: ProxyDraft, globalProxy: string | null): string | null {
  if (draft.mode === "custom") return draft.url.trim() || null;
  if (draft.mode === "off") return null;
  return globalProxy;
}

type CheckState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "ok"; ms: number }
  | { phase: "fail"; msg: string };

/** 检测代理端口是否可连；地址变化时 reset，丢弃过期结果 */
export function useProxyCheck() {
  const [state, setState] = useState<CheckState>({ phase: "idle" });
  const seq = useRef(0);

  async function check(url: string) {
    const id = ++seq.current;
    setState({ phase: "checking" });
    try {
      const ms = await checkNetworkProxy(url);
      if (id === seq.current) setState({ phase: "ok", ms });
    } catch (error) {
      if (id === seq.current) setState({ phase: "fail", msg: String(error) });
    }
  }

  function reset() {
    seq.current += 1;
    setState({ phase: "idle" });
  }

  return { state, check, reset };
}

export function ProxyCheckButton({ state, onClick }: { state: CheckState; onClick: () => void }) {
  return (
    <button
      type="button"
      className={"fetch-btn proxy-check-btn" + (state.phase === "ok" ? " ok" : state.phase === "fail" ? " fail" : "")}
      onClick={onClick}
      disabled={state.phase === "checking"}
      title="检测代理端口能否连接"
    >
      {state.phase === "checking" ? "检测中…"
        : state.phase === "ok" ? <><CheckIcon />可连接</>
          : state.phase === "fail" ? <><AlertIcon />不可用</>
            : <><LinkIcon />检测</>}
    </button>
  );
}

export function ProxyCheckResult({ state }: { state: CheckState }) {
  if (state.phase === "ok") {
    return <div className="hint inline-hint proxy-check-ok"><CheckIcon />代理端口可连接 · {Math.max(1, Math.round(state.ms))}ms</div>;
  }
  if (state.phase === "fail") {
    return <div className="hint inline-hint proxy-check-fail"><AlertIcon />{state.msg}</div>;
  }
  return null;
}

interface ProxyFieldProps {
  app: AppType;
  value: ProxyDraft;
  onChange: (draft: ProxyDraft) => void;
  globalProxy: string | null;
}

/** 供应商网络代理设置：跟随全局 / 自定义 / 不使用 */
export function ProxyField({ app, value, onChange, globalProxy }: ProxyFieldProps) {
  const { state, check, reset } = useProxyCheck();
  const target = draftEffectiveProxy(value, globalProxy);
  const label = app === "claude" ? "Claude Code" : "Codex";

  function update(next: ProxyDraft) {
    reset();
    onChange(next);
  }

  return (
    <div className="field proxy-field">
      <label>
        网络代理
        <span className="desc">{label} 访问该供应商时使用的 HTTP 代理</span>
      </label>
      <div className="proxy-field-row">
        <div className="seg-mini">
          {MODES.map(([mode, text]) => (
            <button key={mode} type="button" className={value.mode === mode ? "on" : ""} onClick={() => update({ ...value, mode })}>
              {text}
            </button>
          ))}
        </div>
        {target && <ProxyCheckButton state={state} onClick={() => check(target)} />}
      </div>
      {value.mode === "custom" && (
        <input
          className="mono proxy-url-input"
          value={value.url}
          onChange={(event) => update({ ...value, url: event.target.value })}
          placeholder={PROXY_PLACEHOLDER}
          spellCheck={false}
          autoComplete="off"
          aria-label="自定义代理地址"
        />
      )}
      <div className="hint">
        {value.mode === "custom"
          ? "仅对此供应商生效，优先于全局默认。仅支持 HTTP 代理（Clash 等工具的混合端口可用）。"
          : value.mode === "off"
            ? "不注入代理，沿用系统环境。"
            : globalProxy
              ? <>跟随全局默认：<span className="mono">{displayProxy(globalProxy)}</span></>
              : "未设置全局默认，不注入代理（可在 设置 → 常规 → 网络代理 中设置）。"}
      </div>
      <ProxyCheckResult state={state} />
    </div>
  );
}

interface ProviderProxyModalProps {
  app: AppType;
  provider: Provider;
  globalProxy: string | null;
  onClose: () => void;
  /** 保存成功返回 true（由调用方关闭弹窗），失败返回 false 保持打开 */
  onSave: (setting: ProviderProxySetting) => Promise<boolean>;
}

/** 单独设置某张卡片（主要用于官方账号卡片）的网络代理 */
export function ProviderProxyModal({ app, provider, globalProxy, onClose, onSave }: ProviderProxyModalProps) {
  const [draft, setDraft] = useState<ProxyDraft>(() => proxyDraftOf(provider));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const label = app === "claude" ? "Claude Code" : "Codex";

  async function save() {
    const result = settingOfDraft(draft);
    if ("error" in result) {
      setError(result.error);
      return;
    }
    setSaving(true);
    if (!(await onSave(result.setting))) setSaving(false);
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal proxy-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>网络代理 · {provider.name}</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          <ProxyField
            app={app}
            value={draft}
            onChange={(next) => {
              setDraft(next);
              setError(null);
            }}
            globalProxy={globalProxy}
          />
          <div className="proxy-modal-note">
            {app === "claude"
              ? "写入 ~/.claude/settings.json 的 HTTPS_PROXY / HTTP_PROXY 环境变量。"
              : "写入 ~/.codex/.env 中由 z-switch 管理的区块，你自己的其它行保持不变。"}
            修改后重启 {label} 生效。
          </div>
          {error && <div className="hint" style={{ color: "var(--danger)" }}>{error}</div>}
        </div>
        <div className="modal-foot">
          <span />
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn" onClick={onClose}>取消</button>
            <button className="btn accent" onClick={save} disabled={saving}>{saving ? "保存中…" : "保存"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
