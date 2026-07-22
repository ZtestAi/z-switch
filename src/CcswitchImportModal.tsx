import { useEffect, useState } from "react";
import { scanCcswitch, importCcswitch, type CcswitchProvider } from "./api";
import type { Root } from "./types";
import { CloseIcon, DownloadIcon } from "./Icons";

function urlOf(p: CcswitchProvider): string {
  const env = (p.settingsConfig as any)?.env ?? {};
  const cfg = (p.settingsConfig as any) ?? {};
  return (
    (env.ANTHROPIC_BASE_URL as string) ??
    (cfg.config ? String(cfg.config).match(/base_url\s*=\s*"([^"]+)"/)?.[1] : "") ??
    ""
  ) || "";
}

interface Props {
  onClose: () => void;
  onImported: (root: Root, count: number) => void;
}

export default function CcswitchImportModal({ onClose, onImported }: Props) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [source, setSource] = useState<string>("none");
  const [items, setItems] = useState<CcswitchProvider[]>([]);
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    scanCcswitch()
      .then((scan) => {
        setSource(scan.source);
        setItems(scan.providers);
        setChecked(new Set(scan.providers.map((_, i) => i)));
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  const allChecked = items.length > 0 && checked.size === items.length;

  function toggle(i: number) {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  }

  function toggleAll() {
    setChecked(allChecked ? new Set() : new Set(items.map((_, i) => i)));
  }

  function confirm() {
    const selected = items.filter((_, i) => checked.has(i));
    if (selected.length === 0) return;
    setBusy(true);
    setError(null);
    importCcswitch(selected)
      .then((root) => onImported(root, selected.length))
      .catch((e) => {
        setError(String(e));
        setBusy(false);
      });
  }

  const sourceLabel =
    source === "sqlite" ? "cc-switch.db（SQLite）" : source === "json" ? "config.json" : "";

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>从 cc-switch 导入</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          {loading ? (
            <div className="hint">正在扫描 ~/.cc-switch …</div>
          ) : error && items.length === 0 ? (
            <div className="hint" style={{ color: "var(--danger)" }}>{error}</div>
          ) : items.length === 0 ? (
            <div className="hint">未在 ~/.cc-switch 找到可导入的 Claude / Codex 供应商。</div>
          ) : (
            <>
              <div className="hint" style={{ marginBottom: 8 }}>
                来源：{sourceLabel} · 共 {items.length} 个（仅第三方供应商，官方账号与其它客户端已跳过）。导入为新增，不会改变当前生效项。
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8, cursor: "pointer" }}>
                <input type="checkbox" checked={allChecked} onChange={toggleAll} />
                全选 / 全不选
              </label>
              <div style={{ display: "flex", flexDirection: "column", gap: 6, maxHeight: 360, overflow: "auto" }}>
                {items.map((p, i) => (
                  <label
                    key={i}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 10,
                      padding: "8px 10px",
                      border: "1px solid var(--border)",
                      borderRadius: 8,
                      cursor: "pointer",
                    }}
                  >
                    <input type="checkbox" checked={checked.has(i)} onChange={() => toggle(i)} />
                    <span className="badge">{p.app === "claude" ? "Claude" : "Codex"}</span>
                    <span style={{ fontWeight: 600 }}>{p.name}</span>
                    <span className="mono" style={{ marginLeft: "auto", opacity: 0.65, fontSize: 12 }}>
                      {urlOf(p) || "—"}
                    </span>
                  </label>
                ))}
              </div>
              {error && <div className="hint" style={{ color: "var(--danger)", marginTop: 8 }}>{error}</div>}
            </>
          )}
        </div>
        <div className="modal-foot">
          <span />
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn" onClick={onClose}>取消</button>
            <button
              className="btn accent"
              disabled={busy || loading || checked.size === 0}
              onClick={confirm}
            >
              <DownloadIcon />
              {busy ? "导入中…" : `导入所选（${checked.size}）`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
