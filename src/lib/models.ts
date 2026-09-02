// Display names for the raw model ids recorded in session logs
// (`claude-opus-5`, `claude-sonnet-4-5-20250929`, `gpt-5.6-sol`, …).
// The raw id stays the stored/filtered value — this only shapes what's shown,
// and anything unrecognized falls through unchanged rather than being mangled.

const CLAUDE_FAMILIES = ["opus", "sonnet", "haiku"];

/** Drop a trailing release date, e.g. "claude-sonnet-4-5-20250929". */
function stripDate(id: string): string {
  return id.replace(/-\d{8}$/, "");
}

export function modelLabel(id: string): string {
  const raw = stripDate(id.trim());
  if (!raw) return id;

  if (raw.startsWith("claude-")) {
    const parts = raw.slice("claude-".length).split("-");
    const family = parts.find((p) => CLAUDE_FAMILIES.includes(p.toLowerCase()));
    if (family) {
      // Version digits can sit on either side of the family name
      // ("opus-5", "3-5-sonnet") and read as one dotted number.
      const version = parts.filter((p) => /^[\d.]+$/.test(p)).join(".");
      const rest = parts
        .filter((p) => p !== family && !/^[\d.]+$/.test(p) && p !== "latest")
        .join(" ");
      const name = family.charAt(0).toUpperCase() + family.slice(1);
      return [name, version, rest].filter(Boolean).join(" ");
    }
  }

  if (/^gpt/i.test(raw)) {
    return `GPT${raw.slice(3)}`;
  }
  if (/^o\d/.test(raw)) {
    return raw;
  }

  return raw;
}

/** Family used to color a model badge — matches the vendor, not the tier. */
export function modelFamily(id: string): "claude" | "gpt" | "other" {
  if (id.startsWith("claude")) return "claude";
  if (/^(gpt|o\d|codex)/i.test(id)) return "gpt";
  return "other";
}

export const modelBadgeClass: Record<string, string> = {
  claude: "bg-orange-50 text-orange-700 border-orange-200",
  gpt: "bg-teal-50 text-teal-700 border-teal-200",
  other: "bg-stone-100 text-stone-600 border-stone-200",
};
