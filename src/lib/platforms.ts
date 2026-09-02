// Canonical platform display names. `platformLabel` is the single place that
// decides how a platform renders. This App Store build suppresses the ChatGPT
// brand name (Guideline 5), so it is relabeled to a neutral "AI 对话" here.
const RAW_LABELS: Record<string, string> = {
  claude: "Claude",
  chatgpt: "AI 对话",
  deepseek: "DeepSeek",
  "claude-code": "Claude Code",
  codex: "Codex",
  agent: "Claude Code + Codex",
};

export function platformLabel(platform: string): string {
  return RAW_LABELS[platform] ?? platform;
}
