import { useEffect, useMemo, useRef, useState } from "react";
import type { AppType, Provider, Root } from "./types";
import {
  getConfig,
  switchProvider,
  deleteProvider,
  saveProvider,
  reorderProviders,
  saveSettings,
  speedtest,
  importLive,
  originalConfigStatus,
  openHelpDocument,
  restoreOriginal,
  restoreOfficialBaseline,
  proxyStatus,
  setAppRouting,
  type ActiveDeleteMode,
  type OriginalConfigStatus,
  type ProxyStatus,
} from "./api";
import ProviderModal from "./ProviderModal";
import SettingsModal from "./SettingsModal";
import ImportExportModal from "./ImportExportModal";
import CcswitchImportModal from "./CcswitchImportModal";
import StreamTestModal, { type StreamTestSummary } from "./StreamTestModal";
import WindowControls from "./Titlebar";
import ConfirmDialog from "./ConfirmDialog";
import DeepLinkImportDialog, { type PendingImport } from "./DeepLinkImportDialog";
import Toasts, { type Toast, type ToastKind } from "./Toasts";
import { buildClaudeProvider, buildCodexProvider, DEFAULT_CODEX_WIRE_API, isHttpUrl, maskSecret } from "./providerFactory";
import { checkForUpdate, installAndRelaunch, type Update } from "./updater";
import { onOpenUrl, getCurrent } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyTheme, type Theme } from "./theme";
import brandLogo from "./assets/zsw.png";
import {
  AlertIcon,
  BookOpenIcon,
  BoltIcon,
  CheckIcon,
  CopyIcon,
  DownloadIcon,
  EditIcon,
  InboxIcon,
  MessageIcon,
  PlusIcon,
  RefreshIcon,
  SettingsIcon,
  TrashIcon,
} from "./Icons";

// 客户端图标（内联，贴合 Claude / Codex 品牌感）
const ClaudeMark = (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d="M12 2 3 7v10l9 5 9-5V7z" opacity=".16" />
    <path d="M12 6.5 8 16h1.9l.8-2h2.6l.8 2H16L12 6.5zm-.9 6 .9-2.4.9 2.4z" />
  </svg>
);
const CodexMark = (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="m8 8-4 4 4 4M16 8l4 4-4 4M14 5l-4 14" />
  </svg>
);
const GrokMark = (
  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.1" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M5 19 19 5M9 19l10-10M15 5 5 15" opacity=".55" />
    <path d="M5 5l14 14" />
  </svg>
);
function MoonIcon() {
  return (
    <svg className="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
    </svg>
  );
}
function DotsIcon() {
  return (
    <svg className="ui-icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5" cy="12" r="1.5" /><circle cx="12" cy="12" r="1.5" /><circle cx="19" cy="12" r="1.5" />
    </svg>
  );
}

const CLIENTS: { key: AppType; label: string; icon: React.ReactNode }[] = [
  { key: "claude", label: "Claude Code", icon: ClaudeMark },
  { key: "codex", label: "Codex", icon: CodexMark },
  { key: "grok", label: "Grok", icon: GrokMark },
];

function initials(name: string): string {
  const s = name.trim();
  return /[一-龥]/.test(s) ? s.slice(0, 1) : s.slice(0, 2).toUpperCase();
}

function isOfficialProvider(provider: Provider): boolean {
  return provider.category === "official" || (provider.meta as any)?.kind === "officialLocal";
}

function summarizeUrl(p: Provider): string {
  const env = (p.settingsConfig as any)?.env ?? {};
  const cfg = (p.settingsConfig as any) ?? {};
  return (
    env.ANTHROPIC_BASE_URL ??
    (cfg.config ? String(cfg.config).match(/base_url\s*=\s*"([^"]+)"/)?.[1] : "") ??
    ""
  ) || "";
}

function stripOneM(m: string): string {
  return m.replace(/\s*\[1M\]\s*$/i, "").trim();
}

// 取供应商展示用模型列表（Claude：主+各档去重；Codex：单模型）+ 是否含 1M + wire_api
function providerModels(app: AppType, p: Provider): { models: string[]; has1M: boolean; wireApi?: string } {
  if (app === "claude") {
    const env = (p.settingsConfig as any)?.env ?? {};
    const raw = [
      env.ANTHROPIC_MODEL,
      env.ANTHROPIC_DEFAULT_HAIKU_MODEL,
      env.ANTHROPIC_DEFAULT_SONNET_MODEL,
      env.ANTHROPIC_DEFAULT_OPUS_MODEL,
      env.ANTHROPIC_DEFAULT_FABLE_MODEL,
    ].filter(Boolean).map(String);
    const has1M = raw.some((m) => /\[1M\]/i.test(m));
    const models = [...new Set(raw.map(stripOneM).filter(Boolean))];
    return { models, has1M };
  }
  const cfg = String((p.settingsConfig as any)?.config ?? "");
  const model = cfg.match(/^model\s*=\s*"([^"]+)"/m)?.[1];
  const wireApi = ((p.meta as any)?.wireApi ?? cfg.match(/wire_api\s*=\s*"([^"]+)"/)?.[1]) as string | undefined;
  return { models: model ? [model] : [], has1M: false, wireApi };
}

function slug(s: string): string {
  const t = s.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return t || `imported-${Math.floor(Math.random() * 1e6).toString(36)}`;
}

