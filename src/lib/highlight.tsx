/** Parse 【match】 markers from FTS snippet() into highlighted spans */
export function FtsSnippet({ text }: { text: string }) {
  const parts = text.split(/(【[^】]*】)/);
  return (
    <span>
      {parts.map((part, i) =>
        part.startsWith("【") && part.endsWith("】") ? (
          <mark key={i} className="rounded bg-orange-100 px-0.5 not-italic font-medium text-orange-800">
            {part.slice(1, -1)}
          </mark>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </span>
  );
}

/** Highlight query words in plain text (for semantic search) */
export function QuerySnippet({ text, query }: { text: string; query: string }) {
  const words = query.trim().split(/\s+/).filter((w) => w.length >= 2);
  if (!words.length) return <span>{text}</span>;

  const escaped = words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const parts = text.split(new RegExp(`(${escaped.join("|")})`, "i"));
  return (
    <span>
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <mark key={i} className="rounded bg-violet-100 px-0.5 not-italic font-medium text-violet-800">
            {part}
          </mark>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </span>
  );
}
