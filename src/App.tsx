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
  proxyStatus,
  setProxyEnabled,
  type ActiveDeleteMode,
  type OriginalConfigStatus,
  type ProxyStatus,
} from "./api";
import ProviderModal from "./ProviderModal";
import SettingsModal from "./SettingsModal";
import ImportExportModal from "./ImportExportModal";
import StreamTestModal, { type StreamTestSummary } from "./StreamTestModal";
import Titlebar from "./Titlebar";
import ConfirmDialog from "./ConfirmDialog";
import DeepLinkImportDialog, { type PendingImport } from "./DeepLinkImportDialog";
import Toasts, { type Toast, type ToastKind } from "./Toasts";
import { buildClaudeProvider, buildCodexProvider, DEFAULT_CODEX_WIRE_API, isHttpUrl, maskSecret } from "./providerFactory";
import { checkForUpdate, installAndRelaunch, type Update } from "./updater";
import { onOpenUrl, getCurrent } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import { applyTheme, type Theme } from "./theme";
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
  SettingsIcon,
  TrashIcon,
  UploadIcon,
} from "./Icons";

const TABS: { key: AppType; label: string }[] = [
  { key: "claude", label: "Claude Code" },
  { key: "codex", label: "Codex" },
];

function initials(name: string): string {
  const s = name.trim();
  return /[一-龥]/.test(s) ? s.slice(0, 1) : s.slice(0, 2).toUpperCase();
}

function isOfficialProvider(provider: Provider): boolean {
  return provider.category === "official" || (provider.meta as any)?.kind === "officialLocal";
}

function summarize(p: Provider): { url: string; model: string } {
  const env = (p.settingsConfig as any)?.env ?? {};
  const cfg = (p.settingsConfig as any) ?? {};
  const url =
    env.ANTHROPIC_BASE_URL ??
    (cfg.config ? String(cfg.config).match(/base_url\s*=\s*"([^"]+)"/)?.[1] : "") ??
    "";
  const model =
    env.ANTHROPIC_MODEL ??
    (cfg.config ? String(cfg.config).match(/^model\s*=\s*"([^"]+)"/m)?.[1] : "") ??
    "";
  return { url: url || "", model: model || "" };
}

function slug(s: string): string {
  const t = s.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return t || `imported-${Math.floor(Math.random() * 1e6).toString(36)}`;
}

// 深链导入的身份判定：直接比对现有「名称」（slug 对中文会返回随机值，不能用来判同名）。
// 同名则给显示名加数字后缀（满血 → 满血 2），并保证 id 也唯一，绝不覆盖已有供应商。
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

function latClass(l?: Lat): string {
  if (!l || (l.ms == null && !l.err && !l.loading)) return "none";
  if (l.loading) return "none";
  if (l.err) return "slow";
  const ms = l.ms!;
  return ms <= 1000 ? "good" : ms <= 3000 ? "mid" : "slow";
}
function latText(l?: Lat): string {
  if (!l) return "未测";
  if (l.loading) return "…";
  if (l.err) return "超时";
  if (l.ms != null) return l.ms < 1 ? "<1ms" : `${Math.round(l.ms)}ms`;
  return "未测";
}
// 信号条点亮数：good=3 / mid=2 / slow=1 / 其它=0
function latBars(l?: Lat): number {
  switch (latClass(l)) {
    case "good":
      return 3;
    case "mid":
      return 2;
    case "slow":
      return 1;
    default:
      return 0;
  }
}
const BAR_HEIGHTS = [5, 8, 12];

