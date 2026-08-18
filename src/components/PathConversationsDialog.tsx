import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { X, FolderOpen, Loader2, MessageSquare, Search, FileText, Heading } from "lucide-react";
import { api, type ConversationSummary, type SearchHit } from "../lib/api";
import { FtsSnippet } from "../lib/highlight";
import { useI18n, formatRelativeDate } from "../lib/i18n";
import { useRegion } from "../lib/region";
import { platformLabel } from "../lib/platforms";

const platformColor: Record<string, string> = {
  "claude-code": "from-amber-400 to-orange-600",
  codex: "from-sky-400 to-indigo-500",
};

export function PathConversationsDialog({
  path,
  currentId,
  onClose,
}: {
  path: string;
  currentId?: string;
  onClose: () => void;
}) {
  const [loading, setLoading] = useState(true);
  const [items, setItems] = useState<ConversationSummary[]>([]);
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const navigate = useNavigate();
  const { t, lang } = useI18n();
  const { isChina } = useRegion();

  useEffect(() => {
    api.listConversationsByPath(path).then(setItems).finally(() => setLoading(false));
  }, [path]);

  // Debounced full-text search across every conversation under this path,
  // regardless of which tool produced it.
  useEffect(() => {
    const q = query.trim();
    if (!q) { setHits([]); setSearching(false); return; }
    setSearching(true);
    const timer = setTimeout(() => {
      api.searchConversationsByPath(path, q).then(setHits).finally(() => setSearching(false));
    }, 250);
    return () => clearTimeout(timer);
  }, [path, query]);

  const goTo = (id: string, messageId?: string) => {
    onClose();
    navigate(`/conversation/${id}${messageId ? `?msg=${encodeURIComponent(messageId)}` : ""}`);
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div
        className="flex max-h-[80vh] w-[520px] flex-col overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-start justify-between gap-3 border-b border-stone-100 px-6 py-4">
          <div className="flex items-start gap-2.5 min-w-0">
            <div className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-stone-100 text-stone-600">
              <FolderOpen size={16} />
            </div>
            <div className="min-w-0">
              <h2 className="font-semibold text-stone-800">{t("pathDialog.title")}</h2>
              <p className="truncate font-mono text-xs text-stone-400" title={path}>{path}</p>
            </div>
          </div>
          <button onClick={onClose} className="shrink-0 rounded-full p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600">
            <X size={17} />
          </button>
        </div>

        {/* Search — scoped to every conversation under this path, either tool */}
        <div className="border-b border-stone-100 px-4 py-2.5">
          <div className="relative">
            <Search size={13} className="absolute left-3 top-1/2 -translate-y-1/2 text-stone-400 pointer-events-none" />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("pathDialog.searchPlaceholder")}
              className="h-8 w-full rounded-lg border border-stone-200 bg-stone-50 pl-8 pr-3 text-sm outline-none transition-colors focus:border-amber-300 focus:bg-white focus:ring-2 focus:ring-amber-100"
            />
          </div>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-3 py-3">
          {query.trim() ? (
            <>
              {searching && (
                <div className="flex flex-col items-center gap-3 py-10 text-stone-400">
                  <Loader2 size={22} className="animate-spin" />
                  <p className="text-sm">{t("pathDialog.loading")}</p>
                </div>
              )}
              {!searching && hits.length === 0 && (
                <div className="flex flex-col items-center gap-3 py-10 text-stone-400">
                  <Search size={26} strokeWidth={1.5} />
                  <p className="text-sm">{t("pathDialog.noSearchResults")}</p>
                </div>
              )}
              {!searching &&
                hits.map((h) => {
                  const isMsg = h.kind === "message";
                  return (
                    <button
                      key={`${h.conversation_id}:${h.ref_id}`}
                      onClick={() => goTo(h.conversation_id, isMsg ? h.ref_id : undefined)}
                      className="flex w-full items-start gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-stone-50"
                    >
                      <div
                        className={`mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br text-white text-[11px] font-bold shadow-sm ${
                          platformColor[h.platform] ?? "from-stone-400 to-stone-600"
                        }`}
                      >
                        {platformLabel(h.platform, isChina)[0]}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          {isMsg ? <FileText size={11} className="shrink-0 text-stone-400" /> : <Heading size={11} className="shrink-0 text-stone-400" />}
                          <p className="truncate text-sm font-medium text-stone-700">
                            {h.conversation_title || t("common.untitledConversation")}
                          </p>
                        </div>
                        {h.snippet && (
                          <div className="mt-0.5 line-clamp-2 text-xs leading-relaxed text-stone-500">
                            <FtsSnippet text={h.snippet} />
                          </div>
                        )}
                      </div>
                      {isMsg && (
                        <span className="mt-0.5 shrink-0 rounded border border-orange-200 bg-orange-50 px-1.5 py-0.5 text-[10px] text-orange-600">
                          {t("commandPalette.jumpToMessage")}
                        </span>
                      )}
                    </button>
                  );
                })}
            </>
          ) : (
            <>
              {loading && (
                <div className="flex flex-col items-center gap-3 py-10 text-stone-400">
                  <Loader2 size={22} className="animate-spin" />
                  <p className="text-sm">{t("pathDialog.loading")}</p>
                </div>
              )}

              {!loading &&
                items.map((c) => (
                  <button
                    key={c.id}
                    onClick={() => goTo(c.id)}
                    className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-stone-50 ${
                      c.id === currentId ? "bg-amber-50" : ""
                    }`}
                  >
                    <div
                      className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br text-white text-[11px] font-bold shadow-sm ${
                        platformColor[c.platform] ?? "from-stone-400 to-stone-600"
                      }`}
                    >
                      {platformLabel(c.platform, isChina)[0]}
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-stone-700">
                        {c.title || t("common.untitledConversation")}
                      </p>
                      <div className="flex items-center gap-1.5 text-xs text-stone-400">
                        <span>{platformLabel(c.platform, isChina)}</span>
                        <span>·</span>
                        <MessageSquare size={10} />
                        <span>{t("common.messageCount", { n: c.message_count })}</span>
                        <span>·</span>
                        <span>{formatRelativeDate(c.updated_at ?? c.created_at, lang, t)}</span>
                      </div>
                    </div>
                  </button>
                ))}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
