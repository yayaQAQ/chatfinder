/**
 * Host OS as seen from the webview, for presentation only — modifier glyphs,
 * file-dialog hints, anything that just has to *look* native. The webview's
 * user agent reports it the same way on all three Tauri backends (WKWebView,
 * WebView2, WebKitGTK) and is available synchronously, so nothing flashes the
 * wrong key on first paint.
 *
 * Anything functional (which terminal to launch, which shell dialect a command
 * needs) must ask the backend via `api.terminalEnv()` instead — that is the
 * side that actually knows.
 */
export type HostOs = "macos" | "windows" | "linux";

export const hostOs: HostOs = /Mac|iPhone|iPad/.test(navigator.userAgent)
  ? "macos"
  : /Win/i.test(navigator.userAgent)
    ? "windows"
    : "linux";

/**
 * The shortcut modifier as written on this platform. The handlers themselves
 * accept either Cmd or Ctrl (see App.tsx) — this is only what the label says.
 */
export const modKey = hostOs === "macos" ? "⌘" : "Ctrl+";
