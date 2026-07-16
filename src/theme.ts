// 主题应用：把 settings.theme（light/dark/system）落到 <html> 的 data-theme。
// system 跟随 OS，并监听变化。App.css 用 :root[data-theme="dark"] 覆盖变量。

export type Theme = "light" | "dark" | "system";

let mediaQuery: MediaQueryList | null = null;
let mqHandler: ((e: MediaQueryListEvent) => void) | null = null;

function resolve(theme: Theme): "light" | "dark" {
  if (theme === "system") {
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return theme;
}

/** 应用主题到 <html data-theme>。system 会注册监听，其它模式清理监听。 */
export function applyTheme(theme: Theme | undefined) {
  const t: Theme = theme ?? "light";
  document.documentElement.setAttribute("data-theme", resolve(t));

  // 清理旧监听
  if (mediaQuery && mqHandler) {
    mediaQuery.removeEventListener("change", mqHandler);
    mediaQuery = null;
    mqHandler = null;
  }
  // system 模式：跟随 OS 实时变化
  if (t === "system" && window.matchMedia) {
    mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mqHandler = (e) => {
      document.documentElement.setAttribute("data-theme", e.matches ? "dark" : "light");
    };
    mediaQuery.addEventListener("change", mqHandler);
  }
}
