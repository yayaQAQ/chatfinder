// Canonical platform display names. `platformLabel` is the single place that
// decides how a platform renders.
import { APP_STORE_BUILD } from "./brand";
import type { Lang } from "./i18n";

const LABELS: Record<string, string> = {
  claude: "Claude",
  deepseek: "DeepSeek",
  "claude-code": "Claude Code",
  codex: "Codex",
  agent: "Claude Code + Codex",
};

// Kept out of LABELS and behind the build flag so the App Store bundle doesn't
// carry the brand name at all — the unused branch is folded away at build time
// (see brand.ts).
const CHATGPT_LABEL: Record<Lang, string> = APP_STORE_BUILD
  ? { zh: "AI 对话", en: "AI chat" }
  : { zh: "ChatGPT", en: "ChatGPT" };

export function platformLabel(platform: string, lang: Lang = "zh"): string {
  if (platform === "chatgpt") return CHATGPT_LABEL[lang] ?? CHATGPT_LABEL.zh;
  return LABELS[platform] ?? platform;
}