export default function App() {
  const [root, setRoot] = useState<Root | null>(null);
  // 深链在 []-effect 里运行，闭包里的 root 会是挂载时的旧值；用 ref 取最新配置算唯一 id。
  const rootRef = useRef<Root | null>(null);
  rootRef.current = root;
  const [tab, setTab] = useState<AppType>("claude");
  const [lat, setLat] = useState<Record<string, Lat>>({});
  const [modal, setModal] = useState<null | { edit?: Provider }>(null);
  const [streamModal, setStreamModal] = useState<null | { app: AppType; provider: Provider }>(null);
  const [streamResults, setStreamResults] = useState<Record<string, StreamTestSummary>>({});
  const [showSettings, setShowSettings] = useState(false);
  const [io, setIo] = useState<null | "import" | "export">(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  const [originalStatus, setOriginalStatus] = useState<OriginalConfigStatus | null>(null);
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);
  const [proxy, setProxy] = useState<ProxyStatus | null>(null);
  const [proxyBusy, setProxyBusy] = useState(false);
  const [proxyRate, setProxyRate] = useState(0); // 估算「次/分」，仅体感提示
  const [nowTs, setNowTs] = useState(0);
  const rateSampleRef = useRef<{ total: number; ts: number } | null>(null);
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

  function pushToast(kind: ToastKind, msg: string) {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, kind, msg }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 2600);
  }

  useEffect(() => {
    getConfig().then(setRoot).catch((e) => pushToast("error", String(e)));
    originalConfigStatus().then(setOriginalStatus).catch(() => {});
    // 启动静默检查更新：有新版只置标识，绝不弹窗/toast 打扰用户。
    checkForUpdate()
      .then((update) => setUpdateInfo(update))
      .catch(() => {});
  }, []);

  // 轮询代理状态 + 本地活跃度（2s）：驱动底部状态栏，与设置页共用同一后端句柄。
  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const st = await proxyStatus();
        if (!alive) return;
        const now = Date.now();
        const prev = rateSampleRef.current;
        if (prev && st.total >= prev.total && now > prev.ts) {
          const rpm = ((st.total - prev.total) / ((now - prev.ts) / 1000)) * 60;
          // 指数平滑，避免单请求造成的尖峰抖动
          setProxyRate((r) => Math.round(r * 0.5 + rpm * 0.5));
        } else if (prev && st.total < prev.total) {
          setProxyRate(0); // 代理重启，频率基线重置
        }
        rateSampleRef.current = { total: st.total, ts: now };
        setProxy(st);
        setNowTs(now);
      } catch {
        /* 忽略：代理未起或命令暂不可用 */
      }
    }
    poll();
    const id = setInterval(poll, 2000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // 底部「一键路由」：复用设置页 toggleProxy 逻辑（开=起服务指向 localhost；关=恢复直连）。
  function toggleRouting() {
    if (proxyBusy || !proxy) return;
    const next = !proxy.enabled;
    const port = proxy.port;
    setProxyBusy(true);
    setProxy((p) => (p ? { ...p, enabled: next } : p)); // 乐观更新
    setProxyEnabled(next)
      .then((r) => {
        setRoot(r);
        pushToast(
          "success",
          next
            ? `本地路由已开启（127.0.0.1:${port}）· 切换供应商无需重启客户端`
            : "本地路由已关闭，已恢复直连",
        );
      })
      .catch((e) => {
        setProxy((p) => (p ? { ...p, enabled: !next } : p)); // 失败回滚
        pushToast("error", "切换本地路由失败：" + String(e));
      })
      .finally(() => setProxyBusy(false));
  }

  // 设置页「检查更新」手动触发：无更新时给一次成功提示。
  async function handleCheckUpdate() {
    const update = await checkForUpdate();
    setUpdateInfo(update);
    if (!update) pushToast("success", "已是最新版本");
  }

  // 下载并安装更新，完成后自动重启。
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

  // 应用主题（settings.theme 变化时）
  const theme = (root?.settings as any)?.theme as Theme | undefined;
  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

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

  // 深链 zswitch://import?... 一键导入
  useEffect(() => {
    function parseAndImport(url: string) {
      try {
        const u = new URL(url);
        const q = u.searchParams;
        const appk = (q.get("app") === "codex" ? "codex" : "claude") as AppType;
        const nm = q.get("name") || "导入的供应商";
        const baseUrl = (q.get("baseUrl") || "").trim();
        // 密钥：支持明文 key，或 base64 的 keyB64（减少原文出现在 URL 里）。
        let key = q.get("key") || "";
        const kb64 = q.get("keyB64");
        if (!key && kb64) {
          try {
            key = atob(kb64);
          } catch {
            /* 非法 base64，忽略，按未提供处理 */
          }
        }
        const model = q.get("model") || "";
        // 校验：接入点必须是 http(s)，否则拒绝（不弹框、不落盘）。
        if (!isHttpUrl(baseUrl)) {
          pushToast("error", "链接被拒绝：接入点必须是 http(s) 地址");
          return;
        }
        // 绝不覆盖已有供应商：同名则加数字后缀新增一张（保证 name 与 id 都唯一），
        // 让不同分组（满血 / 其他渠道 …）能并存，也让重复导入可区分而非静默盖掉。
        const providers = rootRef.current?.apps[appk]?.providers ?? {};
        const { name: finalName, id, collision } = uniqueImportIdentity(nm, providers);
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
                  haiku: q.get("haiku") || undefined,
                  sonnet: q.get("sonnet") || undefined,
                  opus: q.get("opus") || undefined,
                  fable: q.get("fable") || undefined,
                },
                key,
              );
        // 不直接落盘：暂存并弹确认框，让用户看清再导入（安全底线）。
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
    const label = app === "claude" ? "Claude Code" : "Codex";
    setShowSettings(false);
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
      const { url } = summarize(p);
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

  const existingIds = data ? Object.keys(data.providers) : [];

  // 底部状态栏派生值
  const proxyOn = proxy?.enabled ?? false;
  const proxyPort = proxy?.port ?? 8899;
  const idleSec =
    proxy && proxy.lastActivityMs > 0
      ? Math.max(0, Math.floor((nowTs - proxy.lastActivityMs) / 1000))
      : null;
  const proxyActive = proxyOn && ((proxy?.inFlight ?? 0) > 0 || (idleSec != null && idleSec < 10));
  const currentName = data?.current ? data.providers[data.current]?.name ?? null : null;
  const activeSegments: string[] = [];
  if ((proxy?.inFlight ?? 0) > 0) activeSegments.push(`${proxy!.inFlight} 进行中`);
  if (proxyRate >= 1) activeSegments.push(`~${proxyRate} 次/分`);
  activeSegments.push("活跃");

  return (
    <div className="app">
      <Titlebar />

      <div className="toolbar">
        <div className="segmented">
          {TABS.map((t) => (
            <button key={t.key} className={"seg" + (tab === t.key ? " active" : "")} onClick={() => setTab(t.key)}>
              <span className="dot" />
              {t.label}
            </button>
          ))}
        </div>
        <div className="tools">
          <button
            className="icon-btn"
            onClick={() => openHelpDocument().catch((error) => pushToast("error", String(error)))}
            title="打开使用帮助文档"
          >
            <BookOpenIcon />文档
          </button>
          <button className="icon-btn" onClick={speedtestAll} title="测速全部端点"><BoltIcon />测速</button>
          <button className="icon-btn icon-only" onClick={() => setIo("import")} title="导入 JSON" aria-label="导入 JSON"><DownloadIcon /></button>
          <button className="icon-btn icon-only" onClick={() => setIo("export")} title="导出 JSON" aria-label="导出 JSON"><UploadIcon /></button>
          <button className={"icon-btn icon-only" + (updateInfo ? " has-badge" : "")} onClick={() => setShowSettings(true)} title={updateInfo ? "设置 · 有新版本可用" : "设置"} aria-label="设置"><SettingsIcon />{updateInfo && <span className="update-dot" />}</button>
          <button className="btn accent" onClick={() => setModal({})}><PlusIcon />添加</button>
        </div>
      </div>

      <div className={"list" + (dragId ? " sorting" : "")}>
        {ordered.length === 0 && (
          <div className="empty">
            <div className="empty-ico"><InboxIcon /></div>
            <h3>还没有供应商</h3>
            <p>从现有配置导入，或点右上角「添加」快速开始。</p>
            <div className="empty-row">
              <button className="btn" onClick={onImportLive}><DownloadIcon />导入现有配置</button>
              <button className="btn accent" onClick={() => setModal({})}><PlusIcon />添加供应商</button>
            </div>
          </div>
        )}
        {ordered.map((p) => {
          const active = data?.current === p.id;
          const official = isOfficialProvider(p);
          const { url, model } = summarize(p);
          const l = lat[`${tab}:${p.id}`];
          const streamResult = streamResults[`${tab}:${p.id}`];
          return (
            <div
              key={p.id}
              data-provider-id={p.id}
              className={
                "card" +
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
                className="avatar drag-handle"
                style={{ background: (p.meta as any)?.iconColor ?? "#4a5bd4" }}
                title="按住头像拖拽排序"
                onPointerDown={(e) => onSortPointerDown(e, p.id)}
                onPointerMove={onSortPointerMove}
                onPointerUp={onSortPointerUp}
                onPointerCancel={onSortPointerCancel}
              >
                {initials(p.name)}
              </span>
              <div className="meta">
                <div className="row1">
                  <span className="name">{p.name}</span>
                  {official && <span className="official-badge">官方账号</span>}
                  {!official && tab === "codex" && (p.meta as any)?.wireApi && (
                    <span className="model">wire_api·{(p.meta as any).wireApi}</span>
                  )}
                </div>
                <div className="row2">
                  {official ? (
                    <span className="url">使用本机 {tab === "claude" ? "Claude Code" : "Codex"} 登录状态</span>
                  ) : (
                    <>
                      <span className="url">{url || "—"}</span>
                      {model && <span className="model">{model}</span>}
                    </>
                  )}
                </div>
              </div>
              <div className="cluster">
                {official ? (
                  <span className="local-account-state"><span className="pulse" />本机登录</span>
                ) : (
                  <span className={"lat " + latClass(l)}>
                    <span className="bar">
                      {BAR_HEIGHTS.map((h, i) => (
                        <i key={i} className={i < latBars(l) ? "on" : ""} style={{ height: h }} />
                      ))}
                    </span>
                    {latText(l)}
                  </span>
                )}
                {active ? (
                  <span className="in-use"><CheckIcon />已生效</span>
                ) : (
                  <button className="use-btn" onClick={() => run(switchProvider(tab, p.id), `已切换到 ${p.name}`)}>切换</button>
                )}
                {!official && (
                  <>
                    <button
                      className={"provider-test-btn provider-test-icon" + (streamResult ? ` ${streamResult.status}` : "")}
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
                      className="card-icon-btn copy-btn"
                      onClick={() => onCopyProvider(p)}
                      title="复制供应商"
                      aria-label={`复制供应商 ${p.name}`}
                    >
                      <CopyIcon />
                    </button>
                    <button className="card-icon-btn" onClick={() => setModal({ edit: p })} title="编辑" aria-label={`编辑供应商 ${p.name}`}><EditIcon /></button>
                    <button className="card-icon-btn danger-ghost" onClick={() => onDelete(p)} title="删除" aria-label={`删除供应商 ${p.name}`}><TrashIcon /></button>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>

      <div className="statusbar">
        {proxyOn ? (
          <>
            <span className="si">
              <span className={"sb-dot on" + (proxyActive ? " live" : "")} />
              代理 <b>ON</b> · 127.0.0.1:{proxyPort}
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
          <label className={"sb-route" + (proxyBusy ? " busy" : "")}>
            一键路由
            <button
              type="button"
              className={"switch" + (proxyOn ? " on" : "")}
              role="switch"
              aria-checked={proxyOn}
              aria-label="一键开启本地路由"
              onClick={toggleRouting}
            />
          </label>
        </div>
      </div>

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
      {showSettings && root && (
        <SettingsModal
          settings={(root.settings as Record<string, any>) ?? {}}
          originalStatus={originalStatus}
          onClose={() => setShowSettings(false)}
          onSave={onSaveSettings}
          onRestoreOriginal={onRestoreOriginal}
          onToast={pushToast}
          updateInfo={updateInfo}
          updateBusy={updateBusy}
          updateProgress={updateProgress}
          onCheckUpdate={handleCheckUpdate}
          onInstallUpdate={handleInstallUpdate}
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
