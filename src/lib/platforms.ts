// Canonical platform display names. `platformLabel` is the single place that
// decides how a platform renders, so the Guideline 5 China rule (suppress the
// ChatGPT brand name) lives in exactly one spot.
const RAW_LABELS: Record<string, string> = {
  claude: "Claude",
  chatgpt: "ChatGPT",
  deepseek: "DeepSeek",
  "claude-code": "Claude Code",
  codex: "Codex",
  agent: "Claude Code + Codex",
};

export function platformLabel(platform: string, isChina: boolean): string {
  if (isChina && platform === "chatgpt") return "AI 对话";
  return RAW_LABELS[platform] ?? platform;
}
