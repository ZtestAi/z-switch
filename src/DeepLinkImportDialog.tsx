import { CloseIcon } from "./Icons";
import type { AppType, Provider } from "./types";

export interface PendingImport {
  app: AppType;
  name: string;
  provider: Provider;
  baseUrl: string;
  model: string;
  keyMasked: string;
  /** 存在同名供应商时的原始请求名（已自动加后缀改用 name），弹框提示用；无碰撞则 undefined。 */
  collisionOf?: string;
}

interface Props {
  pending: PendingImport;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function DeepLinkImportDialog({ pending, onConfirm, onCancel }: Props) {
  const appLabel = pending.app === "claude" ? "Claude Code" : "Codex";
  return (
    <div className="overlay" onMouseDown={onCancel}>
      <div className="modal confirm" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>确认导入供应商</h3>
          <button className="x" onClick={onCancel} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          <p className="confirm-msg">来自浏览器的链接请求向 z-switch 添加以下供应商，请确认无误后再导入：</p>
          <div className="import-preview">
            <span className="ip-label">应用</span>
            <span className="ip-value">{appLabel}</span>
            <span className="ip-label">名称</span>
            <span className="ip-value">{pending.name}</span>
            <span className="ip-label">接入点</span>
            <span className="ip-value mono">{pending.baseUrl}</span>
            {pending.model && (
              <>
                <span className="ip-label">模型</span>
                <span className="ip-value mono">{pending.model}</span>
              </>
            )}
            <span className="ip-label">密钥</span>
            <span className="ip-value mono">{pending.keyMasked}</span>
          </div>
          {pending.collisionOf && (
            <p className="import-note">已存在同名供应商「{pending.collisionOf}」，将<b>新增</b>为「{pending.name}」，不会覆盖现有配置。</p>
          )}
        </div>
        <div className="modal-foot">
          <div className="confirm-actions">
            <button className="btn" onClick={onCancel}>取消</button>
            <button className="btn accent" onClick={onConfirm}>确认导入</button>
          </div>
        </div>
      </div>
    </div>
  );
}
