// Minimal duck-typed hast shapes — avoids depending directly on the `hast`
// package (react-markdown/rehype pull it in transitively, but it isn't a
// direct dependency here).
interface HastText {
  type: "text";
  value: string;
}
interface HastElement {
  type: "element";
  tagName: string;
  properties: Record<string, unknown>;
  children: HastNode[];
}
type HastNode = HastText | HastElement | { type: string; children?: HastNode[] };
interface HastRoot {
  type: "root";
  children: HastNode[];
}

// Code/pre subtrees are already tokenized by rehype-highlight into nested
// <span class="hljs-*"> elements — splicing marks into their text nodes
// would break that structure, so those subtrees are left untouched.
const SKIP_TAGS = new Set(["code", "pre"]);

function hasChildren(node: HastNode): node is HastElement | HastRoot {
  return Array.isArray((node as { children?: HastNode[] }).children);
}

function splitTextNode(node: HastText, needle: string): HastNode[] | null {
  const value = node.value;
  const lower = value.toLowerCase();
  if (!lower.includes(needle)) return null;

  const parts: HastNode[] = [];
  let cursor = 0;
  let idx = lower.indexOf(needle, cursor);
  while (idx !== -1) {
    if (idx > cursor) parts.push({ type: "text", value: value.slice(cursor, idx) });
    parts.push({
      type: "element",
      tagName: "mark",
      properties: {},
      children: [{ type: "text", value: value.slice(idx, idx + needle.length) }],
    });
    cursor = idx + needle.length;
    idx = lower.indexOf(needle, cursor);
  }
  if (cursor < value.length) parts.push({ type: "text", value: value.slice(cursor) });
  return parts;
}

function walk(nodes: HastNode[], needle: string) {
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (node.type === "element") {
      const el = node as HastElement;
      if (SKIP_TAGS.has(el.tagName)) continue;
      walk(el.children, needle);
    } else if (node.type === "text") {
      const replaced = splitTextNode(node as HastText, needle);
      if (replaced) {
        nodes.splice(i, 1, ...replaced);
        i += replaced.length - 1;
      }
    } else if (hasChildren(node)) {
      walk(node.children, needle);
    }
  }
}

/** Rehype plugin: wraps every case-insensitive occurrence of `query` in the
 * rendered message in a <mark>, so an in-conversation search can show exactly
 * where each match is instead of just jumping to the containing message. */
export function rehypeHighlightQuery(query: string) {
  const needle = query.trim().toLowerCase();
  return (tree: HastRoot) => {
    if (!needle) return;
    walk(tree.children, needle);
  };
}