function uniqueImportIdentity(
  requested: string,
  providers: Record<string, Provider>,
): { name: string; id: string; collision: boolean } {
  const names = new Set(Object.values(providers).map((p) => p.name));
  const ids = new Set(Object.keys(providers));
  let name = requested;
  let n = 1;
  while (names.has(name)) {
    n += 1;
    name = `${requested} ${n}`;
  }
  const base = slug(name);
  let id = base;
  let m = 2;
  while (ids.has(id)) {
    id = `${base}-${m}`;
    m += 1;
  }
  return { name, id, collision: name !== requested };
}

function copyNameOf(name: string, providers: Record<string, Provider>): string {
  const existing = new Set(Object.values(providers).map((provider) => provider.name));
  const base = `${name}-复制`;
  if (!existing.has(base)) return base;
  let index = 2;
  while (existing.has(`${base}-${index}`)) index += 1;
  return `${base}-${index}`;
}

function copyIdOf(id: string, providers: Record<string, Provider>): string {
  const base = `${id}-copy`;
  if (!providers[base]) return base;
  let index = 2;
  while (providers[`${base}-${index}`]) index += 1;
  return `${base}-${index}`;
}

function cloneJson<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

type Lat = { ms?: number; err?: boolean; loading?: boolean };
type DropTarget = { id: string; after: boolean };
type HealthKind = "full" | "ok" | "bad" | "none";

function latClass(l?: Lat): HealthKind {
  if (!l || (l.ms == null && !l.err)) return "none";
  if (l.err) return "bad";
  const ms = l.ms!;
  return ms <= 1000 ? "full" : ms <= 3000 ? "ok" : "bad";
}
function latText(l?: Lat): string {
  if (!l) return "未测速";
  if (l.loading) return "测速中";
  if (l.err) return "超时";
  if (l.ms != null) return l.ms < 1 ? "<1ms" : `${Math.round(l.ms)}ms`;
  return "未测速";
}

const HRANK: Record<HealthKind, number> = { full: 3, ok: 2, bad: 1, none: 0 };
function healthText(kind: HealthKind): string {
  return kind === "full" ? "正常" : kind === "ok" ? "偏慢" : kind === "bad" ? "异常" : "未测";
}

// 侧栏可拖拽宽度（本地持久化）
const SIDEBAR_MIN = 220;
const SIDEBAR_MAX = 460;
const SIDEBAR_DEFAULT = 264;

