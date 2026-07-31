import { useEffect, useRef, useState } from "react";
import { useParams, useSearchParams, useNavigate } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, ExternalLink, Star, MessageSquare, Copy, Check } from "lucide-react";
import { api, type ConversationSummary, type MessageRow } from "../lib/api";
import { FavoriteModal } from "../components/FavoriteModal";
import { MessageBubble } from "../components/MessageBubble";
import { useI18n, formatLongDate } from "../lib/i18n";

export function ConversationDetail() {
  const { t, lang } = useI18n();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const targetMsgId = searchParams.get("msg");
  const [conv, setConv] = useState<ConversationSummary | null>(null);
  const [messages, setMessages] = useState<MessageRow[]>([]);
  const [loading, setLoading]   = useState(true);
  const [idCopied, setIdCopied] = useState(false);
  const [highlightedId, setHighlightedId] = useState<string | null>(null);
  const [selection, setSelection] = useState<{ text: string; messageId: string | null } | null>(null);
  const [favoriteTarget, setFavoriteTarget] = useState<{ text: string; messageId: string | null } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!id) return;
    setLoading(true);
    api
      .getConversation(id)
      .then(([c, msgs]) => {
        setConv(c);
        setMessages(msgs);
      })
      .finally(() => setLoading(false));
  }, [id]);

  // Scroll to and highlight the target message after load
  useEffect(() => {
    if (loading || !targetMsgId) return;
    setHighlightedId(targetMsgId);
    // Small delay so the DOM has rendered
    const t = setTimeout(() => {
      const el = document.querySelector(`[data-message-id="${targetMsgId}"]`);
      el?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 80);
    // Fade out highlight after 2.5 s
    const clear = setTimeout(() => setHighlightedId(null), 2500);
    return () => { clearTimeout(t); clearTimeout(clear); };
  }, [loading, targetMsgId]);

  const handleMouseUp = () => {
    const sel = window.getSelection();
    const text = sel?.toString().trim();
    if (!text || !containerRef.current) return;
    if (!sel || sel.rangeCount === 0) return;
    const anchorNode = sel.anchorNode;
    if (!anchorNode || !containerRef.current.contains(anchorNode)) return;

    let node: Node | null = anchorNode;
    let messageId: string | null = null;
    while (node) {
      if (node instanceof HTMLElement && node.dataset.messageId) {
        messageId = node.dataset.messageId;
        break;
      }
      node = node.parentNode;
    }
    setSelection({ text, messageId });
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="flex flex-col items-center gap-3 text-stone-400">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-stone-300 border-t-orange-400" />
          <span className="text-sm">{t("conversationDetail.loading")}</span>
        </div>
      </div>
    );
  }

  if (!conv) {
    return <div className="flex h-full items-center justify-center text-sm text-stone-400">{t("conversationDetail.notFound")}</div>;
  }

  const platformLabel =
    conv.platform === "claude" ? "Claude"
    : conv.platform === "deepseek" ? "DeepSeek"
    : conv.platform === "claude-code" ? "Claude Code"
    : conv.platform === "codex" ? "Codex"
    : "ChatGPT";
  const platformColor =
    conv.platform === "claude"
      ? "from-orange-400 to-rose-500"
      : conv.platform === "deepseek"
      ? "from-blue-500 to-indigo-600"
      : conv.platform === "claude-code"
      ? "from-amber-400 to-orange-600"
      : conv.platform === "codex"
      ? "from-sky-400 to-indigo-500"
      : "from-emerald-400 to-teal-600";

  return (
    <div className="flex h-full flex-col bg-[#f7f5f2]">
      {/* Header */}
      <div className="flex items-center gap-3 border-b border-stone-200 bg-white px-5 py-3.5 shadow-sm">
        <button
          onClick={() => navigate(-1)}
          className="flex h-8 w-8 items-center justify-center rounded-xl text-stone-400 transition-colors hover:bg-stone-100 hover:text-stone-700"
        >
          <ArrowLeft size={17} />
        </button>
        <div
          className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br ${platformColor} text-white shadow-sm text-xs font-bold`}
        >
          {platformLabel[0]}
        </div>
        <div className="flex-1 min-w-0">
          <h1 className="truncate font-semibold text-stone-800">{conv.title || t("common.untitledConversation")}</h1>
          <div className="flex items-center gap-2 flex-wrap">
            <p className="text-xs text-stone-400">
              {platformLabel} · {t("common.messageCount", { n: conv.message_count })}
              {conv.updated_at ? ` · ${formatLongDate(conv.updated_at, lang)}` : ""}
            </p>
            <button
              onClick={() => {
                navigator.clipboard.writeText(conv.id);
                setIdCopied(true);
                setTimeout(() => setIdCopied(false), 1800);
              }}
              title={t("conversationDetail.copyId")}
              className="flex items-center gap-1 rounded-md bg-stone-100 px-1.5 py-0.5 font-mono text-[10px] text-stone-400 transition-colors hover:bg-stone-200 hover:text-stone-600"
            >
              {idCopied ? <Check size={9} /> : <Copy size={9} />}
              {conv.id.slice(0, 8)}…
            </button>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {conv.url && (
            <button
              onClick={() => openUrl(conv.url!)}
              className="flex items-center gap-1.5 rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-600 transition-colors hover:bg-stone-50 hover:text-stone-800"
            >
              <ExternalLink size={13} />
              {t("conversationDetail.continueConversation")}
            </button>
          )}
        </div>
      </div>

      {/* Messages */}
      <div ref={containerRef} onMouseUp={handleMouseUp} className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-3xl px-5 py-6">
          {messages.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-16 text-stone-400">
              <MessageSquare size={32} strokeWidth={1.5} />
              <p className="text-sm">{t("conversationDetail.noMessages")}</p>
            </div>
          )}
          <div className="flex flex-col gap-5">
            {messages.map((m) => (
              <MessageBubble
                key={m.id}
                message={m}
                platform={conv.platform}
                highlighted={m.id === highlightedId}
              />
            ))}
          </div>
        </div>
      </div>

      {/* Selection toolbar */}
      {selection && (
        <div className="pointer-events-none fixed inset-x-0 bottom-8 flex justify-center z-50">
          <div className="pointer-events-auto flex items-center gap-2 rounded-full bg-stone-900/95 px-4 py-2.5 text-sm text-white shadow-2xl animate-fade-in backdrop-blur-sm">
            <span className="max-w-[240px] truncate text-stone-300 text-xs">{t("conversationDetail.textSelected")}</span>
            <div className="h-4 w-px bg-stone-700" />
            <button
              onClick={() => setFavoriteTarget(selection)}
              className="flex items-center gap-1.5 rounded-full bg-amber-500 px-3 py-1 text-xs font-medium hover:bg-amber-400 transition-colors"
            >
              <Star size={12} />
              {t("conversationDetail.saveSnippet")}
            </button>
            <button onClick={() => setSelection(null)} className="text-stone-500 hover:text-stone-300 transition-colors">
              ✕
            </button>
          </div>
        </div>
      )}

      {favoriteTarget && id && (
        <FavoriteModal
          conversationId={id}
          messageId={favoriteTarget.messageId}
          selectedText={favoriteTarget.text}
          onClose={() => setFavoriteTarget(null)}
          onSaved={() => {
            setFavoriteTarget(null);
            setSelection(null);
          }}
        />
      )}
    </div>
  );
}
