import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Star, Trash2, Sparkles, Tag as TagIcon, Search, X,
  MessageSquare, StickyNote, ExternalLink, ArrowUpRight,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, type FavoriteRow, type TagRow } from "../lib/api";
import { useToast } from "../lib/toast";
import { markdownUrlTransform } from "../lib/markdown";

// ── helpers ───────────────────────────────────────────────────────────────────

function formatDate(iso: string) {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const diff = Math.floor((Date.now() - d.getTime()) / 86400000);
  if (diff === 0) return "今天";
  if (diff === 1) return "昨天";
  if (diff < 7)  return `${diff} 天前`;
  if (diff < 365) return d.toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" });
  return d.toLocaleDateString("zh-CN", { year: "numeric", month: "numeric", day: "numeric" });
}

// ── Detail Modal ──────────────────────────────────────────────────────────────

function FavoriteModal({
  f,
  onClose,
  onRemove,
  onTagClick,
}: {
  f: FavoriteRow;
  onClose: () => void;
  onRemove: () => void;
  onTagClick: (id: number) => void;
}) {
  const navigate = useNavigate();
  const convUrl = f.message_id
    ? `/conversation/${f.conversation_id}?msg=${encodeURIComponent(f.message_id)}`
    : `/conversation/${f.conversation_id}`;

  const goToConv = () => { onClose(); navigate(convUrl); };

  // Close on Escape
  useEffect(() => {
    const fn = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", fn);
    return () => window.removeEventListener("keydown", fn);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4 backdrop-blur-sm"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="flex w-full max-w-2xl flex-col overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200"
           style={{ maxHeight: "85vh" }}>

        {/* ── Modal header ── */}
        <div className="flex items-start justify-between gap-3 border-b border-stone-100 px-6 py-4">
          <div className="min-w-0 flex-1">
            <div className="mb-1 flex items-center gap-2">
              <Star size={13} className="shrink-0 text-amber-400" fill="currentColor" />
              <span className="text-xs text-stone-400">{formatDate(f.created_at)}</span>
              {f.message_id && (
                <span className="rounded bg-amber-50 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 ring-1 ring-amber-200">
                  定位到消息
                </span>
              )}
            </div>
            <p className="truncate text-sm font-semibold text-stone-700">
              {f.conversation_title || "（无标题对话）"}
            </p>
          </div>
          <button
            onClick={onClose}
            className="mt-0.5 shrink-0 rounded-xl p-1.5 text-stone-400 transition-colors hover:bg-stone-100 hover:text-stone-600"
          >
            <X size={17} />
          </button>
        </div>

        {/* ── Scrollable body ── */}
        <div className="flex-1 overflow-y-auto px-6 py-5">
          {/* Quote text */}
          <div className="relative rounded-xl bg-amber-50 px-5 py-4 ring-1 ring-amber-200">
            <div className="absolute left-0 top-3 bottom-3 w-1 rounded-full bg-amber-400" />
            <div className="prose prose-sm prose-stone max-w-none pl-2">
              <ReactMarkdown remarkPlugins={[remarkGfm]} urlTransform={markdownUrlTransform}>
                {f.selected_text}
              </ReactMarkdown>
            </div>
          </div>

          {/* Note */}
          {f.note && (
            <div className="mt-4 flex items-start gap-2 rounded-xl bg-stone-50 px-4 py-3 ring-1 ring-stone-200">
              <StickyNote size={14} className="mt-0.5 shrink-0 text-stone-400" />
              <p className="text-sm leading-relaxed text-stone-600 italic">{f.note}</p>
            </div>
          )}

          {/* Tags */}
          {f.tags.length > 0 && (
            <div className="mt-4 flex flex-wrap gap-2">
              {f.tags.map((t) => (
                <button
                  key={t.id}
                  onClick={() => { onClose(); onTagClick(t.id); }}
                  className="rounded-full bg-amber-50 px-3 py-1 text-xs font-medium text-amber-700 ring-1 ring-amber-200 transition-colors hover:bg-amber-100"
                >
                  #{t.name}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* ── Footer actions ── */}
        <div className="flex items-center gap-2 border-t border-stone-100 bg-stone-50 px-6 py-3">
          <button
            onClick={() => { onRemove(); onClose(); }}
            className="flex items-center gap-1.5 rounded-xl border border-stone-200 px-3 py-2 text-sm text-stone-500 transition-colors hover:border-red-200 hover:bg-red-50 hover:text-red-600"
          >
            <Trash2 size={14} />
            移除收藏
          </button>
          <button
            onClick={goToConv}
            className="ml-auto flex items-center gap-1.5 rounded-xl bg-amber-500 px-4 py-2 text-sm font-medium text-white shadow-sm transition-opacity hover:opacity-90"
          >
            <MessageSquare size={14} />
            {f.message_id ? "跳到消息" : "查看对话"}
            <ArrowUpRight size={13} />
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Summary card (compact) ────────────────────────────────────────────────────

function FavoriteCard({
  f,
  onClick,
}: {
  f: FavoriteRow;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="group flex w-full flex-col items-start overflow-hidden rounded-2xl border border-stone-200 bg-white text-left shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md hover:ring-1 hover:ring-amber-300"
    >
      {/* Amber top bar */}
      <div className="h-1 w-full bg-gradient-to-r from-amber-400 to-orange-400" />

      <div className="flex w-full flex-col gap-2 p-4">
        {/* Header row */}
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1">
            <div className="mb-0.5 flex items-center gap-1.5">
              <Star size={10} className="shrink-0 text-amber-400" fill="currentColor" />
              <span className="text-[11px] text-stone-400">{formatDate(f.created_at)}</span>
            </div>
            <p className="truncate text-xs font-medium text-stone-500">
              {f.conversation_title || "（无标题对话）"}
            </p>
          </div>
          <ExternalLink size={12} className="mt-0.5 shrink-0 text-stone-300 transition-colors group-hover:text-amber-400" />
        </div>

        {/* Text preview */}
        <p className="line-clamp-3 whitespace-pre-wrap text-sm leading-relaxed text-stone-700">
          {f.selected_text}
        </p>

        {/* Tags */}
        {f.tags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {f.tags.map((t) => (
              <span
                key={t.id}
                className="rounded-full bg-amber-50 px-2 py-0.5 text-[10px] font-medium text-amber-700 ring-1 ring-amber-200"
              >
                #{t.name}
              </span>
            ))}
          </div>
        )}

        {f.note && (
          <p className="line-clamp-1 text-[11px] italic text-stone-400">
            备注：{f.note}
          </p>
        )}
      </div>
    </button>
  );
}

// ── Favorites page ────────────────────────────────────────────────────────────

export function Favorites() {
  const [favorites, setFavorites]   = useState<FavoriteRow[]>([]);
  const [tags, setTags]             = useState<TagRow[]>([]);
  const [activeTag, setActiveTag]   = useState<number | null>(null);
  const [query, setQuery]           = useState("");
  const [loading, setLoading]       = useState(true);
  const [organizing, setOrganizing] = useState(false);
  const [selected, setSelected]     = useState<FavoriteRow | null>(null);
  const { push } = useToast();

  const reload = () => {
    setLoading(true);
    Promise.all([api.listFavorites(activeTag), api.listTags()])
      .then(([f, t]) => { setFavorites(f); setTags(t); })
      .finally(() => setLoading(false));
  };

  useEffect(reload, [activeTag]);

  const remove = async (id: string) => {
    await api.deleteFavorite(id);
    push("已移除收藏", "success");
    reload();
  };

  const autoOrganize = async () => {
    setOrganizing(true);
    try {
      const count = await api.autoOrganizeFavorites();
      push(`已为 ${count} 条未分类收藏自动打标签`, "success");
      reload();
    } finally {
      setOrganizing(false);
    }
  };

  const q = query.trim().toLowerCase();
  const filtered = q
    ? favorites.filter(
        (f) =>
          f.selected_text.toLowerCase().includes(q) ||
          f.note.toLowerCase().includes(q) ||
          f.conversation_title.toLowerCase().includes(q),
      )
    : favorites;

  return (
    <div className="flex h-full flex-col bg-[#f7f5f2]">
      {/* ── Header ── */}
      <div className="border-b border-stone-200 bg-white px-6 py-4 shadow-sm">
        <div className="mb-3 flex items-center justify-between">
          <h1 className="flex items-center gap-2 text-lg font-semibold text-stone-800">
            <Star size={17} className="text-amber-500" fill="currentColor" />
            收藏夹
            {!loading && (
              <span className="text-sm font-normal text-stone-400">{favorites.length} 条</span>
            )}
          </h1>
          <button
            onClick={autoOrganize}
            disabled={organizing}
            className="flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-100 disabled:opacity-60"
          >
            <Sparkles size={14} />
            {organizing ? "整理中…" : "自动整理标签"}
          </button>
        </div>

        {/* Search */}
        <div className="relative mb-3">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-stone-400 pointer-events-none" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索收藏内容、备注、对话标题…"
            className="h-8 w-full rounded-lg border border-stone-200 bg-stone-50 pl-8 pr-8 text-sm outline-none transition-colors focus:border-amber-300 focus:bg-white focus:ring-2 focus:ring-amber-100"
          />
          {query && (
            <button onClick={() => setQuery("")} className="absolute right-2.5 top-1/2 -translate-y-1/2 text-stone-400 hover:text-stone-600">
              <X size={13} />
            </button>
          )}
        </div>

        {/* Tag filter */}
        <div className="flex flex-wrap gap-1.5">
          <button
            onClick={() => setActiveTag(null)}
            className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
              activeTag === null ? "bg-stone-900 text-white" : "bg-stone-100 text-stone-600 hover:bg-stone-200"
            }`}
          >
            全部
          </button>
          {tags.map((t) => (
            <button
              key={t.id}
              onClick={() => setActiveTag(activeTag === t.id ? null : t.id)}
              className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                activeTag === t.id ? "bg-stone-900 text-white" : "bg-stone-100 text-stone-600 hover:bg-stone-200"
              }`}
            >
              #{t.name}
            </button>
          ))}
        </div>
      </div>

      {/* ── Grid ── */}
      <div className="flex-1 overflow-y-auto px-6 py-5">
        {loading && <div className="py-16 text-center text-sm text-stone-400">加载中…</div>}

        {!loading && filtered.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-3 py-24 text-stone-400">
            <TagIcon size={32} />
            <p className="text-sm">
              {query
                ? `没有匹配"${query}"的收藏内容`
                : "还没有收藏内容。在对话中选中文字，点击「收藏」即可保存到这里。"}
            </p>
          </div>
        )}

        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {filtered.map((f) => (
            <FavoriteCard key={f.id} f={f} onClick={() => setSelected(f)} />
          ))}
        </div>
      </div>

      {/* ── Detail modal ── */}
      {selected && (
        <FavoriteModal
          f={selected}
          onClose={() => setSelected(null)}
          onRemove={() => remove(selected.id)}
          onTagClick={(id) => { setSelected(null); setActiveTag(id); }}
        />
      )}
    </div>
  );
}
