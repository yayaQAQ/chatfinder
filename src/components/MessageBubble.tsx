import { memo, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import ReactMarkdown from "react-markdown";
import type { PluggableList } from "unified";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Check, Copy, User, Bot, X, ZoomIn, Terminal } from "lucide-react";
import type { MessageRow } from "../lib/api";
import { markdownUrlTransform } from "../lib/markdown";
import { rehypeHighlightQuery } from "../lib/highlightPlugin";
import { useI18n, formatDateTime } from "../lib/i18n";
import { modelLabel } from "../lib/models";

function ImageLightbox({ src, onClose }: { src: string; onClose: () => void }) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return createPortal(
    <div
      className="fixed inset-0 z-[200] flex cursor-zoom-out items-center justify-center bg-black/85 backdrop-blur-sm animate-fade-in"
      onClick={onClose}
    >
      <button
        onClick={onClose}
        className="absolute right-6 top-6 flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-white transition-colors hover:bg-white/20"
      >
        <X size={19} />
      </button>
      <img
        src={src}
        alt=""
        className="max-h-[90vh] max-w-[90vw] rounded-lg object-contain shadow-2xl"
      />
    </div>,
    document.body,
  );
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function CodeBlock({ children, ...props }: any) {
  const { t } = useI18n();
  const preRef = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);

  const copy = () => {
    const text = preRef.current?.innerText ?? "";
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    });
  };

  // Extract language label from className (e.g. "language-python" → "python")
  const codeClass: string = children?.props?.className ?? "";
  const lang = codeClass.replace("language-", "") || null;

  return (
    <div className="group relative my-3">
      {/* Top bar: language label + always-visible copy button */}
      <div className="flex items-center justify-between rounded-t-xl bg-[#181825] px-3 py-1.5">
        <span className="text-[11px] font-mono text-stone-500">{lang ?? "code"}</span>
        <button
          onClick={copy}
          className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] text-stone-400 transition-colors hover:bg-white/10 hover:text-stone-200"
        >
          {copied ? <Check size={11} /> : <Copy size={11} />}
          {copied ? t("messageBubble.copied") : t("messageBubble.copy")}
        </button>
      </div>
      <pre
        ref={preRef}
        {...props}
        className="!mt-0 !rounded-t-none !rounded-b-xl !bg-[#1e1e2e] !text-[0.8rem] !leading-relaxed overflow-x-auto p-4"
      >
        {children}
      </pre>
    </div>
  );
}

