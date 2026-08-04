import { defaultUrlTransform } from "react-markdown";

// react-markdown's default urlTransform strips "data:" URIs (its XSS
// safelist is http/https/irc/mailto/xmpp only), which silently drops every
// inline image we embed as a base64 data URI. Allow data:image/* through
// while keeping the default sanitization for every other URL/protocol.
export function markdownUrlTransform(url: string): string {
  if (url.startsWith("data:image/")) return url;
  return defaultUrlTransform(url);
}

// Rendered messages are markdown (bold, code, lists, links, ...) turned into
// HTML by ReactMarkdown. window.getSelection().toString() flattens all of
// that back to plain text, so anything the user highlights to save as a
// favorite loses its formatting. This walks the selected DOM range and
// re-serializes it to markdown so favorites keep the original styling.
export function selectionToMarkdown(range: Range): string {
  const container = document.createElement("div");
  container.appendChild(range.cloneContents());
  return nodeToMarkdown(container)
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function childMarkdown(el: HTMLElement): string {
  return Array.from(el.childNodes).map(nodeToMarkdown).join("");
}

function nodeToMarkdown(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent ?? "";
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return "";

  const el = node as HTMLElement;
  const tag = el.tagName.toLowerCase();

  switch (tag) {
    case "button":
      // Copy-button / language-label chrome injected around code blocks —
      // not part of the message content.
      return "";
    case "strong":
    case "b":
      return `**${childMarkdown(el)}**`;
    case "em":
    case "i":
      return `*${childMarkdown(el)}*`;
    case "del":
    case "s":
      return `~~${childMarkdown(el)}~~`;
    case "a": {
      const href = el.getAttribute("href");
      const text = childMarkdown(el);
      return href ? `[${text}](${href})` : text;
    }
    case "img": {
      const alt = el.getAttribute("alt") ?? "";
      const src = el.getAttribute("src") ?? "";
      return `![${alt}](${src})`;
    }
    case "code": {
      if (el.closest("pre")) return childMarkdown(el);
      return `\`${childMarkdown(el)}\``;
    }
    case "pre": {
      const codeEl = el.querySelector("code");
      const lang = codeEl?.className.match(/language-(\S+)/)?.[1] ?? "";
      const code = (codeEl ?? el).textContent ?? "";
      return `\n\`\`\`${lang}\n${code.replace(/\n$/, "")}\n\`\`\`\n`;
    }
    case "h1":
    case "h2":
    case "h3":
    case "h4":
    case "h5":
    case "h6": {
      const level = Number(tag[1]);
      return `\n${"#".repeat(level)} ${childMarkdown(el)}\n\n`;
    }
    case "blockquote": {
      const inner = childMarkdown(el).trim();
      return `\n${inner.split("\n").map((l) => `> ${l}`).join("\n")}\n\n`;
    }
    case "li": {
      const marker = el.parentElement?.tagName.toLowerCase() === "ol" ? "1." : "-";
      return `${marker} ${childMarkdown(el).trim()}\n`;
    }
    case "ul":
    case "ol":
      return `\n${childMarkdown(el)}\n`;
    case "br":
      return "\n";
    case "p":
      return `${childMarkdown(el)}\n\n`;
    case "div": {
      // CodeBlock renders <div><div>header (lang label + copy button)</div><pre>...</pre></div>.
      // The header is UI chrome, not message content — if this div directly
      // wraps a <pre>, capture only the code block and skip the header.
      const directPre = Array.from(el.children).find((c) => c.tagName.toLowerCase() === "pre");
      if (directPre) return nodeToMarkdown(directPre);
      return `${childMarkdown(el)}\n\n`;
    }
    case "tr":
      return `${childMarkdown(el)}\n`;
    case "th":
    case "td":
      return `${childMarkdown(el).trim()} | `;
    default:
      return childMarkdown(el);
  }
}
