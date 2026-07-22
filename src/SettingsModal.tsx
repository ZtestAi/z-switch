import { useEffect, useState } from "react";
import {
  clearProxyErrorLog,
  openBackupsFolder,
  openProxyLogFolder,
  openConfigDir,
  proxyStatus,
  setAutoLaunch,
  setClaudeDesktopEnabled,
  setClaudeOnboardingSkip,
  setClaudePluginEnabled,
  setAppRouting,
  environmentDiagnose,
  environmentRepair,
  type OriginalConfigStatus,
  type EnvDiagnosis,
} from "./api";
import type { AppType, Root } from "./types";
import type { Update } from "./updater";
import { getVersion } from "@tauri-apps/api/app";
import { ChevronDownIcon, ChevronRightIcon, ClockIcon, CloseIcon, DownloadIcon, FolderIcon, TrashIcon, UploadIcon } from "./Icons";

type SettingsTab = "general" | "integration" | "recover" | "about";

const TABS: { id: SettingsTab; label: string }[] = [
  { id: "general", label: "常规" },
  { id: "integration", label: "集成" },
  { id: "recover", label: "恢复与迁移" },
  { id: "about", label: "关于" },
];

interface Props {
  settings: Record<string, any>;
  originalStatus: OriginalConfigStatus | null;
  onClose: () => void;
  onSave: (s: Record<string, any>) => void;
  onRestoreOriginal: (app: AppType) => void;
  onRestoreOfficialBaseline?: (app: AppType) => void;
  onToast?: (kind: "success" | "error", msg: string) => void;
  updateInfo: Update | null;
  updateBusy: boolean;
  updateProgress: number | null;
  onCheckUpdate: () => Promise<void>;
  onInstallUpdate: () => void;
  /** 以应用内页面（非浮层弹窗）形式渲染 */
  asPage?: boolean;
  onOpenImport?: () => void;
  onOpenExport?: () => void;
  onOpenCcswitch?: () => void;
  onRepaired?: (root: Root) => void;
}

function EnvRepairSection({
  onToast,
  onRepaired,
}: {
  onToast?: (kind: "success" | "error", msg: string) => void;
  onRepaired?: (root: Root) => void;
}) {
  const [diag, setDiag] = useState<EnvDiagnosis | null>(null);
  const [busy, setBusy] = useState<AppType | null>(null);
  const [scanning, setScanning] = useState(false);

  function refresh() {
    setScanning(true);
    environmentDiagnose()
      .then(setDiag)
      .catch(() => {})
      .finally(() => setScanning(false));
  }
  useEffect(() => {
    refresh();
  }, []);

  function repair(app: AppType) {
    setBusy(app);
    environmentRepair(app)
      .then((root) => {
        onRepaired?.(root);
        onToast?.("success", `${app === "claude" ? "Claude Code" : "Codex"} 环境已修复为直连`);
        refresh();
      })
      .catch((e) => onToast?.("error", "修复失败：" + String(e)))
      .finally(() => setBusy(null));
  }

  const apps = diag?.apps ?? [];
  return (
    <div className="set-group">
      <h4>环境自检与修复</h4>
      <div className="set-row" style={{ display: "block" }}>
        <div className="set-copy">
          <div className="d">
            检测 live 是否残留本地代理占位（127.0.0.1 / 占位密钥）。异常时可一键恢复为直连当前供应商。
          </div>
        </div>
      </div>
      {apps.map((a) => (
        <div className="set-row" key={a.app}>
          <div>
            <div className={"l" + (a.healthy ? "" : " status-running")}>
              {!a.healthy && <span className="status-dot" style={{ background: "var(--danger)" }} />}
              {a.app === "claude" ? "Claude Code" : "Codex"} · {a.healthy ? (a.routed ? "本地路由中" : "正常") : "异常"}
            </div>
            <div className="d">
              {a.healthy
                ? a.routed
                  ? "由本地路由托管（localhost 属正常状态）"
                  : `直连${a.currentName ? " · " + a.currentName : ""}${a.liveBaseUrl ? " · " + a.liveBaseUrl : ""}`
                : a.issue ?? "检测到异常"}
            </div>
          </div>
          {a.fixable && (
            <button type="button" className="btn" disabled={busy === a.app} onClick={() => repair(a.app)}>
              {busy === a.app ? "修复中…" : "一键修复"}
            </button>
          )}
        </div>
      ))}
      <div className="set-row" style={{ display: "block" }}>
        <button className="adv-toggle" onClick={refresh} disabled={scanning}>
          {scanning ? "检测中…" : "重新检测"}
        </button>
      </div>
    </div>
  );
}