function getMarkdownComponents(onImageClick: (src: string) => void) {
  return {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  pre({ children, ...props }: any) {
    return <CodeBlock {...props}>{children}</CodeBlock>;
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  code({ inline, className, children, ...props }: any) {
    if (inline) {
      return (
        <code
          className="rounded-md bg-stone-100 px-1.5 py-0.5 font-mono text-[0.82em] text-rose-600"
          {...props}
        >
          {children}
        </code>
      );
    }
    return (
      <code className={className} {...props}>
        {children}
      </code>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  table({ children }: any) {
    return (
      <div className="my-3 overflow-x-auto rounded-xl border border-stone-200">
        <table className="w-full text-sm">{children}</table>
      </div>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  th({ children }: any) {
    return <th className="bg-stone-50 px-3 py-2 text-left font-semibold text-stone-700">{children}</th>;
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  td({ children }: any) {
    return <td className="border-t border-stone-100 px-3 py-2 text-stone-700">{children}</td>;
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  blockquote({ children }: any) {
    return (
      <blockquote className="my-2 border-l-4 border-orange-300 bg-orange-50 py-1 pl-4 pr-2 italic text-stone-600">
        {children}
      </blockquote>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  a({ href, children }: any) {
    return (
      <a href={href} className="text-orange-600 underline hover:text-orange-700" target="_blank" rel="noreferrer">
        {children}
      </a>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  mark({ children }: any) {
    return (
      <mark className="rounded bg-amber-200 px-0.5 not-italic text-stone-900">{children}</mark>
    );
  },
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  img({ src, alt }: any) {
    return (
      <span className="group/img relative my-3 inline-block">
        <img
          src={src}
          alt={alt ?? ""}
          onClick={() => onImageClick(src)}
          className="max-w-full cursor-zoom-in rounded-xl border border-stone-200 shadow-sm transition-transform duration-150 group-hover/img:brightness-95"
          style={{ maxHeight: "520px", objectFit: "contain" }}
          onError={(e) => {
            (e.currentTarget as HTMLImageElement).style.display = "none";
          }}
        />
        <span className="pointer-events-none absolute inset-0 flex items-center justify-center opacity-0 transition-opacity group-hover/img:opacity-100">
          <span className="flex h-9 w-9 items-center justify-center rounded-full bg-black/50 text-white">
            <ZoomIn size={16} />
          </span>
        </span>
      </span>
    );
  },
  };
}

function MessageBubbleImpl({
  message,
  platform,
  highlighted = false,
  highlightQuery,
}: {
  message: MessageRow;
  platform: string;
  highlighted?: boolean;
  /** In-conversation search term — occurrences get wrapped in <mark>. */
  highlightQuery?: string;
}) {
  const { t, lang } = useI18n();
  const isHuman = message.sender === "human";
  // Localized category label for non-text agent content (tool calls, results,
  // thinking). The raw content itself is stored language-neutral.
  const kindHeader = (() => {
    switch (message.kind) {
      case "tool_use": return { icon: "🔧", label: t("conversationDetail.kindToolUse") };
      case "tool_result": return { icon: "📄", label: t("conversationDetail.kindToolResult") };
      case "thinking": return { icon: "💭", label: t("conversationDetail.kindThinking") };
      default: return null;
    }
  })();
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null);
  // Stable reference so ReactMarkdown doesn't treat these as new component
  // types on every re-render — that would remount the DOM and clear any
  // active text selection (e.g. right after setSelection() in the parent).
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const markdownComponents = useMemo(() => getMarkdownComponents(setLightboxSrc), []);
  const rehypePlugins = useMemo(
    // unified calls each entry as the attacher and uses its return value as
    // the transformer — [plugin, options] makes it call rehypeHighlightQuery
    // itself with `highlightQuery`, instead of us pre-calling it and handing
    // unified the already-built transformer (which it would then invoke with
    // no arguments, so `tree` inside it would be undefined).
    (): PluggableList =>
      highlightQuery ? [rehypeHighlight, [rehypeHighlightQuery, highlightQuery]] : [rehypeHighlight],
    [highlightQuery],
  );

  // CLI-injected local content (slash-command output, hooks, caveats) rides
  // on a "user" turn in the source log but wasn't typed by a person — shown
  // as a compact system note instead of a real chat bubble so it reads as
  // distinct from actual human input at a glance.
  if (message.sender === "meta") {
    return (
      <div
        data-message-id={message.id}
        className={`flex justify-center transition-colors duration-700 ${highlighted ? "rounded-2xl bg-amber-50 py-1" : ""}`}
      >
        <div className="flex max-w-[85%] items-start gap-2 rounded-xl bg-stone-100/80 px-3 py-2 text-xs text-stone-500 ring-1 ring-stone-200">
          <Terminal size={12} className="mt-0.5 shrink-0 text-stone-400" />
          <div className="message-content min-w-0 flex-1 whitespace-pre-wrap break-words font-mono">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              rehypePlugins={rehypePlugins}
              components={markdownComponents}
              urlTransform={markdownUrlTransform}
            >
              {message.text || t("messageBubble.emptyMessage")}
            </ReactMarkdown>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      data-message-id={message.id}
      className={`flex gap-3 rounded-2xl transition-colors duration-700 ${highlighted ? "bg-amber-50 -mx-3 px-3 py-1" : ""} ${isHuman ? "flex-row-reverse" : ""}`}
    >
      <div
        className={`mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full shadow-sm ${
          isHuman
            ? "bg-gradient-to-br from-stone-500 to-stone-700 text-white"
            : platform === "claude" || platform === "claude-code"
            ? "bg-gradient-to-br from-orange-400 to-rose-500 text-white"
            : platform === "deepseek"
            ? "bg-gradient-to-br from-blue-500 to-indigo-600 text-white"
            // GPT-family (chatgpt, codex) — OpenAI's teal/cyan brand color
            : "bg-gradient-to-br from-cyan-400 to-teal-600 text-white"
        }`}
      >
        {isHuman ? <User size={15} /> : <Bot size={15} />}
      </div>

      <div
        className={`group relative min-w-0 max-w-[85%] rounded-2xl px-4 py-3 text-sm leading-relaxed shadow-sm ${
          isHuman
            ? "rounded-tr-sm bg-stone-100 text-stone-800"
            : "w-full rounded-tl-sm bg-white ring-1 ring-stone-200 text-stone-800"
        }`}
      >
        <div className="message-content">
          {kindHeader && (
            <div className="mb-1.5 flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-stone-400">
              <span>{kindHeader.icon}</span>
              <span>{kindHeader.label}</span>
            </div>
          )}
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={rehypePlugins}
            components={markdownComponents}
            urlTransform={markdownUrlTransform}
          >
            {message.text || t("messageBubble.emptyMessage")}
          </ReactMarkdown>
        </div>
        {(message.created_at || message.model) && (
          <div className={`mt-1.5 flex items-center gap-1.5 text-[11px] text-stone-400 ${isHuman ? "justify-end" : ""}`}>
            {message.created_at && <span>{formatDateTime(message.created_at, lang)}</span>}
            {/* Which model wrote this turn — shown per message because a
                session can switch models partway through. */}
            {message.model && (
              <span
                title={message.model}
                className="rounded bg-stone-100 px-1.5 py-0.5 font-medium text-stone-500"
              >
                {modelLabel(message.model)}
              </span>
            )}
          </div>
        )}
      </div>

      {lightboxSrc && <ImageLightbox src={lightboxSrc} onClose={() => setLightboxSrc(null)} />}
    </div>
  );
}

// Most messages in a long conversation don't match the current search term,
// so memoize on props to skip re-rendering (and re-parsing markdown for)
// the ones whose highlightQuery/highlighted state didn't actually change.
export const MessageBubble = memo(MessageBubbleImpl);
