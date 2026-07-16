import { useEffect, useState } from "react";
import {
  clearProxyErrorLog,
  openBackupsFolder,
  openProxyLogFolder,
  proxyStatus,
  setAutoLaunch,
  setProxyEnabled,
  type OriginalConfigStatus,
} from "./api";
import type { AppType } from "./types";
import type { Update } from "./updater";
import { getVersion } from "@tauri-apps/api/app";
import { ChevronDownIcon, ChevronRightIcon, ClockIcon, CloseIcon, FolderIcon, TrashIcon } from "./Icons";

interface Props {
  settings: Record<string, any>;
  originalStatus: OriginalConfigStatus | null;
  onClose: () => void;
  onSave: (s: Record<string, any>) => void;
  onRestoreOriginal: (app: AppType) => void;
  onToast?: (kind: "success" | "error", msg: string) => void;
  updateInfo: Update | null;
  updateBusy: boolean;
  updateProgress: number | null;
  onCheckUpdate: () => Promise<void>;
  onInstallUpdate: () => void;
}

export default function SettingsModal({
  settings,
  originalStatus,
  onClose,
  onSave,
  onRestoreOriginal,
  onToast,
  updateInfo,
  updateBusy,
  updateProgress,
  onCheckUpdate,
  onInstallUpdate,
}: Props) {
  const [version, setVersion] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  async function checkUpdate() {
    if (checkingUpdate) return;
    setCheckingUpdate(true);
    try {
      await onCheckUpdate();
    } catch (e) {
      onToast?.("error", "检查更新失败：" + String(e));
    } finally {
      setCheckingUpdate(false);
    }
  }
  const [s, setS] = useState<Record<string, any>>({ ...settings });
  const [proxyOn, setProxyOn] = useState(false);
  const [proxyPort, setProxyPort] = useState(8899);
  const [proxyBusy, setProxyBusy] = useState(false);
  const [showProxyAdvanced, setShowProxyAdvanced] = useState(false);
  const [clearLogArmed, setClearLogArmed] = useState(false);

  // 进设置时读一次代理真实运行状态（以后端为准，不只看持久化 flag）
  useEffect(() => {
    proxyStatus()
      .then((st) => {
        setProxyOn(st.enabled);
        setProxyPort(st.port);
      })
      .catch(() => {});
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

  // 开机自启走专门命令（同步系统 + 持久化），本地乐观更新，失败回滚
  function toggleAutoLaunch() {
    const next = !s.autoLaunch;
    setS((cur) => ({ ...cur, autoLaunch: next }));
    setAutoLaunch(next).catch((e) => {
      setS((cur) => ({ ...cur, autoLaunch: !next }));
      onToast?.("error", "设置开机自启失败：" + String(e));
    });
  }

  function openBackupDirectory() {
    openBackupsFolder().catch((error) => {
      onToast?.("error", "打开备份文件夹失败：" + String(error));
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

  // 本地热切换代理：开=起服务并把 live 指向 localhost；关=恢复直连。含 live 写盘，需 await。
  function toggleProxy() {
    if (proxyBusy) return;
    const next = !proxyOn;
    setProxyBusy(true);
    setProxyEnabled(next)
      .then(() => {
        setProxyOn(next);
        onToast?.(
          "success",
          next
            ? `本地路由已开启（127.0.0.1:${proxyPort}）· 切换供应商无需重启客户端`
            : "本地路由已关闭，已恢复直连"
        );
      })
      .catch((e) => onToast?.("error", "切换本地路由失败：" + String(e)))
      .finally(() => setProxyBusy(false));
  }

  const capturedAt = originalStatus?.capturedAt
    ? new Date(originalStatus.capturedAt).toLocaleString("zh-CN", { hour12: false })
    : null;

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>设置</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          <div className="set-group">
            <h4>外观</h4>
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
          </div>

          <div className="set-group">
            <h4>安全</h4>
            <div className="set-row backup-row">
              <div className="set-copy">
                <div className="l">写入前备份</div>
                <div className="d">切换前对 settings.json / config.toml 备份到 ~/.z-switch/backups/</div>
              </div>
              <div className="backup-controls">
                <button type="button" className="btn folder-btn" onClick={openBackupDirectory}>
                  <FolderIcon />打开文件夹
                </button>
                <button type="button" className={"switch" + (s.backupBeforeWrite !== false ? " on" : "")} role="switch" aria-checked={s.backupBeforeWrite !== false} aria-label="写入前备份" onClick={() => set("backupBeforeWrite", s.backupBeforeWrite === false)} />
              </div>
            </div>
            <div className="set-row">
              <div>
                <div className="l">开机自启</div>
                <div className="d">登录系统时自动启动 z-switch 到托盘</div>
              </div>
              <button type="button" className={"switch" + (s.autoLaunch ? " on" : "")} role="switch" aria-checked={!!s.autoLaunch} aria-label="开机自启" onClick={toggleAutoLaunch} />
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
                </div>
              ) : (
                <div className="original-unavailable">原始配置快照尚不可用</div>
              )}
              <div className="original-actions">
                <button className="btn" disabled={!originalStatus?.captured} onClick={() => onRestoreOriginal("claude")}>恢复 Claude</button>
                <button className="btn" disabled={!originalStatus?.captured} onClick={() => onRestoreOriginal("codex")}>恢复 Codex</button>
              </div>
            </div>
          </div>

          <div className="set-group">
            <h4>本地路由 · 热切换（实验）</h4>
            <div className="set-row">
              <div>
                <div className="l">开启本地路由代理</div>
                <div className="d">
                  开启后把客户端指向 127.0.0.1:{proxyPort}，切换供应商<b>无需重启</b> Claude Code / Codex。
                  请求仅在本机转发至你选择的供应商，不发送给 z-switch / 真测 Ztest；真实密钥仅在本机内存中注入。
                </div>
              </div>
              <button
                type="button"
                className={"switch" + (proxyOn ? " on" : "") + (proxyBusy ? " busy" : "")}
                role="switch"
                aria-checked={proxyOn}
                aria-label="开启本地路由代理"
                onClick={toggleProxy}
                style={proxyBusy ? { opacity: 0.5, pointerEvents: "none" } : undefined}
              />
            </div>
            {proxyOn && (
              <div className="set-row" style={{ opacity: 0.8 }}>
                <div>
                  <div className="l status-running"><span className="status-dot" />运行中</div>
                  <div className="d">
                    正在监听 127.0.0.1:{proxyPort}/claude 与 /codex。关闭时会自动把地址改回真实供应商。
                  </div>
                </div>
              </div>
            )}
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
                    <div className="d">只记录失败状态、连接错误与流超时；自动脱敏密钥并轮转文件，不记录请求正文。</div>
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
                  <span>{proxyOn ? "参数修改后，重新开启本地路由生效。" : "参数将在下次开启本地路由时生效。"}</span>
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

          <div className="set-group">
            <h4>验真 · 即将上线（V2）</h4>
            <div className="set-row" style={{ opacity: 0.75 }}>
              <div>
                <div className="l">连接 真测 Ztest 账号</div>
                <div className="d">在供应商卡片显示模型验真状态（真 / 降级 / 假）</div>
              </div>
              <button className="btn" disabled style={{ opacity: 0.55, cursor: "default" }}>敬请期待</button>
            </div>
          </div>

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
        </div>
      </div>
    </div>
  );
}