export default function App() {
  const [root, setRoot] = useState<Root | null>(null);
  const rootRef = useRef<Root | null>(null);
  rootRef.current = root;
  const [tab, setTab] = useState<AppType>("claude");
  const [lat, setLat] = useState<Record<string, Lat>>({});
  const [modal, setModal] = useState<null | { edit?: Provider }>(null);
  const [streamModal, setStreamModal] = useState<null | { app: AppType; provider: Provider }>(null);
  const [streamResults, setStreamResults] = useState<Record<string, StreamTestSummary>>({});
  const [page, setPage] = useState<"providers" | "settings">("providers");
  const [io, setIo] = useState<null | "import" | "export">(null);
  const [ccOpen, setCcOpen] = useState(false);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  const [originalStatus, setOriginalStatus] = useState<OriginalConfigStatus | null>(null);
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);
  const [proxy, setProxy] = useState<ProxyStatus | null>(null);
  const [proxyBusy, setProxyBusy] = useState(false);
  const [proxyRate, setProxyRate] = useState<Record<AppType, number>>({ claude: 0, codex: 0, grok: 0 });
  const [nowTs, setNowTs] = useState(0);
  const rateSampleRef = useRef<Record<AppType, { total: number; ts: number } | null>>({ claude: null, codex: null, grok: null });
  const [confirm, setConfirm] = useState<null | {
    title?: string;
    message: string;
    confirmText?: string;
    secondaryText?: string;
    danger?: boolean;
    onConfirm: () => void;
    onSecondary?: () => void;
  }>(null);
  const [pendingImport, setPendingImport] = useState<PendingImport | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(0);
  const dragSourceRef = useRef<string | null>(null);
  const dropTargetRef = useRef<DropTarget | null>(null);
  const [sidebarW, setSidebarW] = useState<number>(() => {
    const v = Number(localStorage.getItem("zsw.sidebarW"));
    return Number.isFinite(v) && v >= SIDEBAR_MIN && v <= SIDEBAR_MAX ? v : SIDEBAR_DEFAULT;
  });
  const lastWRef = useRef(sidebarW);
  const resizingRef = useRef(false);
  const [maximized, setMaximized] = useState(false);

  function pushToast(kind: ToastKind, msg: string) {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, kind, msg }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 2600);
  }

  useEffect(() => {
    getConfig().then(setRoot).catch((e) => pushToast("error", String(e)));
    originalConfigStatus().then(setOriginalStatus).catch(() => {});
    checkForUpdate()
      .then((update) => setUpdateInfo(update))
      .catch(() => {});
  }, []);

  // 轮询代理状态 + 本地活跃度（2s）
  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const st = await proxyStatus();
        if (!alive) return;
        const now = Date.now();
        for (const app of ["claude", "codex"] as ("claude" | "codex")[]) {
          const total = st[app].total;
          const prev = rateSampleRef.current[app];
          if (prev && total >= prev.total && now > prev.ts) {
            const rpm = ((total - prev.total) / ((now - prev.ts) / 1000)) * 60;
            setProxyRate((r) => ({ ...r, [app]: Math.round((r[app] ?? 0) * 0.5 + rpm * 0.5) }));
          } else if (prev && total < prev.total) {
            setProxyRate((r) => ({ ...r, [app]: 0 }));
          }
          rateSampleRef.current[app] = { total, ts: now };
        }
        setProxy(st);
        setNowTs(now);
      } catch {
        /* 忽略 */
      }
    }
    poll();
    const id = setInterval(poll, 2000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // 底部「一键路由」：只对指定客户端生效（分客户端路由）。Grok 不支持路由。
  function toggleRouting(app: AppType) {
    if (app === "grok" || proxyBusy || !proxy) return;
    const next = !proxy[app].routed;
    const port = proxy.port;
    const label = app === "claude" ? "Claude Code" : "Codex";
    setProxyBusy(true);
    setProxy((p) => (p ? { ...p, [app]: { ...p[app], routed: next } } : p));
    setAppRouting(app, next)
      .then((r) => {
        setRoot(r);
        proxyStatus().then(setProxy).catch(() => {});
        pushToast(
          "success",
          next
            ? `${label} 已开启本地路由（127.0.0.1:${port}/${app}）· 切换供应商无需重启`
            : `${label} 已关闭本地路由，已恢复直连`,
        );
      })
      .catch((e) => {
        setProxy((p) => (p ? { ...p, [app]: { ...p[app], routed: !next } } : p));
        pushToast("error", "切换本地路由失败：" + String(e));
      })
      .finally(() => setProxyBusy(false));
  }

  async function handleCheckUpdate() {
    const update = await checkForUpdate();
    setUpdateInfo(update);
    if (!update) pushToast("success", "已是最新版本");
  }

  async function handleInstallUpdate() {
    if (!updateInfo || updateBusy) return;
    setUpdateBusy(true);
    setUpdateProgress(0);
    try {
      await installAndRelaunch(updateInfo, (downloaded, total) => {
        setUpdateProgress(total ? Math.round((downloaded / total) * 100) : null);
      });
    } catch (e) {
      console.error("[z-switch] 更新失败:", e);
      pushToast("error", "更新失败，请稍后重试");
      setUpdateBusy(false);
      setUpdateProgress(null);
    }
  }

  const theme = (root?.settings as any)?.theme as Theme | undefined;
  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  // 侧栏快捷主题切换（在 light/dark 间切并持久化，system 仍可在设置里选）
  function toggleTheme() {
    const resolved =
      theme === "dark"
        ? "dark"
        : theme === "light"
          ? "light"
          : window.matchMedia?.("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light";
    const next: Theme = resolved === "dark" ? "light" : "dark";
    onSaveSettings({ ...((root?.settings as Record<string, unknown>) ?? {}), theme: next });
    pushToast("success", next === "dark" ? "已切换到深色主题" : "已切换到浅色主题");
  }

  // 托盘切换后后端广播 → 重载配置
  useEffect(() => {
    let un: (() => void) | undefined;
    listen("config-changed", () => {
      getConfig().then(setRoot).catch(() => {});
    })
      .then((f) => (un = f))
      .catch(() => {});
    return () => un?.();
  }, []);

  // 窗口最大化状态：最大化时取消窗口圆角，贴屏更规整
  useEffect(() => {
    const w = getCurrentWindow();
    w.isMaximized().then(setMaximized).catch(() => {});
    let un: (() => void) | undefined;
    w.onResized(() => w.isMaximized().then(setMaximized).catch(() => {}))
      .then((f) => (un = f))
      .catch(() => {});
    return () => un?.();
  }, []);

  // 深链 zswitch://import?... 一键导入
  useEffect(() => {
    function parseAndImport(url: string) {
      try {
        const u = new URL(url);
        // 兼容两种协议：zswitch://（本软件）与 ccswitch://（cc-switch，中转站一键导入按钮常用）
        const isCcswitch = u.protocol === "ccswitch:";
        const q = u.searchParams;

        // cc-switch 深链支持 provider/mcp/prompt/skill，这里只接受 provider
        if (isCcswitch) {
          const resource = (q.get("resource") || "provider").toLowerCase();
          if (resource !== "provider") {
            pushToast("error", "该链接不是供应商导入（暂不支持 MCP / Prompt / Skill）");
            return;
          }
        }

        const appRaw = q.get("app");
        // cc-switch 还支持 gemini/opencode 等，本软件仅 claude/codex
        if (isCcswitch && appRaw && appRaw !== "claude" && appRaw !== "codex") {
          pushToast("error", `暂不支持导入 ${appRaw} 供应商（仅 Claude Code / Codex）`);
          return;
        }
        const appk = (appRaw === "codex" ? "codex" : "claude") as AppType;
        const nm = q.get("name") || "导入的供应商";

        // 地址：zswitch 用 baseUrl；ccswitch 用 endpoint（可逗号分隔多个，取第一个）
        const baseUrl = ((isCcswitch ? q.get("endpoint") : q.get("baseUrl")) || "").split(",")[0].trim();

        // 密钥：zswitch 用 key/keyB64；ccswitch 用 apiKey
        let key = (isCcswitch ? q.get("apiKey") : q.get("key")) || "";
        const kb64 = q.get("keyB64");
        if (!key && kb64) {
          try {
            key = atob(kb64);
          } catch {
            /* 非法 base64，忽略 */
          }
        }
        const model = q.get("model") || "";
        if (!isHttpUrl(baseUrl)) {
          pushToast("error", "链接被拒绝：接入点必须是 http(s) 地址");
          return;
        }
        const providers = rootRef.current?.apps[appk]?.providers ?? {};
        const { name: finalName, id, collision } = uniqueImportIdentity(nm, providers);
        // 分档模型：zswitch=haiku/sonnet/opus/fable；ccswitch=haikuModel/sonnetModel/opusModel
        const haiku = (isCcswitch ? q.get("haikuModel") : q.get("haiku")) || undefined;
        const sonnet = (isCcswitch ? q.get("sonnetModel") : q.get("sonnet")) || undefined;
        const opus = (isCcswitch ? q.get("opusModel") : q.get("opus")) || undefined;
        const fable = (isCcswitch ? q.get("fableModel") : q.get("fable")) || undefined;
        const provider =
          appk === "codex"
            ? buildCodexProvider(
                { id, name: finalName, category: "custom", baseUrl, model: model || "gpt-5.5", wireApi: (q.get("wireApi") as any) || DEFAULT_CODEX_WIRE_API },
                key,
              )
            : buildClaudeProvider(
                {
                  id,
                  name: finalName,
                  category: "custom",
                  baseUrl,
                  apiKeyField: (q.get("apiKeyField") as any) || "ANTHROPIC_AUTH_TOKEN",
                  model,
                  haiku,
                  sonnet,
                  opus,
                  fable,
                },
                key,
              );
        setPendingImport({
          app: appk,
          name: finalName,
          provider,
          baseUrl,
          model,
          keyMasked: maskSecret(key),
          collisionOf: collision ? nm : undefined,
        });
      } catch (e) {
        pushToast("error", "链接解析失败：" + String(e));
      }
    }

    let un: (() => void) | undefined;
    (async () => {
      try {
        const cur = await getCurrent();
        if (cur) cur.forEach(parseAndImport);
        un = await onOpenUrl((urls) => urls.forEach(parseAndImport));
      } catch {
        /* 非 Tauri 环境或未注册协议，忽略 */
      }
    })();
    return () => un?.();
  }, []);

  const data = root?.apps[tab];
  const ordered = useMemo<Provider[]>(
    () => (data ? data.order.map((id) => data.providers[id]).filter(Boolean) : []),
    [data],
  );

  async function run(p: Promise<Root>, okMsg?: string) {
    try {
      setRoot(await p);
      if (okMsg) pushToast("success", okMsg);
    } catch (e) {
      pushToast("error", String(e));
    }
  }

  // 由真实信号（延迟 + 上次流式测试）派生健康结论；不伪造评分
  function healthKind(clientKey: AppType, p: Provider): HealthKind {
    if (isOfficialProvider(p)) return "full";
    const l = lat[`${clientKey}:${p.id}`];
    const sr = streamResults[`${clientKey}:${p.id}`];
    if (l?.err) return "bad";
    if (sr?.status === "error") return "bad";
    if (sr?.status === "warning") return "ok";
    if (sr?.status === "success") {
      if (l?.ms != null) return l.ms <= 1000 ? "full" : l.ms <= 3000 ? "ok" : "bad";
      return "full";
    }
    return latClass(l);
  }

  function clientBestHealth(clientKey: AppType): HealthKind {
    const d = root?.apps[clientKey];
    if (!d) return "none";
    let best: HealthKind = "none";
    for (const id of d.order) {
      const p = d.providers[id];
      if (!p || isOfficialProvider(p)) continue;
      const k = healthKind(clientKey, p);
      if (HRANK[k] > HRANK[best]) best = k;
    }
    return best;
  }

  function onDelete(p: Provider) {
    if (isOfficialProvider(p)) {
      pushToast("error", "官方账号是系统卡片，不能删除");
      return;
    }
    const isActive = data?.current === p.id;
    const remove = (mode?: ActiveDeleteMode) => {
      setConfirm(null);
      run(deleteProvider(tab, p.id, mode), `已删除 ${p.name}`);
    };

    if (isActive) {
      const canRestore = originalStatus?.captured === true;
      setConfirm({
        title: "删除正在使用的供应商",
        message: canRestore
          ? `「${p.name}」正在使用。你可以先恢复首次保存的本机配置，也可以保留电脑当前配置、仅将它移出 z-switch。`
          : `「${p.name}」正在使用，且没有可用的原始配置快照。可以保留电脑当前配置，仅将它移出 z-switch。`,
        confirmText: canRestore ? "恢复并删除" : "仅移出列表",
        secondaryText: canRestore ? "仅移出列表" : undefined,
        danger: true,
        onConfirm: () => remove(canRestore ? "restore" : "keep"),
        onSecondary: canRestore ? () => remove("keep") : undefined,
      });
      return;
    }

    setConfirm({
      message: `删除供应商「${p.name}」？此操作不可撤销。`,
      confirmText: "删除",
      danger: true,
      onConfirm: () => remove(),
    });
  }

  function onRestoreOriginal(app: AppType) {
    const label = app === "claude" ? "Claude Code" : app === "grok" ? "Grok" : "Codex";
    setConfirm({
      title: `恢复 ${label} 原始配置`,
      message: `将恢复首次运行 z-switch 时保存的 ${label} 配置，并取消当前供应商关联。恢复前会自动备份电脑上的现有配置。`,
      confirmText: "确认恢复",
      danger: true,
      onConfirm: () => {
        setConfirm(null);
        run(restoreOriginal(app), `已恢复 ${label} 原始配置`);
      },
    });
  }

  function onRestoreOfficialBaseline(app: AppType) {
    const label = app === "claude" ? "Claude Code" : "Codex";
    const loginHint =
      app === "claude"
        ? "重置后请用 Claude Code 自行登录官方账号（如需）。"
        : "若本机曾保存过 Codex 官方登录态会一并恢复；否则请在 Codex 里重新登录。";
    setConfirm({
      title: `重置 ${label} 为官方账号配置`,
      message:
        `将把 ${label} 的配置文件重置为干净的官方账号基线（清除中转地址与密钥）。` +
        `现有文件会先备份到 ~/.z-switch/backups/；即使文件已损坏也会强制重写。` +
        loginHint +
        `本地路由会关闭，当前生效项切换为官方账号。此操作不会恢复你之前的第三方供应商配置。`,
      confirmText: "确认重置",
      danger: true,
      onConfirm: () => {
        setConfirm(null);
        restoreOfficialBaseline(app)
          .then((r) => {
            setRoot(r);
            pushToast("success", `已将 ${label} 重置为官方账号配置`);
          })
          .catch((e) => pushToast("error", String(e)));
      },
    });
  }

  function onSaveProvider(p: Provider) {
    setStreamResults((current) => {
      const next = { ...current };
      delete next[`${tab}:${p.id}`];
      return next;
    });
    run(saveProvider(tab, p), "已保存");
    setModal(null);
  }

  async function onCopyProvider(provider: Provider) {
    if (isOfficialProvider(provider)) {
      pushToast("error", "官方账号不能复制");
      return;
    }
    const appData = root?.apps[tab];
    if (!appData) return;

    const copied: Provider = {
      ...provider,
      id: copyIdOf(provider.id, appData.providers),
      name: copyNameOf(provider.name, appData.providers),
      settingsConfig: cloneJson(provider.settingsConfig),
      meta: cloneJson(provider.meta ?? {}),
      failover: cloneJson(provider.failover ?? {}),
    };

    let saved: Root;
    try {
      saved = await saveProvider(tab, copied);
      setRoot(saved);
    } catch (error) {
      pushToast("error", "复制失败：" + String(error));
      return;
    }

    const savedData = saved.apps[tab];
    const order = savedData.order.filter((id) => id !== copied.id);
    const sourceIndex = order.indexOf(provider.id);
    order.splice(sourceIndex >= 0 ? sourceIndex + 1 : order.length, 0, copied.id);
    try {
      setRoot(await reorderProviders(tab, order));
      pushToast("success", `已复制为「${copied.name}」`);
    } catch (error) {
      pushToast("error", "供应商已复制，但移动到原项下方失败：" + String(error));
    }
  }

  function onSaveSettings(s: Record<string, unknown>) {
    saveSettings(s).then(setRoot).catch((e) => pushToast("error", String(e)));
  }

  function onImportLive() {
    importLive()
      .then((r) => {
        setRoot(r);
        pushToast("success", "已从现有配置导入");
      })
      .catch((e) => pushToast("error", String(e)));
  }

  async function speedtestAll() {
    for (const p of ordered) {
      if (isOfficialProvider(p)) continue;
      const url = summarizeUrl(p);
      const key = `${tab}:${p.id}`;
      if (!url) {
        setLat((m) => ({ ...m, [key]: { err: true } }));
        continue;
      }
      setLat((m) => ({ ...m, [key]: { loading: true } }));
      try {
        const ms = await speedtest(url);
        setLat((m) => ({ ...m, [key]: { ms } }));
      } catch {
        setLat((m) => ({ ...m, [key]: { err: true } }));
      }
    }
  }

  async function speedtestOne(p: Provider) {
    const url = summarizeUrl(p);
    const key = `${tab}:${p.id}`;
    if (!url) {
      setLat((m) => ({ ...m, [key]: { err: true } }));
      return;
    }
    setLat((m) => ({ ...m, [key]: { loading: true } }));
    try {
      const ms = await speedtest(url);
      setLat((m) => ({ ...m, [key]: { ms } }));
    } catch {
      setLat((m) => ({ ...m, [key]: { err: true } }));
    }
  }

  function clearSortState() {
    dragSourceRef.current = null;
    dropTargetRef.current = null;
    setDragId(null);
    setDropTarget(null);
  }

  function onSortPointerDown(e: React.PointerEvent<HTMLSpanElement>, id: string) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragSourceRef.current = id;
    dropTargetRef.current = null;
    setDragId(id);
    setDropTarget(null);
    e.currentTarget.setPointerCapture(e.pointerId);
  }

  function onSortPointerMove(e: React.PointerEvent<HTMLSpanElement>) {
    const sourceId = dragSourceRef.current;
    if (!sourceId) return;

    const list = e.currentTarget.closest<HTMLElement>(".list");
    if (list) {
      const listRect = list.getBoundingClientRect();
      if (e.clientY < listRect.top + 36) list.scrollTop -= 14;
      else if (e.clientY > listRect.bottom - 36) list.scrollTop += 14;
    }

    const hit = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
    const card = hit?.closest<HTMLElement>("[data-provider-id]");
    const targetId = card?.dataset.providerId;
    if (!card || !targetId || targetId === sourceId) {
      dropTargetRef.current = null;
      setDropTarget(null);
      return;
    }

    const rect = card.getBoundingClientRect();
    const nextTarget = { id: targetId, after: e.clientY > rect.top + rect.height / 2 };
    dropTargetRef.current = nextTarget;
    setDropTarget(nextTarget);
  }

  function onSortPointerUp(e: React.PointerEvent<HTMLSpanElement>) {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    const sourceId = dragSourceRef.current;
    const target = dropTargetRef.current;
    clearSortState();
    if (!sourceId || !target || !data) return;

    const order = data.order.filter((id) => id !== sourceId);
    const targetIndex = order.indexOf(target.id);
    if (targetIndex < 0) return;
    order.splice(targetIndex + (target.after ? 1 : 0), 0, sourceId);
    if (order.every((id, index) => id === data.order[index])) return;
    run(reorderProviders(tab, order));
  }

  function onSortPointerCancel(e: React.PointerEvent<HTMLSpanElement>) {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    clearSortState();
  }

  // 侧栏 / 主区分隔线：拖动调宽，双击复位，本地持久化
  function onResizeDown(e: React.PointerEvent<HTMLDivElement>) {
    e.preventDefault();
    resizingRef.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }
  function onResizeMove(e: React.PointerEvent<HTMLDivElement>) {
    if (!resizingRef.current) return;
    const w = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(e.clientX)));
    lastWRef.current = w;
    setSidebarW(w);
  }
  function onResizeUp(e: React.PointerEvent<HTMLDivElement>) {
    if (!resizingRef.current) return;
    resizingRef.current = false;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    localStorage.setItem("zsw.sidebarW", String(lastWRef.current));
  }
  function onResizeReset() {
    lastWRef.current = SIDEBAR_DEFAULT;
    setSidebarW(SIDEBAR_DEFAULT);
    localStorage.setItem("zsw.sidebarW", String(SIDEBAR_DEFAULT));
  }

  const existingIds = data ? Object.keys(data.providers) : [];

  // 底部状态栏派生值（跟随当前选中客户端）。Grok 不支持本地路由，恒直连。
  const appStatus = proxy && tab !== "grok" ? proxy[tab] : null;
  const routedForTab = appStatus?.routed ?? false;
  const proxyPort = proxy?.port ?? 8899;
  const appInFlight = appStatus?.inFlight ?? 0;
  const appLastActivity = appStatus?.lastActivityMs ?? 0;
  const appRate = proxyRate[tab] ?? 0;
  const idleSec = appLastActivity > 0 ? Math.max(0, Math.floor((nowTs - appLastActivity) / 1000)) : null;
  const proxyActive = routedForTab && (appInFlight > 0 || (idleSec != null && idleSec < 10));
  const currentName = data?.current ? data.providers[data.current]?.name ?? null : null;
  const activeSegments: string[] = [];
  if (appInFlight > 0) activeSegments.push(`${appInFlight} 进行中`);
  if (appRate >= 1) activeSegments.push(`~${appRate} 次/分`);
  activeSegments.push("活跃");

  const clientLabel = CLIENTS.find((c) => c.key === tab)?.label ?? "";

  return (
    <div className={"app" + (maximized ? " maximized" : "")} style={{ "--sidebar-w": `${sidebarW}px` } as React.CSSProperties}>
      {/* ============ 侧栏：客户端 ============ */}
      <aside className="sidebar">
        <div className="sb-top" data-tauri-drag-region>
          <img className="brand-logo" src={brandLogo} alt="" />
          <span className="brand">z-switch</span>
        </div>

        <div className="sb-scroll">
          <div className="sb-label">客户端</div>
          <div className="clist">
            {CLIENTS.map((c) => {
              const d = root?.apps[c.key];
              const count = d ? d.order.length : 0;
              const curName = d?.current ? d.providers[d.current]?.name ?? null : null;
              const hb = clientBestHealth(c.key);
              return (
                <button
                  key={c.key}
                  className={"citem" + (page === "providers" && tab === c.key ? " sel" : "")}
                  onClick={() => { setTab(c.key); setPage("providers"); }}
                >
                  <span className="cico">{c.icon}</span>
                  <span className="cbody">
                    <span className="cname">{c.label}</span>
                    <span className="csub">
                      <span className={"hb " + hb} />
                      {curName ? `当前 · ${curName}` : "未选择"}
                    </span>
                  </span>
                  {c.key !== "grok" && proxy?.[c.key]?.routed && <span className="route-tag">路由</span>}
                  <span className="ccount">{count}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="sb-foot">
          <button
            className={"citem nav-item" + (page === "settings" ? " sel" : "")}
            onClick={() => setPage("settings")}
            title={updateInfo ? "设置 · 有新版本可用" : "设置"}
          >
            <span className="cico"><SettingsIcon /></span>
            <span className="cbody"><span className="cname">设置</span></span>
            {updateInfo && <span className="nav-dot" />}
          </button>
          <div className="foot-tools">
            <button className="foot-btn" onClick={toggleTheme} title="切换深色 / 浅色主题">
              <MoonIcon />主题
            </button>
            <button className="foot-btn" onClick={() => openHelpDocument().catch((e) => pushToast("error", String(e)))} title="使用帮助文档">
              <BookOpenIcon />文档
            </button>
          </div>
          <button
            className="acct"
            title="z-switch · 由真测 Ztest 出品"
            onClick={() => pushToast("info", "z-switch · 开源 · 由真测 Ztest 出品 · ztest.ai")}
          >
            <img className="ava" src={brandLogo} alt="" />
            <span className="who">
              <b>真测 Ztest</b>
              <span>ztest.ai · 本地保存</span>
            </span>
            <span className="acct-more"><DotsIcon /></span>
          </button>
        </div>
      </aside>

      {/* ============ 主区：供应商 ============ */}
      <main className="main">
        <div
          className="resizer"
          role="separator"
          aria-orientation="vertical"
          title="拖动调整侧栏宽度 · 双击复位"
          onPointerDown={onResizeDown}
          onPointerMove={onResizeMove}
          onPointerUp={onResizeUp}
          onPointerCancel={onResizeUp}
          onDoubleClick={onResizeReset}
        />
        <div className="topbar">
          <div className="tb-drag" data-tauri-drag-region>
            <span className="crumb"><b>{page === "settings" ? "设置" : clientLabel}</b></span>
          </div>
          <div className="topbar-actions">
            <WindowControls />
          </div>
        </div>

        {page === "settings" ? (
          <SettingsModal
            asPage
            settings={(root?.settings as Record<string, any>) ?? {}}
            originalStatus={originalStatus}
            onClose={() => setPage("providers")}
            onSave={onSaveSettings}
            onRestoreOriginal={onRestoreOriginal}
            onRestoreOfficialBaseline={onRestoreOfficialBaseline}
            onToast={pushToast}
            updateInfo={updateInfo}
            updateBusy={updateBusy}
            updateProgress={updateProgress}
            onCheckUpdate={handleCheckUpdate}
            onInstallUpdate={handleInstallUpdate}
            onOpenImport={() => setIo("import")}
            onOpenExport={() => setIo("export")}
            onOpenCcswitch={() => setCcOpen(true)}
            onRepaired={(r) => setRoot(r)}
          />
        ) : (
        <>
        <div className="list-header">
          <span className="lh-title">供应商 <b>{ordered.length}</b></span>
          <div className="lh-actions">
            <button className="btn" onClick={speedtestAll}><BoltIcon />全部测速</button>
            <button className="btn primary" onClick={() => setModal({})}><PlusIcon />添加供应商</button>
          </div>
        </div>
        <div className={"list" + (dragId ? " sorting" : "")}>
          {ordered.length === 0 ? (
            <div className="empty">
              <div className="empty-ico"><InboxIcon /></div>
              <h3>还没有供应商</h3>
              <p>从现有配置导入，或点右上角「添加供应商」快速开始。</p>
              <div className="empty-row">
                <button className="btn" onClick={onImportLive}><DownloadIcon />导入现有配置</button>
                <button className="btn accent" onClick={() => setModal({})}><PlusIcon />添加供应商</button>
              </div>
            </div>
          ) : (
            <div className="plist-box">
              {ordered.map((p) => {
                const active = data?.current === p.id;
                const official = isOfficialProvider(p);
                const url = summarizeUrl(p);
                const { models, has1M, wireApi } = providerModels(tab, p);
                const l = lat[`${tab}:${p.id}`];
                const kind = healthKind(tab, p);
                const streamResult = streamResults[`${tab}:${p.id}`];
                return (
                  <div
                    key={p.id}
                    data-provider-id={p.id}
                    className={
                      "prow" +
                      (active ? " active" : "") +
                      (dragId === p.id ? " dragging" : "") +
                      (dropTarget?.id === p.id && dragId !== p.id
                        ? dropTarget.after
                          ? " drop-after"
                          : " drop-before"
                        : "")
                    }
                  >
                    <span
                      className="p-ava"
                      title="按住拖拽排序"
                      onPointerDown={(e) => onSortPointerDown(e, p.id)}
                      onPointerMove={onSortPointerMove}
                      onPointerUp={onSortPointerUp}
                      onPointerCancel={onSortPointerCancel}
                    >
                      {initials(p.name)}
                    </span>

                    <div className="pinfo">
                      <div className="prow1">
                        <span className="pname">{p.name}</span>
                        {official && <span className="badge">官方账号</span>}
                        {active && <span className="badge green"><span className="d" />正在生效</span>}
                      </div>
                      <div className="prow2">
                        {official ? (
                          <span className="purl">使用本机 {clientLabel} 登录 · 提供全部模型</span>
                        ) : (
                          <>
                            <span className="purl mono" title={url}>{url || "—"}</span>
                            {(models.length > 0 || (tab === "codex" && wireApi) || has1M) && (
                              <span className="prow2-models">
                                <span className="sep" />
                                {models.length > 0 && <span className="tierchip mono" title={models.join(" · ")}>{models[0]}</span>}
                                {models.length > 1 && <span className="tierchip more" title={models.join(" · ")}>+{models.length - 1}</span>}
                                {tab === "codex" && wireApi && <span className="tierchip">wire_api·{wireApi}</span>}
                                {has1M && <span className="tierchip more">1M</span>}
                              </span>
                            )}
                          </>
                        )}
                      </div>
                    </div>

                    <div className="phealth">
                      {official ? (
                        <span className="local-state"><span className="d" />本机登录</span>
                      ) : (
                        <>
                          <span className={"verdict " + kind}><span className="d" />{healthText(kind)}</span>
                          <span className={"chip-lat" + (l?.loading ? " loading" : "")}>{latText(l)}</span>
                        </>
                      )}
                    </div>

                    <div className="pactions">
                      {active ? (
                        <span className="inuse"><CheckIcon />已生效</span>
                      ) : (
                        <button className="row-btn" onClick={() => run(switchProvider(tab, p.id), `已切换到 ${p.name}`)}>切换</button>
                      )}
                      {!official && (
                        <>
                          <div className="row-tools">
                            <button
                              className={"mini-ic" + (streamResult ? ` test-${streamResult.status}` : "")}
                              onClick={() => setStreamModal({ app: tab, provider: p })}
                              title={streamResult?.status === "success"
                                ? `真实流式测试：上次成功，首字 ${streamResult.firstTokenMs}ms`
                                : streamResult?.status === "warning"
                                  ? "真实流式测试：上次可用，但未检测到流式响应"
                                  : streamResult?.status === "error"
                                    ? "真实流式测试：上次调用失败"
                                    : "真实流式测试"}
                              aria-label={`真实流式测试 ${p.name}`}
                            >
                              {streamResult?.status === "success" ? <CheckIcon />
                                : streamResult?.status === "warning" || streamResult?.status === "error" ? <AlertIcon />
                                  : <MessageIcon />}
                            </button>
                            <button
                              className={"mini-ic" + (l?.loading ? " spin" : "")}
                              onClick={() => speedtestOne(p)}
                              disabled={l?.loading}
                              title="测速"
                              aria-label={`测速 ${p.name}`}
                            >
                              {l?.loading ? <RefreshIcon /> : <BoltIcon />}
                            </button>
                          </div>
                          <div className="row-tools">
                            <button className="mini-ic" onClick={() => setModal({ edit: p })} title="编辑" aria-label={`编辑 ${p.name}`}><EditIcon /></button>
                            <button className="mini-ic copy" onClick={() => onCopyProvider(p)} title="复制" aria-label={`复制 ${p.name}`}><CopyIcon /></button>
                            <button className="mini-ic danger" onClick={() => onDelete(p)} title="删除" aria-label={`删除 ${p.name}`}><TrashIcon /></button>
                          </div>
                        </>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
        </>
        )}

        {/* 底部状态栏（跟随当前客户端） */}
        <div className="statusbar">
          {routedForTab ? (
            <>
              <span className="si">
                <span className={"sb-dot on" + (proxyActive ? " live" : "")} />
                本地路由 <b>ON</b> · 127.0.0.1:{proxyPort}/{tab}
              </span>
              <span className="si">
                {proxyActive
                  ? activeSegments.join(" · ")
                  : `空闲${idleSec != null ? ` ${idleSec}s` : ""}`}
              </span>
            </>
          ) : (
            <span className="si">
              <span className="sb-dot off" />
              直连模式 · 切换供应商需重启客户端
            </span>
          )}
          <div className="sb-right">
            {currentName && <span className="si sb-current">生效 · {currentName}</span>}
            {tab !== "grok" && (
              <label className={"sb-route" + (proxyBusy ? " busy" : "")}>
                {clientLabel} 路由
                <button
                  type="button"
                  className={"switch" + (routedForTab ? " on" : "")}
                  role="switch"
                  aria-checked={routedForTab}
                  aria-label={`${clientLabel} 本地路由`}
                  onClick={() => toggleRouting(tab)}
                />
              </label>
            )}
          </div>
        </div>
      </main>

      {streamModal && (
        <StreamTestModal
          app={streamModal.app}
          provider={streamModal.provider}
          onClose={() => setStreamModal(null)}
          onResult={(result) => {
            const key = `${streamModal.app}:${streamModal.provider.id}`;
            setStreamResults((current) => ({ ...current, [key]: result }));
          }}
        />
      )}
      {modal && (
        <ProviderModal
          app={tab}
          initial={modal.edit}
          existingIds={existingIds}
          onClose={() => setModal(null)}
          onSave={onSaveProvider}
        />
      )}
      {io && (
        <ImportExportModal
          mode={io}
          onClose={() => setIo(null)}
          onImported={(r) => {
            setRoot(r);
            pushToast("success", "已导入配置");
          }}
        />
      )}
      {ccOpen && (
        <CcswitchImportModal
          onClose={() => setCcOpen(false)}
          onImported={(r, count) => {
            setRoot(r);
            setCcOpen(false);
            pushToast("success", `已从 cc-switch 导入 ${count} 个供应商`);
          }}
        />
      )}
      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          message={confirm.message}
          confirmText={confirm.confirmText}
          secondaryText={confirm.secondaryText}
          danger={confirm.danger}
          onConfirm={confirm.onConfirm}
          onSecondary={confirm.onSecondary}
          onCancel={() => setConfirm(null)}
        />
      )}
      {pendingImport && (
        <DeepLinkImportDialog
          pending={pendingImport}
          onCancel={() => setPendingImport(null)}
          onConfirm={() => {
            const p = pendingImport;
            setPendingImport(null);
            saveProvider(p.app, p.provider)
              .then((r) => {
                setRoot(r);
                setTab(p.app);
                pushToast("success", `已导入「${p.name}」`);
              })
              .catch((e) => pushToast("error", String(e)));
          }}
        />
      )}
      <Toasts toasts={toasts} />
    </div>
  );
}
