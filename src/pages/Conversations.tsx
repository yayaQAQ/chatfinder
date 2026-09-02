import { useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import {
  Search, MessageSquare, Inbox, Loader2, X,
  SlidersHorizontal, ArrowUpDown, Calendar, Hash, BrainCircuit,
  History, FileText, Heading, Star, UserRound, Bot, Trash2,
} from "lucide-react";
import { api, type ConversationSummary, type ConversationFilter, type SortOption, type RoleFilter, type SearchHit, type FavoriteRow, type EmbedConfig } from "../lib/api";
import { QuerySnippet, FtsSnippet } from "../lib/highlight";
import { getRecentSearches, addRecentSearch, removeRecentSearch } from "../lib/searchHistory";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { useToast } from "../lib/toast";
import { useI18n, formatRelativeDate, type TranslationKey } from "../lib/i18n";
import { platformLabel } from "../lib/platforms";

const PAGE_SIZE = 60;

const platformBadge: Record<string, string> = {
  claude:        "bg-orange-100 text-orange-700 border-orange-200",
  chatgpt:       "bg-emerald-100 text-emerald-700 border-emerald-200",
  deepseek:      "bg-blue-100 text-blue-700 border-blue-200",
  "claude-code": "bg-amber-100 text-amber-700 border-amber-200",
  codex:         "bg-sky-100 text-sky-700 border-sky-200",
};

const SORT_OPTIONS: { value: SortOption; labelKey: TranslationKey }[] = [
  { value: "newest",          labelKey: "conversations.sortNewest" },
  { value: "oldest",          labelKey: "conversations.sortOldest" },
  { value: "most_messages",   labelKey: "conversations.sortMostMessages" },
  { value: "fewest_messages", labelKey: "conversations.sortFewestMessages" },
];

interface ActiveFilter {
  label: string;
  clear: () => void;
}

export function Conversations({
  refreshKey,
  embedIndexed = 0,
  onDataChanged,
}: {
  refreshKey: number;
  embedIndexed?: number;
  onDataChanged?: () => void;
}) {
  const { t, lang } = useI18n();
  const { push } = useToast();
  const [pendingDelete, setPendingDelete] = useState<ConversationSummary | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [searchParams, setSearchParams] = useSearchParams();

  // Filter state in URL — restores on back-navigation (component remounts from URL)
  const platform     = searchParams.get("platform") ?? "";
  const dateFrom     = searchParams.get("from")     ?? "";
  const dateTo       = searchParams.get("to")       ?? "";
  const minMsg       = searchParams.get("min")      ?? "";
  const maxMsg       = searchParams.get("max")      ?? "";
  const role         = (searchParams.get("role")    ?? "") as RoleFilter;
  const sort         = (searchParams.get("sort")    ?? "newest") as SortOption;
  const semanticMode = searchParams.get("semantic") === "1";

  // query is local state so IME composition (Chinese pinyin etc.) works correctly.
  // On mount the component reads the URL, so back-navigation still restores the value.
  const [query, setQuery] = useState(() => searchParams.get("q") ?? "");
  // While an IME composition (e.g. Chinese pinyin) is in progress, onChange
  // fires for every intermediate romanization/candidate — none of those are
  // a real committed search, so search/history recording waits for compositionend.
  const [isComposing, setIsComposing] = useState(false);

  // Debounce-sync local query → URL (so URL stays accurate for back-nav without
  // every keystroke triggering a re-render from URL params)
  useEffect(() => {
    const h = setTimeout(() => {
      setSearchParams((p) => {
        const next = new URLSearchParams(p);
        if (query) next.set("q", query); else next.delete("q");
        return next;
      }, { replace: true });
    }, 200);
    return () => clearTimeout(h);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query]);

  const [showFilter, setShowFilter] = useState(false);
  const [searchFocused, setSearchFocused] = useState(false);
  // Sticky once true: the role filter only matters for search hits, but we want
  // it visible in the filter panel as soon as the user engages with the search
  // box, not only after they've typed something and lose it again on blur.
  const [searchEverFocused, setSearchEverFocused] = useState(() => searchParams.get("q") ? true : false);
  const [recentSearches, setRecentSearches] = useState<string[]>(() => getRecentSearches());

  const setParam = (key: string, val: string) =>
    setSearchParams((p) => {
      const next = new URLSearchParams(p);
      if (val) next.set(key, val); else next.delete(key);
      return next;
    }, { replace: true });

  const setPlatform = (v: string)      => setParam("platform", v);
  const setDateFrom = (v: string)      => setParam("from", v);
  const setDateTo   = (v: string)      => setParam("to", v);
  const setMinMsg   = (v: string)      => setParam("min", v);
  const setMaxMsg   = (v: string)      => setParam("max", v);
  const setRole     = (v: RoleFilter)  => setParam("role", v);
  const setSort     = (v: SortOption)  => setParam("sort", v === "newest" ? "" : v);

  const hasQuery = query.trim().length > 0;

  const structuralFilter = { platform, dateFrom, dateTo, minMessages: minMsg ? parseInt(minMsg) : undefined, maxMessages: maxMsg ? parseInt(maxMsg) : undefined, role };

  // ── Browse mode (no query): paginated conversation card grid ──
  const [items, setItems]           = useState<ConversationSummary[]>([]);
  const [loading, setLoading]       = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore]       = useState(false);
  const sentinelRef = useRef<HTMLDivElement>(null);

  const browseFilter: ConversationFilter = { ...structuralFilter, sort };

  useEffect(() => {
    if (hasQuery) return;
    let active = true;
    setLoading(true);
    const handle = setTimeout(() => {
      api.listConversations(browseFilter, PAGE_SIZE, 0)
        .then((res) => {
          if (!active) return;
          setItems(res);
          setHasMore(res.length === PAGE_SIZE);
        })
        .finally(() => active && setLoading(false));
    }, 180);
    return () => { active = false; clearTimeout(handle); };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasQuery, platform, dateFrom, dateTo, minMsg, maxMsg, sort, refreshKey]);

  useEffect(() => {
    if (hasQuery) return;
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && hasMore && !loading && !loadingMore) {
          setLoadingMore(true);
          api.listConversations(browseFilter, PAGE_SIZE, items.length)
            .then((res) => {
              setItems((prev) => [...prev, ...res]);
              setHasMore(res.length === PAGE_SIZE);
            })
            .finally(() => setLoadingMore(false));
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasQuery, platform, dateFrom, dateTo, minMsg, maxMsg, sort, items.length, hasMore, loading, loadingMore]);

  // ── Search mode (query present): keyword or semantic hits + favorites ──
  const embedAvail = embedIndexed > 0;
  const [embedConfig, setEmbedConfig] = useState<EmbedConfig | null>(null);
  const [hits, setHits]       = useState<SearchHit[]>([]);
  const [favHits, setFavHits] = useState<FavoriteRow[]>([]);
  const [searchLoading, setSearchLoading] = useState(false);

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

  useEffect(() => {
    if (!hasQuery) { setHits([]); setFavHits([]); return; }
    if (isComposing) return;
    if (semanticMode && !embedConfig) return;
    setSearchLoading(true);
    const handle = setTimeout(() => {
      const q = query.trim();
      setRecentSearches(addRecentSearch(q));
      const searchPromise = semanticMode
        ? api.semanticSearch(q, embedConfig!.apiUrl, embedConfig!.model, embedConfig!.apiKey, 40, structuralFilter)
        : api.searchAll(q, structuralFilter);
      Promise.all([searchPromise, semanticMode ? Promise.resolve([]) : api.searchFavorites(q)])
        .then(([r, favs]) => { setHits(r); setFavHits(favs); })
        .catch(() => { setHits([]); setFavHits([]); })
        .finally(() => setSearchLoading(false));
    }, 1500);
    return () => { clearTimeout(handle); setSearchLoading(false); };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, hasQuery, isComposing, semanticMode, embedConfig, platform, dateFrom, dateTo, minMsg, maxMsg, role]);

  // Build human-readable active filter chips
  const activeFilters: ActiveFilter[] = [];
  if (platform)   activeFilters.push({ label: platformLabel(platform), clear: () => setPlatform("") });
  if (dateFrom)   activeFilters.push({ label: t("conversations.fromPrefix", { date: dateFrom }), clear: () => setDateFrom("") });
  if (dateTo)     activeFilters.push({ label: t("conversations.toPrefix", { date: dateTo }), clear: () => setDateTo("") });
  if (minMsg)     activeFilters.push({ label: t("conversations.minMessagesChip", { n: minMsg }), clear: () => setMinMsg("") });
  if (maxMsg)     activeFilters.push({ label: t("conversations.maxMessagesChip", { n: maxMsg }), clear: () => setMaxMsg("") });
  if (hasQuery && role) activeFilters.push({ label: role === "human" ? t("conversations.scopeHumanOnly") : t("conversations.scopeAssistantOnly"), clear: () => setRole("") });
  if (!hasQuery && sort !== "newest") {
    const label = t(SORT_OPTIONS.find(o => o.value === sort)?.labelKey ?? "conversations.sortNewest");
    activeFilters.push({ label, clear: () => setSort("newest") });
  }

  const clearAll = () => {
    setSearchParams((p) => {
      const next = new URLSearchParams(p);
      ["platform", "from", "to", "min", "max", "role", "sort"].forEach((k) => next.delete(k));
      return next;
    }, { replace: true });
  };

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setDeleting(true);
    try {
      await api.deleteConversation(pendingDelete.id);
      push(t("common.deleteConversationToast"), "success");
      setItems((prev) => prev.filter((c) => c.id !== pendingDelete.id));
      setPendingDelete(null);
      onDataChanged?.();
    } catch (e) {
      push(String(e), "error");
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="flex h-full flex-col bg-[#f7f5f2]">
      {/* ── Header / search hero ── */}
      <div className="border-b border-stone-200 bg-white px-6 pt-6 pb-4 shadow-sm">
        <div className="mx-auto flex max-w-3xl flex-col gap-3">
          {/* Big search bar */}
          <div className="flex gap-2">
            <div className="relative flex-1">
              {semanticMode ? (
                <BrainCircuit size={18} className="absolute left-4 top-1/2 -translate-y-1/2 text-violet-500 pointer-events-none" />
              ) : (
                <Search size={18} className="absolute left-4 top-1/2 -translate-y-1/2 text-stone-400 pointer-events-none" />
              )}
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onCompositionStart={() => setIsComposing(true)}
                onCompositionEnd={(e) => { setIsComposing(false); setQuery(e.currentTarget.value); }}
                onFocus={() => { setSearchFocused(true); setSearchEverFocused(true); }}
                onBlur={() => setTimeout(() => setSearchFocused(false), 120)}
                placeholder={semanticMode ? t("conversations.searchPlaceholderSemantic") : t("conversations.searchPlaceholderKeyword")}
                className={`h-12 w-full rounded-2xl border pl-12 pr-10 text-[15px] outline-none transition-colors placeholder:text-stone-400 ${
                  semanticMode
                    ? "border-violet-300 bg-violet-50 focus:border-violet-400 focus:bg-white focus:ring-4 focus:ring-violet-100"
                    : "border-stone-200 bg-stone-50 focus:border-orange-300 focus:bg-white focus:ring-4 focus:ring-orange-100"
                }`}
              />
              {query && (
                <button onClick={() => setQuery("")} className="absolute right-4 top-1/2 -translate-y-1/2 text-stone-400 hover:text-stone-600">
                  <X size={15} />
                </button>
              )}
              {searchLoading && (
                <Loader2 size={14} className="absolute right-10 top-1/2 -translate-y-1/2 animate-spin text-violet-400" />
              )}

              {/* Recent search dropdown */}
              {searchFocused && !hasQuery && recentSearches.length > 0 && (
                <div className="absolute left-0 right-0 top-full z-10 mt-1.5 overflow-hidden rounded-xl border border-stone-200 bg-white py-1.5 shadow-lg">
                  <div className="flex items-center gap-1.5 px-3 py-1 text-[11px] font-medium text-stone-400">
                    <History size={11} />
                    {t("conversations.recentSearches")}
                  </div>
                  {recentSearches.map((term) => (
                    <div key={term} className="group flex items-center justify-between px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50">
                      <button
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => setQuery(term)}
                        className="flex-1 truncate text-left"
                      >
                        {term}
                      </button>
                      <button
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => setRecentSearches(removeRecentSearch(term))}
                        className="ml-2 shrink-0 rounded p-0.5 text-stone-300 opacity-0 hover:bg-stone-200 hover:text-stone-600 group-hover:opacity-100"
                      >
                        <X size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {embedAvail && (
              <button
                onClick={() => setParam("semantic", semanticMode ? "" : "1")}
                title={semanticMode ? t("conversations.toggleToKeyword") : t("conversations.toggleToSemantic")}
                className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border transition-colors ${
                  semanticMode
                    ? "border-violet-300 bg-violet-100 text-violet-700"
                    : "border-stone-200 bg-stone-50 text-stone-500 hover:bg-stone-100"
                }`}
              >
                <BrainCircuit size={18} />
              </button>
            )}

            <button
              onClick={() => setShowFilter(!showFilter)}
              className={`flex h-12 shrink-0 items-center gap-1.5 rounded-2xl border px-4 text-sm font-medium transition-colors ${
                showFilter || activeFilters.length > 0
                  ? "border-orange-300 bg-orange-50 text-orange-700"
                  : "border-stone-200 bg-stone-50 text-stone-600 hover:bg-white"
              }`}
            >
              <SlidersHorizontal size={15} />
              {t("conversations.filter")}
              {activeFilters.length > 0 && (
                <span className="flex h-4 w-4 items-center justify-center rounded-full bg-orange-500 text-[10px] text-white font-bold">
                  {activeFilters.length}
                </span>
              )}
            </button>
          </div>

          {/* Quick platform pills — always visible so search + filter compose in one motion */}
          <div className="flex flex-wrap items-center gap-1.5">
            {(["", "claude", "chatgpt", "deepseek", "claude-code", "codex"] as const).map((p) => (
              <button
                key={p}
                onClick={() => setPlatform(platform === p ? "" : p)}
                className={`rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                  platform === p
                    ? "border-orange-300 bg-orange-100 text-orange-700"
                    : "border-stone-200 bg-white text-stone-600 hover:bg-stone-100"
                }`}
              >
                {p === "" ? t("conversations.allPlatforms") : platformLabel(p)}
              </button>
            ))}
            {!hasQuery && (
              <>
                <span className="mx-1 h-3.5 w-px bg-stone-200" />
                {SORT_OPTIONS.map((o) => (
                  <button
                    key={o.value}
                    onClick={() => setSort(o.value)}
                    className={`rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                      sort === o.value
                        ? "border-stone-300 bg-stone-100 text-stone-700"
                        : "border-stone-200 bg-white text-stone-500 hover:bg-stone-100"
                    }`}
                  >
                    {t(o.labelKey)}
                  </button>
                ))}
              </>
            )}
          </div>

          {/* Filter panel (date range, message count) */}
          {showFilter && (
            <div className="rounded-xl border border-stone-200 bg-stone-50 p-4 space-y-4">
              <div className="flex items-start gap-3">
                <label className="w-28 shrink-0 whitespace-nowrap pt-1 text-xs font-medium text-stone-500 flex items-center gap-1">
                  <Calendar size={11} />
                  {t("conversations.filterDateLabel")}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type="date"
                    value={dateFrom}
                    onChange={(e) => setDateFrom(e.target.value)}
                    className="h-8 rounded-lg border border-stone-200 bg-white px-2.5 text-xs text-stone-700 outline-none focus:border-orange-300 focus:ring-1 focus:ring-orange-200"
                  />
                  <span className="text-xs text-stone-400">{t("conversations.dateRangeSeparator")}</span>
                  <input
                    type="date"
                    value={dateTo}
                    onChange={(e) => setDateTo(e.target.value)}
                    className="h-8 rounded-lg border border-stone-200 bg-white px-2.5 text-xs text-stone-700 outline-none focus:border-orange-300 focus:ring-1 focus:ring-orange-200"
                  />
                </div>
              </div>

              <div className="flex items-start gap-3">
                <label className="w-28 shrink-0 whitespace-nowrap pt-1 text-xs font-medium text-stone-500 flex items-center gap-1">
                  <Hash size={11} />
                  {t("conversations.filterMessageCountLabel")}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type="number"
                    min="0"
                    value={minMsg}
                    onChange={(e) => setMinMsg(e.target.value)}
                    placeholder={t("conversations.minPlaceholder")}
                    className="h-8 w-20 rounded-lg border border-stone-200 bg-white px-2.5 text-xs text-stone-700 outline-none focus:border-orange-300 focus:ring-1 focus:ring-orange-200"
                  />
                  <span className="text-xs text-stone-400">—</span>
                  <input
                    type="number"
                    min="0"
                    value={maxMsg}
                    onChange={(e) => setMaxMsg(e.target.value)}
                    placeholder={t("conversations.maxPlaceholder")}
                    className="h-8 w-20 rounded-lg border border-stone-200 bg-white px-2.5 text-xs text-stone-700 outline-none focus:border-orange-300 focus:ring-1 focus:ring-orange-200"
                  />
                </div>
              </div>

              {(hasQuery || searchEverFocused) && (
                <div className="flex items-start gap-3">
                  <label className="w-28 shrink-0 whitespace-nowrap pt-1 text-xs font-medium text-stone-500 flex items-center gap-1">
                    <UserRound size={11} />
                    {t("conversations.filterScopeLabel")}
                  </label>
                  <div className="flex items-center gap-1.5">
                    {([
                      { value: "", labelKey: "conversations.scopeAll", icon: null },
                      { value: "human", labelKey: "conversations.scopeHumanOnly", icon: UserRound },
                      { value: "assistant", labelKey: "conversations.scopeAssistantOnly", icon: Bot },
                    ] as const).map((o) => (
                      <button
                        key={o.value}
                        onClick={() => setRole(o.value)}
                        className={`flex items-center gap-1 rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                          role === o.value
                            ? "border-orange-300 bg-orange-100 text-orange-700"
                            : "border-stone-200 bg-white text-stone-600 hover:bg-stone-100"
                        }`}
                      >
                        {o.icon && <o.icon size={11} />}
                        {t(o.labelKey)}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {!hasQuery && (
                <div className="flex items-start gap-3">
                  <label className="w-28 shrink-0 whitespace-nowrap pt-1 text-xs font-medium text-stone-500 flex items-center gap-1">
                    <ArrowUpDown size={11} />
                    {t("conversations.sortLabel")}
                  </label>
                  <p className="pt-1.5 text-xs text-stone-400">{t("conversations.sortAlreadyAvailable")}</p>
                </div>
              )}

              {activeFilters.length > 0 && (
                <div className="pt-1 border-t border-stone-200 flex justify-end">
                  <button onClick={clearAll} className="text-xs text-stone-400 hover:text-stone-700">
                    {t("conversations.clearAllFilters")}
                  </button>
                </div>
              )}
            </div>
          )}

          {/* Active filter chips */}
          {!showFilter && activeFilters.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {activeFilters.map((f, i) => (
                <span key={i} className="flex items-center gap-1 rounded-full bg-orange-100 border border-orange-200 px-2.5 py-0.5 text-xs text-orange-700">
                  {f.label}
                  <button onClick={f.clear} className="hover:text-orange-900">
                    <X size={10} />
                  </button>
                </span>
              ))}
              <button onClick={clearAll} className="text-xs text-stone-400 hover:text-stone-600 px-1">
                {t("conversations.clearAll")}
              </button>
            </div>
          )}
        </div>
      </div>

      {/* ── Body ── */}
      <div className="flex-1 overflow-y-auto px-6 py-5">
        <div className={`mx-auto ${hasQuery ? "max-w-3xl" : ""}`}>
          {hasQuery ? (
            <>
              {searchLoading && hits.length === 0 && favHits.length === 0 && (
                <div className="flex items-center justify-center py-20">
                  <div className={`flex flex-col items-center gap-3 ${semanticMode ? "text-violet-400" : "text-stone-400"}`}>
                    {semanticMode ? <BrainCircuit size={24} className="animate-pulse" /> : <Loader2 size={24} className="animate-spin" />}
                    <span className="text-sm">{semanticMode ? t("conversations.semanticSearching") : t("conversations.keywordSearching")}</span>
                  </div>
                </div>
              )}

              {!searchLoading && hits.length === 0 && favHits.length === 0 && (
                <div className="flex flex-col items-center justify-center gap-4 py-24 text-stone-400">
                  <div className={`flex h-16 w-16 items-center justify-center rounded-2xl ${semanticMode ? "bg-violet-50" : "bg-stone-100"}`}>
                    {semanticMode ? <BrainCircuit size={28} strokeWidth={1.5} className="text-violet-300" /> : <Search size={28} strokeWidth={1.5} />}
                  </div>
                  <p className="font-medium text-stone-600">{t("conversations.noResults")}</p>
                </div>
              )}

              {(hits.length > 0 || favHits.length > 0) && (
                <div className="flex flex-col gap-2">
                  <p className="mb-1 text-xs text-stone-400">
                    {t("conversations.resultsFound", { n: hits.length + favHits.length })}
                  </p>

                  {hits.map((h) => {
                    const isMsg = h.kind === "message";
                    const sim = h.kind.startsWith("semantic:") ? parseFloat(h.kind.slice(9)) : null;
                    const msgParam = isMsg || sim !== null ? `?msg=${encodeURIComponent(h.ref_id)}` : "";
                    return (
                      <Link
                        key={h.ref_id}
                        to={`/conversation/${h.conversation_id}${msgParam}`}
                        className="group flex flex-col rounded-2xl border border-stone-200 bg-white p-4 shadow-sm transition-all duration-150 hover:-translate-y-0.5 hover:border-stone-300 hover:shadow-md"
                      >
                        <div className="mb-2 flex items-center gap-2">
                          <span className={`rounded-full border px-2 py-0.5 text-[11px] font-medium ${platformBadge[h.platform] ?? "bg-stone-100 text-stone-600 border-stone-200"}`}>
                            {platformLabel(h.platform)}
                          </span>
                          {sim !== null ? (
                            <FileText size={11} className="text-violet-400" />
                          ) : isMsg ? (
                            <FileText size={11} className="text-stone-400" />
                          ) : (
                            <Heading size={11} className="text-stone-400" />
                          )}
                          <span className="text-xs text-stone-400">{formatRelativeDate(h.updated_at ?? h.created_at, lang, t)}</span>
                          {sim !== null && (
                            <span className="ml-auto flex items-center gap-1 rounded-full bg-violet-50 border border-violet-200 px-2 py-0.5 text-[11px] font-medium text-violet-600">
                              <BrainCircuit size={10} />
                              {t("conversations.similarity", { pct: Math.round(sim * 100) })}
                            </span>
                          )}
                          {sim === null && isMsg && (
                            <span className="ml-auto rounded border border-orange-200 bg-orange-50 px-1.5 py-0.5 text-[10px] text-orange-600 opacity-0 transition-opacity group-hover:opacity-100">
                              {t("conversations.jumpToMessage")}
                            </span>
                          )}
                        </div>
                        <h3 className="mb-1 line-clamp-1 text-sm font-semibold leading-snug text-stone-800 transition-colors group-hover:text-orange-700">
                          {h.conversation_title || t("common.untitledConversation")}
                        </h3>
                        {h.snippet && (
                          <p className="line-clamp-2 text-xs leading-relaxed text-stone-500">
                            {sim !== null ? <QuerySnippet text={h.snippet} query={query} /> : <FtsSnippet text={h.snippet} />}
                          </p>
                        )}
                      </Link>
                    );
                  })}

                  {favHits.map((f) => (
                    <Link
                      key={`fav-${f.id}`}
                      to={`/conversation/${f.conversation_id}${f.message_id ? `?msg=${encodeURIComponent(f.message_id)}` : ""}`}
                      className="group flex flex-col rounded-2xl border border-amber-200 bg-amber-50/40 p-4 shadow-sm transition-all duration-150 hover:-translate-y-0.5 hover:border-amber-300 hover:shadow-md"
                    >
                      <div className="mb-2 flex items-center gap-2">
                        <Star size={12} className="text-amber-500" fill="currentColor" />
                        <span className="rounded-full border border-amber-200 bg-amber-100 px-2 py-0.5 text-[11px] font-medium text-amber-700">{t("common.favorite")}</span>
                        {f.tags.slice(0, 3).map((tag) => (
                          <span key={tag.id} className="rounded-full bg-white px-1.5 py-0.5 text-[10px] font-medium text-amber-700 border border-amber-200">
                            #{tag.name}
                          </span>
                        ))}
                      </div>
                      <h3 className="mb-1 line-clamp-1 text-sm font-semibold leading-snug text-stone-800 transition-colors group-hover:text-amber-700">
                        {f.conversation_title || t("common.untitledConversation")}
                      </h3>
                      <p className="line-clamp-2 text-xs leading-relaxed text-stone-600">{f.selected_text}</p>
                    </Link>
                  ))}
                </div>
              )}
            </>
          ) : (
            <>
              {loading && (
                <div className="flex items-center justify-center py-20">
                  <div className="flex flex-col items-center gap-3 text-stone-400">
                    <Loader2 size={24} className="animate-spin" />
                    <span className="text-sm">{t("conversations.loadingConversations")}</span>
                  </div>
                </div>
              )}

              {!loading && items.length === 0 && (
                <div className="flex flex-col items-center justify-center gap-4 py-24 text-stone-400">
                  <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-stone-100">
                    <Inbox size={28} strokeWidth={1.5} />
                  </div>
                  <div className="text-center">
                    <p className="font-medium text-stone-600">
                      {activeFilters.length > 0 ? t("conversations.noMatchingConversations") : t("conversations.noConversationsYet")}
                    </p>
                    {activeFilters.length === 0 && (
                      <p className="mt-1 text-sm text-stone-400">{t("conversations.importHint")}</p>
                    )}
                    {activeFilters.length > 0 && (
                      <button onClick={clearAll} className="mt-2 text-sm text-orange-500 hover:underline">
                        {t("conversations.clearFilters")}
                      </button>
                    )}
                  </div>
                </div>
              )}

              <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
                {items.map((c) => (
                  <Link
                    key={c.id}
                    to={`/conversation/${c.id}`}
                    className="group flex flex-col rounded-2xl border border-stone-200 bg-white p-4 shadow-sm transition-all duration-150 hover:-translate-y-0.5 hover:border-stone-300 hover:shadow-md"
                  >
                    <div className="mb-2.5 flex items-center gap-2">
                      <span className={`rounded-full border px-2 py-0.5 text-[11px] font-medium ${platformBadge[c.platform] ?? "bg-stone-100 text-stone-600 border-stone-200"}`}>
                        {platformLabel(c.platform)}
                      </span>
                      <span className="ml-auto text-xs text-stone-400">{formatRelativeDate(c.updated_at ?? c.created_at, lang, t)}</span>
                      <button
                        onClick={(e) => { e.preventDefault(); e.stopPropagation(); setPendingDelete(c); }}
                        title={t("conversations.deleteConversation")}
                        className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-stone-300 opacity-0 transition-colors hover:bg-red-50 hover:text-red-600 group-hover:opacity-100"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                    <h3 className="mb-1 line-clamp-2 text-sm font-semibold leading-snug text-stone-800 transition-colors group-hover:text-orange-700">
                      {c.title || t("common.untitledConversation")}
                    </h3>
                    {c.summary && (
                      <p className="mb-2 line-clamp-2 text-xs leading-relaxed text-stone-500">{c.summary}</p>
                    )}
                    <div className="mt-auto flex items-center gap-1 border-t border-stone-50 pt-2 text-xs text-stone-400">
                      <MessageSquare size={11} />
                      <span>{t("common.messageCount", { n: c.message_count })}</span>
                    </div>
                  </Link>
                ))}
              </div>

              {!loading && hasMore && (
                <div ref={sentinelRef} className="flex justify-center py-10 text-stone-400">
                  {loadingMore && <Loader2 size={18} className="animate-spin" />}
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {pendingDelete && (
        <ConfirmDialog
          title={t("common.deleteConversationConfirmTitle")}
          message={t("common.deleteConversationConfirmMessage", { n: pendingDelete.message_count })}
          confirmLabel={t("common.delete")}
          cancelLabel={t("common.cancel")}
          busy={deleting}
          onConfirm={confirmDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </div>
  );
}
