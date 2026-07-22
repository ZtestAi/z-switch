import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

const win = getCurrentWindow();

// 窗口控件（最小化 / 最大化-还原 / 关闭→隐藏到托盘）。
// 关闭请求拦截为 hide，退出交给托盘菜单；随窗口尺寸更新最大化状态。
export default function WindowControls() {
  const [maxed, setMaxed] = useState(false);

  useEffect(() => {
    win.isMaximized().then(setMaxed).catch(() => {});
    const unResize = win.onResized(() => {
      win.isMaximized().then(setMaxed).catch(() => {});
    });
    const unClose = win.onCloseRequested((event) => {
      event.preventDefault();
      void win.hide();
    });
    return () => {
      unResize.then((f) => f()).catch(() => {});
      unClose.then((f) => f()).catch(() => {});
    };
  }, []);

  return (
    <div className="win-controls">
      <button className="wc" title="最小化" onClick={() => win.minimize()}>
        <svg width="11" height="11" viewBox="0 0 11 11"><rect x="1.5" y="5" width="8" height="1.2" fill="currentColor" /></svg>
      </button>
      <button className="wc" title={maxed ? "还原" : "最大化"} onClick={() => win.toggleMaximize()}>
        {maxed ? (
          <svg width="11" height="11" viewBox="0 0 11 11" fill="none" stroke="currentColor" strokeWidth="1.1">
            <rect x="1.6" y="3" width="6" height="6" /><path d="M3.4 3V1.6h6v6H8" />
          </svg>
        ) : (
          <svg width="11" height="11" viewBox="0 0 11 11" fill="none" stroke="currentColor" strokeWidth="1.1">
            <rect x="1.8" y="1.8" width="7.4" height="7.4" />
          </svg>
        )}
      </button>
      <button className="wc close" title="最小化到托盘" aria-label="最小化到托盘" onClick={() => win.hide()}>
        <svg width="11" height="11" viewBox="0 0 11 11" fill="none" stroke="currentColor" strokeWidth="1.2">
          <path d="M2 2l7 7M9 2l-7 7" />
        </svg>
      </button>
    </div>
  );
}
