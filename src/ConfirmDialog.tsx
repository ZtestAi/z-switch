import { CloseIcon } from "./Icons";

interface Props {
  title?: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  secondaryText?: string;
  danger?: boolean;
  onConfirm: () => void;
  onSecondary?: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog({
  title = "确认",
  message,
  confirmText = "确定",
  cancelText = "取消",
  secondaryText,
  danger,
  onConfirm,
  onSecondary,
  onCancel,
}: Props) {
  return (
    <div className="overlay" onMouseDown={onCancel}>
      <div className="modal confirm" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{title}</h3>
          <button className="x" onClick={onCancel} aria-label="关闭"><CloseIcon /></button>
        </div>
        <div className="modal-body">
          <p className="confirm-msg">{message}</p>
        </div>
        <div className="modal-foot">
          <div className="confirm-actions">
            <button className="btn" onClick={onCancel}>{cancelText}</button>
            {secondaryText && onSecondary && (
              <button className="btn" onClick={onSecondary}>{secondaryText}</button>
            )}
            <button className={"btn " + (danger ? "danger" : "accent")} onClick={onConfirm}>
              {confirmText}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