export default function SettingsModal({
  settings,
  originalStatus,
  onClose,
  onSave,
  onRestoreOriginal,
  onRestoreOfficialBaseline,
  onToast,
  updateInfo,
  updateBusy,
  updateProgress,
  onCheckUpdate,
  onInstallUpdate,
  asPage,
  onOpenImport,
  onOpenExport,
  onOpenCcswitch,
  onRepaired,
}: Props) {
  const [version, setVersion] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [tab, setTab] = useState<SettingsTab>("general");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  async function checkUpdate() {
    if (checkingUpdate) return;
    setCheckingUpdate(true);
    try {
      await onCheckUpdate();
    } catch (e) {
      console.error("[z-switch] 检查更新失败:", e);
      onToast?.("error", "检查更新失败，请检查网络后重试");
    } finally {
      setCheckingUpdate(false);
    }
  }
  const [s, setS] = useState<Record<string, any>>({ ...settings });
  const [routed, setRouted] = useState<{ claude: boolean; codex: boolean }>({ claude: false, codex: false });
  const [proxyPort, setProxyPort] = useState(8899);
  const [proxyBusy, setProxyBusy] = useState<AppType | null>(null);
  const [showProxyAdvanced, setShowProxyAdvanced] = useState(false);
  const [clearLogArmed, setClearLogArmed] = useState(false);
  const anyRouted = routed.claude || routed.codex;

  function refreshProxy() {
    proxyStatus()
      .then((st) => {
        setProxyPort(st.port);
        setRouted({ claude: st.claude.routed, codex: st.codex.routed });
      })
      .catch(() => {});
  }
  useEffect(() => {
    refreshProxy();
  }, []);

  function set(key: string, value: unknown) {
    const next = { ...s, [key]: value };
    setS(next);
    onSave(next);
  }

  function setReliability(key: string, value: unknown) {
    const next = {
      ...s,
      reliability: { ...(s.reliability ?? {}), [key]: value },
    };
    setS(next);
    onSave(next);
  }

  function reliabilityNumber(key: string, fallback: number) {
    const value = Number(s.reliability?.[key]);
    return Number.isFinite(value) && value > 0 ? value : fallback;
  }

  function toggleAutoLaunch() {
    const next = !s.autoLaunch;
    setS((cur) => ({ ...cur, autoLaunch: next }));
    setAutoLaunch(next).catch((e) => {
      setS((cur) => ({ ...cur, autoLaunch: !next }));
      onToast?.("error", "设置开机自启失败：" + String(e));
    });
  }

  function toggleClaudePlugin() {
    const next = !s.applyClaudePlugin;
    const nextS = { ...s, applyClaudePlugin: next };
    setS(nextS);
    onSave(nextS);
    setClaudePluginEnabled(next).catch((e) => {
      const revertS = { ...s, applyClaudePlugin: !next };
      setS(revertS);
      onSave(revertS);
      onToast?.("error", "设置「应用到 Claude Code 插件」失败：" + String(e));
    });
  }

  function toggleClaudeOnboarding() {
    const next = !s.skipClaudeOnboarding;
    const nextS = { ...s, skipClaudeOnboarding: next };
    setS(nextS);
    onSave(nextS);
    setClaudeOnboardingSkip(next).catch((e) => {
      const revertS = { ...s, skipClaudeOnboarding: !next };
      setS(revertS);
      onSave(revertS);
      onToast?.("error", "设置「跳过初次安装确认」失败：" + String(e));
    });
  }

  function toggleClaudeDesktop() {
    const next = !s.applyClaudeDesktop;
    const nextS = { ...s, applyClaudeDesktop: next };
    setS(nextS);
    onSave(nextS);
    setClaudeDesktopEnabled(next)
      .then(() => {
        onToast?.(
          "success",
          next
            ? "已开启 Claude 桌面版随切换 · 重启桌面 App 后生效"
            : "已关闭 Claude 桌面版随切换 · 桌面版已退回官方"
        );
      })
      .catch((e) => {
        const revertS = { ...s, applyClaudeDesktop: !next };
        setS(revertS);
        onSave(revertS);
        onToast?.("error", "设置「Claude 桌面版随切换」失败：" + String(e));
      });
  }

  function openBackupDirectory() {
    openBackupsFolder().catch((error) => {
      onToast?.("error", "打开备份文件夹失败：" + String(error));
    });
  }

  function openDir(kind: "claude" | "codex" | "grok" | "app") {
    openConfigDir(kind).catch((error) => {
      onToast?.("error", "打开配置目录失败：" + String(error));
    });
  }

  function openProxyLogs() {
    openProxyLogFolder().catch((error) => {
      onToast?.("error", "打开错误日志目录失败：" + String(error));
    });
  }

  function clearProxyLogs() {
    if (!clearLogArmed) {
      setClearLogArmed(true);
      window.setTimeout(() => setClearLogArmed(false), 3000);
      return;
    }
    clearProxyErrorLog()
      .then(() => onToast?.("success", "本地路由错误日志已清空"))
      .catch((error) => onToast?.("error", "清空错误日志失败：" + String(error)))
      .finally(() => setClearLogArmed(false));
  }

  function toggleAppProxy(app: AppType) {
    if (app === "grok" || proxyBusy) return;
    const next = !routed[app];
    const label = app === "claude" ? "Claude Code" : "Codex";
    setProxyBusy(app);
    setAppRouting(app, next)
      .then(() => {
        setRouted((r) => ({ ...r, [app]: next }));
        refreshProxy();
        onToast?.(
          "success",
          next
            ? `${label} 已开启本地路由（127.0.0.1:${proxyPort}/${app}）· 切换供应商无需重启`
            : `${label} 已关闭本地路由，已恢复直连`
        );
      })
      .catch((e) => onToast?.("error", "切换本地路由失败：" + String(e)))
      .finally(() => setProxyBusy(null));
  }

  const capturedAt = originalStatus?.capturedAt
    ? new Date(originalStatus.capturedAt).toLocaleString("zh-CN", { hour12: false })
    : null;

  const tabs = (
    <div className="settings-tabs" role="tablist" aria-label="设置分类">
      {TABS.map((t) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          aria-selected={tab === t.id}
          className={tab === t.id ? "on" : ""}
          onClick={() => setTab(t.id)}
        >
          {t.label}
          {t.id === "about" && updateInfo && <span className="settings-tab-dot" aria-hidden />}
        </button>
      ))}
    </div>
  );

  const generalPanel = (
    <>
      <div className="set-group">
        <h4>外观与启动</h4>
        <div className="set-row">
          <div>
            <div className="l">主题</div>
            <div className="d">跟随系统 / 浅色 / 深色</div>
          </div>
          <div className="seg-mini">
            {(["system", "light", "dark"] as const).map((t) => (
              <button
                key={t}
                className={(s.theme ?? "light") === t ? "on" : ""}
                onClick={() => set("theme", t)}
              >
                {t === "system" ? "跟随" : t === "light" ? "浅色" : "深色"}
              </button>
            ))}
          </div>
        </div>
        <div className="set-row">
          <div>
            <div className="l">开机自启</div>
            <div className="d">登录系统时自动启动 z-switch 到托盘</div>
          </div>
          <button type="button" className={"switch" + (s.autoLaunch ? " on" : "")} role="switch" aria-checked={!!s.autoLaunch} aria-label="开机自启" onClick={toggleAutoLaunch} />
        </div>
        <div className="set-row backup-row">
          <div className="set-copy">
            <div className="l">写入前备份</div>
            <div className="d">切换前备份到 ~/.z-switch/backups/</div>
          </div>
          <div className="backup-controls">
            <button type="button" className="btn folder-btn" onClick={openBackupDirectory}>
              <FolderIcon />打开文件夹
            </button>
            <button type="button" className={"switch" + (s.backupBeforeWrite !== false ? " on" : "")} role="switch" aria-checked={s.backupBeforeWrite !== false} aria-label="写入前备份" onClick={() => set("backupBeforeWrite", s.backupBeforeWrite === false)} />
          </div>
        </div>
      </div>

      <div className="set-group">
        <h4>本地路由 · 热切换</h4>
        <div className="set-row" style={{ display: "block" }}>
          <div className="set-copy">
            <div className="d">
              开启后指向 127.0.0.1:{proxyPort}/&lt;app&gt;，切换供应商无需重启。请求仅在本机转发，不上传。
            </div>
          </div>
        </div>
        {(["claude", "codex"] as const).map((app) => (
          <div className="set-row" key={app}>
            <div>
              <div className={"l" + (routed[app] ? " status-running" : "")}>
                {routed[app] && <span className="status-dot" />}
                {app === "claude" ? "Claude Code" : "Codex"} 本地路由
              </div>
              <div className="d">
                {routed[app]
                  ? `运行中 · 127.0.0.1:${proxyPort}/${app}`
                  : "直连模式 · 切换后需重启客户端"}
              </div>
            </div>
            <button
              type="button"
              className={"switch" + (routed[app] ? " on" : "") + (proxyBusy === app ? " busy" : "")}
              role="switch"
              aria-checked={routed[app]}
              aria-label={`${app === "claude" ? "Claude Code" : "Codex"} 本地路由`}
              onClick={() => toggleAppProxy(app)}
              style={proxyBusy === app ? { opacity: 0.5, pointerEvents: "none" } : undefined}
            />
          </div>
        ))}
        <button className="adv-toggle proxy-adv-toggle" onClick={() => setShowProxyAdvanced((value) => !value)}>
          {showProxyAdvanced ? <ChevronDownIcon /> : <ChevronRightIcon />}
          高级设置
        </button>
        {showProxyAdvanced && (
          <div className="set-row proxy-advanced-row">
            <div className="proxy-fixed-capabilities">
              <span><strong>流式返回</strong> 始终开启</span>
              <span><strong>连接复用</strong> 始终开启</span>
            </div>
            <div className="proxy-config-grid">
              {[
                ["connectTimeoutSeconds", "连接超时", "秒", 10, 1, 120],
                ["streamingFirstByteTimeoutSeconds", "流式首段超时", "秒", 60, 5, 300],
                ["streamingIdleTimeoutSeconds", "流式静默超时", "秒", 120, 10, 900],
                ["nonStreamingTimeoutSeconds", "非流式总超时", "秒", 600, 30, 3600],
                ["requestBodyLimitMb", "请求体上限", "MB", 64, 1, 256],
                ["poolMaxIdlePerHost", "每站空闲连接", "个", 10, 1, 100],
                ["tcpKeepaliveSeconds", "TCP Keepalive", "秒", 60, 10, 600],
                ["proxyErrorLogMaxMb", "错误日志上限", "MB", 5, 1, 100],
              ].map(([key, label, unit, fallback, min, max]) => (
                <label className="proxy-number-field" key={String(key)}>
                  <span>{label}</span>
                  <span className="proxy-number-control">
                    <input
                      className="mono"
                      type="number"
                      min={Number(min)}
                      max={Number(max)}
                      value={reliabilityNumber(String(key), Number(fallback))}
                      onChange={(event) => {
                        const value = Number(event.target.value);
                        if (Number.isFinite(value) && value > 0) setReliability(String(key), value);
                      }}
                    />
                    <em>{unit}</em>
                  </span>
                </label>
              ))}
            </div>
            <div className="proxy-log-setting">
              <div className="set-copy">
                <div className="l">记录真实上游错误</div>
                <div className="d">只记录失败状态与超时；自动脱敏密钥，不记录请求正文。</div>
              </div>
              <button
                type="button"
                className={"switch" + (s.reliability?.proxyErrorLogEnabled !== false ? " on" : "")}
                role="switch"
                aria-checked={s.reliability?.proxyErrorLogEnabled !== false}
                aria-label="记录真实上游错误"
                onClick={() => setReliability("proxyErrorLogEnabled", s.reliability?.proxyErrorLogEnabled === false)}
              />
            </div>
            <div className="proxy-log-actions">
              <span>{anyRouted ? "参数修改后，重新开启本地路由生效。" : "参数将在下次开启本地路由时生效。"}</span>
              <div>
                <button className="btn" onClick={openProxyLogs}><FolderIcon />打开日志目录</button>
                <button className={"btn" + (clearLogArmed ? " danger" : "")} onClick={clearProxyLogs}>
                  <TrashIcon />{clearLogArmed ? "确认清空" : "清空日志"}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </>
  );

  const integrationPanel = (
    <>
      <div className="set-group">
        <h4>Claude Code</h4>
        <div className="set-row">
          <div className="set-copy">
            <div className="l">应用到 Claude Code 插件</div>
            <div className="d">VS Code 扩展供应商随本软件切换（写 primaryApiKey）</div>
          </div>
          <button type="button" className={"switch" + (s.applyClaudePlugin ? " on" : "")} role="switch" aria-checked={!!s.applyClaudePlugin} aria-label="应用到 Claude Code 插件" onClick={toggleClaudePlugin} />
        </div>
        <div className="set-row">
          <div className="set-copy">
            <div className="l">跳过初次安装确认</div>
            <div className="d">写入 hasCompletedOnboarding，跳过首次引导</div>
          </div>
          <button type="button" className={"switch" + (s.skipClaudeOnboarding ? " on" : "")} role="switch" aria-checked={!!s.skipClaudeOnboarding} aria-label="跳过 Claude Code 初次安装确认" onClick={toggleClaudeOnboarding} />
        </div>
      </div>

      <div className="set-group">
        <h4>Claude 桌面版</h4>
        <div className="set-row">
          <div className="set-copy">
            <div className="l">桌面版随切换生效</div>
            <div className="d">
              独立聊天 App 跟随当前 Claude 供应商。仅 macOS / Windows，<b>需重启桌面 App</b>。
            </div>
          </div>
          <button type="button" className={"switch" + (s.applyClaudeDesktop ? " on" : "")} role="switch" aria-checked={!!s.applyClaudeDesktop} aria-label="Claude 桌面版随切换生效" onClick={toggleClaudeDesktop} />
        </div>
      </div>

      <div className="set-group">
        <h4>配置目录</h4>
        <div className="set-row config-dir-row">
          <div className="set-copy">
            <div className="l">快速打开</div>
            <div className="d">在文件管理器中打开对应目录（不存在时打开用户主目录）</div>
          </div>
          <div className="dir-actions">
            <button type="button" className="btn folder-btn" onClick={() => openDir("claude")}><FolderIcon />Claude</button>
            <button type="button" className="btn folder-btn" onClick={() => openDir("codex")}><FolderIcon />Codex</button>
            <button type="button" className="btn folder-btn" onClick={() => openDir("grok")}><FolderIcon />Grok</button>
            <button type="button" className="btn folder-btn" onClick={() => openDir("app")}><FolderIcon />z-switch</button>
          </div>
        </div>
      </div>
    </>
  );

  const recoverPanel = (
    <>
      <EnvRepairSection onToast={onToast} onRepaired={onRepaired} />

      <div className="set-group">
        <h4>重置与恢复</h4>
        <div className="set-row original-config-row">
          <div className="original-config-head">
            <div>
              <div className="l">重置为官方账号配置</div>
              <div className="d">不依赖首启快照；文件损坏也能强制写入干净官方基线（写前备份）</div>
            </div>
          </div>
          <div className="original-actions">
            <button className="btn" onClick={() => onRestoreOfficialBaseline?.("claude")}>重置 Claude</button>
            <button className="btn" onClick={() => onRestoreOfficialBaseline?.("codex")}>重置 Codex</button>
          </div>
        </div>
        <div className="set-row original-config-row">
          <div className="original-config-head">
            <div>
              <div className="l">本机原始配置</div>
              <div className="d">首次打开 z-switch 时保存的恢复基线</div>
            </div>
            {originalStatus?.captured && (
              <span className="snapshot-time"><ClockIcon />{capturedAt}</span>
            )}
          </div>
          {originalStatus?.captured ? (
            <div className="original-status-grid">
              <div className="original-status-item">
                <span>Claude</span>
                <strong>{originalStatus.claudeHadConfig ? "已保存配置" : "首次为空"}</strong>
              </div>
              <div className="original-status-item">
                <span>Codex</span>
                <strong>{originalStatus.codexHadConfig ? "已保存配置" : "首次为空"}</strong>
              </div>
              <div className="original-status-item">
                <span>Grok</span>
                <strong>{originalStatus.grokHadConfig ? "已保存配置" : "首次为空"}</strong>
              </div>
            </div>
          ) : (
            <div className="original-unavailable">原始配置快照尚不可用</div>
          )}
          <div className="original-actions">
            <button className="btn" disabled={!originalStatus?.captured} onClick={() => onRestoreOriginal("claude")}>恢复 Claude</button>
            <button className="btn" disabled={!originalStatus?.captured} onClick={() => onRestoreOriginal("codex")}>恢复 Codex</button>
            <button className="btn" disabled={!originalStatus?.captured} onClick={() => onRestoreOriginal("grok")}>恢复 Grok</button>
          </div>
        </div>
      </div>

      <div className="set-group">
        <h4>备份与迁移</h4>
        <div className="set-row config-dir-row">
          <div className="set-copy">
            <div className="l">配置导入 / 导出</div>
            <div className="d">整份 JSON 迁移（不含官方账号本机登录凭据）</div>
          </div>
          <div className="dir-actions">
            <button type="button" className="btn folder-btn" onClick={() => onOpenImport?.()}><DownloadIcon />导入配置</button>
            <button type="button" className="btn folder-btn" onClick={() => onOpenExport?.()}><UploadIcon />导出配置</button>
          </div>
        </div>
        <div className="set-row config-dir-row">
          <div className="set-copy">
            <div className="l">从 cc-switch 导入</div>
            <div className="d">扫描 ~/.cc-switch，迁入 Claude / Codex 供应商（新增，不改当前生效项）</div>
          </div>
          <div className="dir-actions">
            <button type="button" className="btn folder-btn" onClick={() => onOpenCcswitch?.()}><DownloadIcon />从 cc-switch 导入</button>
          </div>
        </div>
      </div>
    </>
  );

  const aboutPanel = (
    <div className="set-group">
      <h4>关于</h4>
      <div className="set-row">
        <div>
          <div className="l">z-switch{version ? ` v${version}` : ""}</div>
          <div className="d">开源 · 无广告 · 由 真测 Ztest 出品</div>
        </div>
        <button
          type="button"
          className="btn"
          onClick={checkUpdate}
          disabled={checkingUpdate || updateBusy}
        >
          {checkingUpdate ? "检查中…" : "检查更新"}
        </button>
      </div>
      {updateInfo && (
        <div className="set-row update-row">
          <div>
            <div className="l status-running"><span className="status-dot" />发现新版本 v{updateInfo.version}</div>
            <div className="d">
              {updateBusy
                ? updateProgress != null
                  ? `正在下载并安装… ${updateProgress}%`
                  : "正在下载并安装…"
                : "下载完成后将自动重启应用完成更新。"}
            </div>
          </div>
          <button
            type="button"
            className="btn accent"
            onClick={onInstallUpdate}
            disabled={updateBusy}
          >
            {updateBusy ? "更新中…" : "更新并重启"}
          </button>
        </div>
      )}
    </div>
  );

  const panel =
    tab === "general" ? generalPanel
      : tab === "integration" ? integrationPanel
        : tab === "recover" ? recoverPanel
          : aboutPanel;

  const body = (
    <>
      {tabs}
      <div className="settings-tab-panel" role="tabpanel">
        {panel}
      </div>
    </>
  );

  if (asPage) {
    return (
      <div className="settings-page">
        <div className="settings-page-inner">{body}</div>
      </div>
    );
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>设置</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">{body}</div>
      </div>
    </div>
  );
}
