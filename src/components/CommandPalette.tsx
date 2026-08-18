import { useEffect, useRef, useState, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { Search, MessageSquare, Clock, X, FileText, Heading, Star, History, BrainCircuit } from "lucide-react";
import { api, type ConversationSummary, type SearchHit, type FavoriteRow, type EmbedConfig } from "../lib/api";
import { FtsSnippet, QuerySnippet } from "../lib/highlight";
import { getRecentSearches, addRecentSearch, removeRecentSearch } from "../lib/searchHistory";
import { useI18n, formatCompactRelativeDate } from "../lib/i18n";
import { useRegion } from "../lib/region";
import { platformLabel } from "../lib/platforms";

interface Props {
  onClose: () => void;
  embedIndexed?: number;
}

const platformBadge: Record<string, string> = {
  claude:        "bg-orange-100 text-orange-700",
  chatgpt:       "bg-emerald-100 text-emerald-700",
  deepseek:      "bg-blue-100 text-blue-700",
  "claude-code": "bg-amber-100 text-amber-700",
  codex:         "bg-sky-100 text-sky-700",
};

// Unified item shape for keyboard nav
type Item =
  | { kind: "conv"; data: ConversationSummary }
  | { kind: "hit"; data: SearchHit }
  | { kind: "fav"; data: FavoriteRow };

export function CommandPalette({ onClose, embedIndexed = 0 }: Props) {
  const { t, lang } = useI18n();
  const { isChina } = useRegion();
  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState<ConversationSummary[]>([]);
  const [hits, setHits]       = useState<SearchHit[]>([]);
  const [favHits, setFavHits] = useState<FavoriteRow[]>([]);
  const [active, setActive] = useState(0);
  const [loading, setLoading] = useState(false);
  const [recentSearches, setRecentSearches] = useState<string[]>(() => getRecentSearches());
  // While an IME composition (e.g. Chinese pinyin) is in progress, onChange
  // fires for every intermediate romanization/candidate — none of those are
  // a real committed search, so search/history recording waits for compositionend.
  const [isComposing, setIsComposing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();

  // Semantic (AI) search mode
  const embedAvail = embedIndexed > 0;
  const [semanticMode, setSemanticMode] = useState(false);
  const [embedConfig, setEmbedConfig] = useState<EmbedConfig | null>(null);
  const [semanticHits, setSemanticHits] = useState<SearchHit[]>([]);
  const [semanticLoading, setSemanticLoading] = useState(false);

  useEffect(() => {
    api.listConversations({}, 8, 0).then(setRecent).catch(() => {});
    inputRef.current?.focus();
  }, []);

  // Load embed config once semantic mode is first used
  useEffect(() => {
    if (!semanticMode || embedConfig) return;
    Promise.all([
      api.getSetting("embed_api_url"),
      api.getSetting("embed_model"),
      api.getSetting("embed_api_key"),
    ]).then(([url, model, key]) => {
      setEmbedConfig({
        apiUrl: url ?? "http://127.0.0.1:11434",
        model: model ?? "nomic-embed-text",
        apiKey: key ?? "",
      });
    });
  }, [semanticMode, embedConfig]);

  // Keyword (FTS) search
  useEffect(() => {
    if (semanticMode) return;
    if (!query.trim()) { setHits([]); setFavHits([]); setActive(0); return; }
    if (isComposing) return;
    setLoading(true);
    const t = setTimeout(() => {
      const q = query.trim();
      setRecentSearches(addRecentSearch(q));
      Promise.all([
        api.searchAll(q),
        api.searchFavorites(q),
      ])
        .then(([r, favs]) => { setHits(r); setFavHits(favs); setActive(0); })
        .catch(() => {})
        .finally(() => setLoading(false));
    }, 1500);
    return () => clearTimeout(t);
  }, [query, semanticMode, isComposing]);

  // Semantic (AI) search
  useEffect(() => {
    if (!semanticMode || !embedConfig || !query.trim()) { setSemanticHits([]); return; }
    if (isComposing) return;
    setSemanticLoading(true);
    const t = setTimeout(() => {
      const q = query.trim();
      setRecentSearches(addRecentSearch(q));
      api.semanticSearch(q, embedConfig.apiUrl, embedConfig.model, embedConfig.apiKey, 20)
        .then((r) => { setSemanticHits(r); setActive(0); })
        .catch(() => setSemanticHits([]))
        .finally(() => setSemanticLoading(false));
    }, 1500);
    return () => { clearTimeout(t); setSemanticLoading(false); };
  }, [query, semanticMode, embedConfig, isComposing]);

  const items: Item[] = semanticMode
    ? semanticHits.map((h): Item => ({ kind: "hit", data: h }))
    : query.trim()
    ? [
        ...hits.map((h): Item => ({ kind: "hit", data: h })),
        ...favHits.map((f): Item => ({ kind: "fav", data: f })),
      ]
    : recent.map((c): Item => ({ kind: "conv", data: c }));

  const open = useCallback(
    (item: Item) => {
      onClose();
      if (item.kind === "conv") {
        navigate(`/conversation/${item.data.id}`);
      } else if (item.kind === "fav") {
        const f = item.data;
        const msgParam = f.message_id ? `?msg=${encodeURIComponent(f.message_id)}` : "";
        navigate(`/conversation/${f.conversation_id}${msgParam}`);
      } else {
        const h = item.data;
        const msgParam = h.kind === "message" || h.kind.startsWith("semantic:")
          ? `?msg=${encodeURIComponent(h.ref_id)}`
          : "";
        navigate(`/conversation/${h.conversation_id}${msgParam}`);
      }
    },
    [navigate, onClose],
  );

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") { onClose(); return; }
      if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(a + 1, items.length - 1)); }
      if (e.key === "ArrowUp")   { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)); }
      if (e.key === "Enter" && items[active]) open(items[active]);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [active, items, onClose, open]);

  useEffect(() => {
    const el = listRef.current?.querySelector(`[data-idx="${active}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-[15vh] backdrop-blur-sm"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="w-[600px] overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200 animate-palette-in">
        {/* Input */}
        <div className="flex items-center gap-3 border-b border-stone-100 px-4 py-3.5">
          {semanticMode ? (
            <BrainCircuit size={18} className="shrink-0 text-violet-500" />
          ) : (
            <Search size={18} className="shrink-0 text-stone-400" />
          )}
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onCompositionStart={() => setIsComposing(true)}
            onCompositionEnd={(e) => { setIsComposing(false); setQuery(e.currentTarget.value); }}
            placeholder={semanticMode ? t("commandPalette.searchPlaceholderSemantic") : t("commandPalette.searchPlaceholderKeyword")}
            className="flex-1 bg-transparent text-base text-stone-800 outline-none placeholder:text-stone-400"
          />
          {query && (
            <button onClick={() => setQuery("")} className="text-stone-400 hover:text-stone-600">
              <X size={16} />
            </button>
          )}
          {(loading || semanticLoading) && (
            <div className="h-4 w-4 animate-spin rounded-full border-2 border-stone-200 border-t-orange-400" />
          )}
          {embedAvail && (
            <button
              onClick={() => setSemanticMode((m) => !m)}
              title={semanticMode ? t("commandPalette.toggleToKeyword") : t("commandPalette.toggleToSemantic")}
              className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border transition-colors ${
                semanticMode
                  ? "border-violet-300 bg-violet-100 text-violet-700"
                  : "border-stone-200 bg-stone-50 text-stone-400 hover:bg-stone-100"
              }`}
            >
              <BrainCircuit size={13} />
            </button>
          )}
          <kbd className="rounded border border-stone-200 bg-stone-50 px-1.5 py-0.5 text-[11px] text-stone-400">ESC</kbd>
        </div>

        {/* Results */}
        <div ref={listRef} className="max-h-[60vh] overflow-y-auto">
          {!query.trim() && recentSearches.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5 border-b border-stone-100 px-4 py-2.5">
              <History size={12} className="shrink-0 text-stone-400" />
              {recentSearches.map((term) => (
                <span
                  key={term}
                  className="flex items-center gap-1 rounded-full border border-stone-200 bg-stone-50 pl-2.5 pr-1 py-0.5 text-xs text-stone-600 hover:border-orange-200 hover:bg-orange-50 hover:text-orange-700"
                >
                  <button onClick={() => setQuery(term)} className="max-w-[140px] truncate">
                    {term}
                  </button>
                  <button
                    onClick={() => setRecentSearches(removeRecentSearch(term))}
                    className="rounded-full p-0.5 text-stone-400 hover:bg-stone-200 hover:text-stone-600"
                  >
                    <X size={10} />
                  </button>
                </span>
              ))}
            </div>
          )}

          {!query.trim() && !semanticMode && (
            <div className="flex items-center gap-1.5 px-4 py-2 text-xs font-medium text-stone-400">
              <Clock size={12} />
              {t("commandPalette.recentConversations")}
            </div>
          )}

          {!query.trim() && semanticMode && (
            <div className="flex flex-col items-center justify-center gap-3 py-16 text-stone-400">
              <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-violet-50">
                <BrainCircuit size={26} strokeWidth={1.5} className="text-violet-400" />
              </div>
              <div className="text-center">
                <p className="font-medium text-stone-600">{t("commandPalette.semanticModeTitle")}</p>
                <p className="mt-1 text-sm text-stone-400">{t("commandPalette.semanticModeDesc")}</p>
              </div>
            </div>
          )}

          {query.trim() && !semanticMode && hits.length === 0 && favHits.length === 0 && !loading && (
            <div className="py-10 text-center text-sm text-stone-400">{t("commandPalette.noMatchingConversations")}</div>
          )}

          {query.trim() && semanticMode && semanticHits.length === 0 && !semanticLoading && (
            <div className="flex flex-col items-center gap-2 py-10 text-stone-400">
              <BrainCircuit size={20} className="text-violet-300" />
              <span className="text-sm">{t("commandPalette.noRelatedFound")}</span>
            </div>
          )}

          {items.map((item, i) => {
            const isActive = i === active;
            const base = `flex w-full items-start gap-3 px-4 py-3 text-left transition-colors ${isActive ? "bg-orange-50" : "hover:bg-stone-50"}`;

            if (item.kind === "conv") {
              const c = item.data;
              return (
                <button key={c.id} data-idx={i} onClick={() => open(item)} onMouseEnter={() => setActive(i)} className={base}>
                  <span className={`mt-0.5 shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium ${platformBadge[c.platform] ?? "bg-stone-100 text-stone-500"}`}>
                    {platformLabel(c.platform, isChina)}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-stone-800">{c.title || t("common.untitledConversation")}</div>
                    {c.summary && <div className="mt-0.5 truncate text-xs text-stone-500">{c.summary}</div>}
                  </div>
                  <div className="shrink-0 text-xs text-stone-400">{formatCompactRelativeDate(c.updated_at ?? c.created_at, lang, t)}</div>
                  <div className="shrink-0 flex items-center gap-1 text-xs text-stone-400">
                    <MessageSquare size={11} />{c.message_count}
                  </div>
                </button>
              );
            }

            // Favorite hit
            if (item.kind === "fav") {
              const f = item.data;
              return (
                <button key={`fav-${f.id}`} data-idx={i} onClick={() => open(item)} onMouseEnter={() => setActive(i)} className={base}>
                  <Star size={14} className="mt-0.5 shrink-0 text-amber-500" fill="currentColor" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-stone-700">{f.conversation_title || t("common.untitledConversation")}</div>
                    <div className="mt-0.5 line-clamp-2 text-xs leading-relaxed text-stone-500">
                      {f.selected_text.slice(0, 120)}
                    </div>
                    {f.tags.length > 0 && (
                      <div className="mt-1 flex flex-wrap gap-1">
                        {f.tags.map((tag) => (
                          <span key={tag.id} className="rounded-full bg-amber-50 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">
                            #{tag.name}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                  <span className="mt-1 shrink-0 rounded border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[10px] text-amber-600">
                    {t("common.favorite")}
                  </span>
                </button>
              );
            }

            // SearchHit — either a keyword FTS match (message/title) or a semantic match
            const h = item.data;
            const isMsg = h.kind === "message";
            const sim = h.kind.startsWith("semantic:") ? parseFloat(h.kind.slice(9)) : null;
            return (
              <button key={h.ref_id} data-idx={i} onClick={() => open(item)} onMouseEnter={() => setActive(i)} className={base}>
                <div className="mt-0.5 shrink-0 flex flex-col items-center gap-1">
                  <span className={`rounded px-1.5 py-0.5 text-[11px] font-medium ${platformBadge[h.platform] ?? "bg-stone-100 text-stone-500"}`}>
                    {platformLabel(h.platform, isChina)}
                  </span>
                  {sim !== null
                    ? <BrainCircuit size={11} className="text-violet-400" />
                    : isMsg
                    ? <FileText size={11} className="text-stone-400" />
                    : <Heading size={11} className="text-stone-400" />}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-stone-700">{h.conversation_title || t("common.untitledConversation")}</div>
                  {h.snippet && (
                    <div className="mt-0.5 line-clamp-2 text-xs leading-relaxed text-stone-500">
                      {sim !== null ? <QuerySnippet text={h.snippet} query={query} /> : <FtsSnippet text={h.snippet} />}
                    </div>
                  )}
                </div>
                {sim !== null ? (
                  <span className="mt-1 shrink-0 flex items-center gap-1 rounded border border-violet-200 bg-violet-50 px-1.5 py-0.5 text-[10px] text-violet-600">
                    {t("commandPalette.similarity", { pct: Math.round(sim * 100) })}
                  </span>
                ) : isMsg && (
                  <span className="mt-1 shrink-0 rounded border border-orange-200 bg-orange-50 px-1.5 py-0.5 text-[10px] text-orange-600">
                    {t("commandPalette.jumpToMessage")}
                  </span>
                )}
              </button>
            );
          })}
        </div>

        {/* Footer */}
        <div className="flex items-center gap-4 border-t border-stone-100 bg-stone-50 px-4 py-2 text-[11px] text-stone-400">
          <span className="flex items-center gap-1"><kbd className="rounded border border-stone-200 bg-white px-1 py-0.5">↑↓</kbd> {t("commandPalette.footerSelect")}</span>
          <span className="flex items-center gap-1"><kbd className="rounded border border-stone-200 bg-white px-1 py-0.5">↵</kbd> {t("commandPalette.footerOpen")}</span>
          <span className="flex items-center gap-1"><kbd className="rounded border border-stone-200 bg-white px-1 py-0.5">ESC</kbd> {t("commandPalette.footerClose")}</span>
          <span className="ml-auto">{t("commandPalette.resultsCount", { n: items.length })}</span>
        </div>
      </div>
    </div>
  );
}
