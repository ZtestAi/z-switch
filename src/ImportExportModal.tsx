import { useEffect, useState } from "react";
import type { Root } from "./types";
import { exportJson, importConfig } from "./api";
import { CheckIcon, CloseIcon, CopyIcon, DownloadIcon } from "./Icons";

interface Props {
  mode: "import" | "export";
  onClose: () => void;
  onImported: (root: Root) => void;
}

export default function ImportExportModal({ mode, onClose, onImported }: Props) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (mode === "export") {
      exportJson().then(setText).catch((e) => setError(String(e)));
    }
  }, [mode]);

  async function doImport() {
    try {
      const root = JSON.parse(text) as Root;
      if (!root.apps) throw new Error("缺少 apps 字段，不是有效的 z-switch 配置");
      const saved = await importConfig(root);
      onImported(saved);
      onClose();
    } catch (e) {
      setError("导入失败：" + String(e));
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
          <h3>{mode === "export" ? "导出配置" : "导入配置"}</h3>
          <button className="x" onClick={onClose} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          <div className="field">
            <label>
              {mode === "export"
                ? "供应商与设置（不含本机官方账号登录凭据）"
                : "粘贴 z-switch 配置 JSON"}
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
          {error && <div className="hint" style={{ color: "var(--danger)" }}>{error}</div>}
        </div>
        <div className="modal-foot">
          <span />
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn" onClick={onClose}>关闭</button>
            {mode === "export" ? (
              <button className="btn accent" onClick={copy}>{copied ? <><CheckIcon />已复制</> : <><CopyIcon />复制</>}</button>
            ) : (
              <button className="btn accent" onClick={doImport}><DownloadIcon />导入并覆盖</button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
