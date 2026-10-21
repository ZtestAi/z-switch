import { useEffect, useState } from "react";
import type { AppType, Root } from "./types";
import {
  exportJson,
  importConfig,
  previewImport,
  type EndpointSummary,
  type ImportConflict,
  type ImportOutcome,
} from "./api";
import { CheckIcon, CloseIcon, CopyIcon, DownloadIcon } from "./Icons";

interface Props {
  mode: "import" | "export";
  onClose: () => void;
  onImported: (outcome: ImportOutcome) => void;
}

type Choice = "local" | "imported";

const APP_LABEL: Record<AppType, string> = { claude: "Claude Code", codex: "Codex", grok: "Grok" };

function EndpointRows({ title, name, summary }: { title: string; name: string; summary: EndpointSummary }) {
  return (
    <>
      <span className="ip-label">{title}</span>
      <span className="ip-value">
        {name}
        {summary.empty && <span style={{ color: "var(--danger)" }}> · 缺少地址或密钥，可能已损坏</span>}
      </span>
      <span className="ip-label" />
      <span className="ip-value mono">
        {summary.url || "（无地址）"} · {summary.key || "（无密钥）"}
      </span>
    </>
  );
}

export default function ImportExportModal({ mode, onClose, onImported }: Props) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  // 导入冲突确认：本机正在使用的卡片与导入文件里同 id 的卡片内容不同。
  const [pending, setPending] = useState<{ root: Root; conflicts: ImportConflict[] } | null>(null);
  const [choices, setChoices] = useState<Partial<Record<AppType, Choice>>>({});
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (mode === "export") {
      exportJson().then(setText).catch((e) => setError(String(e)));
    }
  }, [mode]);

  async function finish(root: Root, useImported: { app: AppType; id: string }[]) {
    const outcome = await importConfig(root, useImported);
    onImported(outcome);
    onClose();
  }

  async function doImport() {
    setBusy(true);
    try {
      const root = JSON.parse(text) as Root;
      if (!root.apps) throw new Error("缺少 apps 字段，不是有效的 z-switch 配置");
      const conflicts = await previewImport(root);
      if (conflicts.length === 0) {
        await finish(root, []);
        return;
      }
      // 本机版本已损坏而导入版本完整时，默认改用导入版本；其余默认保留本机。
      setChoices(
        Object.fromEntries(
          conflicts.map((c) => [c.app, c.local.empty && !c.imported.empty ? "imported" : "local"]),
        ),
      );
      setPending({ root, conflicts });
      setError(null);
    } catch (e) {
      setError("导入失败：" + String(e));
    } finally {
      setBusy(false);
    }
  }

  async function confirmImport() {
    if (!pending) return;
    setBusy(true);
    try {
      const useImported = pending.conflicts
        .filter((c) => choices[c.app] === "imported")
        .map((c) => ({ app: c.app, id: c.id }));
      await finish(pending.root, useImported);
    } catch (e) {
      setError("导入失败：" + String(e));
    } finally {
      setBusy(false);
    }
  }

  function copy() {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{mode === "export" ? "导出配置" : pending ? "确认正在使用的供应商" : "导入配置"}</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          {pending ? (
            <>
              <p className="confirm-msg">
                以下供应商本机正在使用，导入文件里也有同一张卡片但地址或密钥不同。请选择保留哪个版本：
              </p>
              {pending.conflicts.map((c) => (
                <div key={c.app} className="field" style={{ marginTop: 14 }}>
                  <label>{APP_LABEL[c.app]}</label>
                  <div className="import-preview" style={{ marginTop: 4 }}>
                    <EndpointRows title="本机版本" name={c.localName} summary={c.local} />
                    <EndpointRows title="导入版本" name={c.importedName} summary={c.imported} />
                  </div>
                  <div className="seg-mini" style={{ marginTop: 10 }}>
                    {(["local", "imported"] as const).map((value) => (
                      <button
                        key={value}
                        className={choices[c.app] === value ? "on" : ""}
                        onClick={() => setChoices((prev) => ({ ...prev, [c.app]: value }))}
                      >
                        {value === "local" ? "保留本机版本" : "使用导入版本"}
                      </button>
                    ))}
                  </div>
                </div>
              ))}
              <p className="import-note">
                「保留本机版本」：导入文件里的那张改用新 id 一并保留，不改动客户端配置文件。<br />
                「使用导入版本」：会立即把它写入该客户端的配置文件（开启「写入前备份」时先备份）；本机版本不是空卡片时改用新 id 一并保留。
              </p>
            </>
          ) : (
            <div className="field">
              <label>
                {mode === "export"
                  ? "供应商与设置（不含本机官方账号登录凭据）"
                  : "粘贴 z-switch 配置 JSON（覆盖供应商列表与设置；本机正在使用的供应商默认保持不变，有差异时会让你选择）"}
              </label>
              <textarea
                className="mono"
                value={text}
                readOnly={mode === "export"}
                placeholder={mode === "import" ? '{ "version": 2, "apps": { … } }' : ""}
                onChange={(e) => setText(e.target.value)}
                style={{ minHeight: 260 }}
              />
            </div>
          )}
          {error && <div className="hint" style={{ color: "var(--danger)" }}>{error}</div>}
        </div>
        <div className="modal-foot">
          <span />
          <div style={{ display: "flex", gap: 8 }}>
            {pending ? (
              <>
                <button className="btn" disabled={busy} onClick={() => { setPending(null); setError(null); }}>返回</button>
                <button className="btn accent" disabled={busy} onClick={confirmImport}><DownloadIcon />确认导入</button>
              </>
            ) : (
              <>
                <button className="btn" onClick={onClose}>关闭</button>
                {mode === "export" ? (
                  <button className="btn accent" onClick={copy}>{copied ? <><CheckIcon />已复制</> : <><CopyIcon />复制</>}</button>
                ) : (
                  <button className="btn accent" disabled={busy} onClick={doImport}><DownloadIcon />导入并覆盖</button>
                )}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
