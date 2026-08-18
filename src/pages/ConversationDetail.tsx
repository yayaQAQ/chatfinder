import { useEffect, useMemo, useRef, useState } from "react";
import { writeText as clipboardWrite } from "@tauri-apps/plugin-clipboard-manager";
import { useParams, useSearchParams, useNavigate } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft, ExternalLink, Star, MessageSquare, Copy, Check, PanelRightClose, PanelRight,
  Search, ChevronUp, ChevronDown, X, Trash2, FolderOpen,
} from "lucide-react";
import { api, type ConversationSummary, type MessageRow } from "../lib/api";
import { FavoriteModal } from "../components/FavoriteModal";
import { MessageBubble } from "../components/MessageBubble";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PathConversationsDialog } from "../components/PathConversationsDialog";
import { selectionToMarkdown } from "../lib/markdown";
import { useI18n, formatLongDate } from "../lib/i18n";
import { useToast } from "../lib/toast";
import { useRegion } from "../lib/region";
import { platformLabel } from "../lib/platforms";

function stripForPreview(text: string, imagePlaceholder: string): string {
  return text
    .replace(/!\[.*?\]\(<data:image\/[^>]*>\)/g, imagePlaceholder)
    .replace(/!\[.*?\]\(data:image\/[^)]*\)/g, imagePlaceholder)
    .replace(/!\[.*?\]\(.*?\)/g, imagePlaceholder)
    .replace(/\[(.+?)\]\(.*?\)/g, "$1")
    .replace(/```[\s\S]*?```/g, "")
    .replace(/`(.+?)`/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/[*_~]/g, "")
    .replace(/\n+/g, " ")
    .trim();
}

export function ConversationDetail({ onDataChanged }: { onDataChanged?: () => void } = {}) {
  const { t, lang } = useI18n();
  const { isChina } = useRegion();
  const { push } = useToast();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const targetMsgId = searchParams.get("msg");
  const [conv, setConv] = useState<ConversationSummary | null>(null);
  const [messages, setMessages] = useState<MessageRow[]>([]);
  const [loading, setLoading]   = useState(true);
  const [idCopied, setIdCopied] = useState(false);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [pathDialogOpen, setPathDialogOpen] = useState(false);
  const [highlightedId, setHighlightedId] = useState<string | null>(null);
  const [selection, setSelection] = useState<{ text: string; messageId: string | null } | null>(null);
  const [favoriteTarget, setFavoriteTarget] = useState<{ text: string; messageId: string | null } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [resuming, setResuming] = useState(false);
  const [panelOpen, setPanelOpen] = useState(true);
  const humanMessages = messages.filter((m) => m.sender === "human");

  // In-conversation search: client-side, scoped to the messages already
  // loaded for this conversation — not the global keyword/semantic search.
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchIndex, setSearchIndex] = useState(0);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const searchMatchIds = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return [];
    return messages.filter((m) => m.text.toLowerCase().includes(q)).map((m) => m.id);
  }, [messages, searchQuery]);
  // Only messages that actually match get a highlightQuery prop, so
  // MessageBubble's memo skips re-rendering (and re-highlighting) the rest
  // of a long conversation on every keystroke.
  const searchMatchIdSet = useMemo(() => new Set(searchMatchIds), [searchMatchIds]);

  const gotoSearchMatch = (index: number) => {
    if (searchMatchIds.length === 0) return;
    const wrapped = ((index % searchMatchIds.length) + searchMatchIds.length) % searchMatchIds.length;
    setSearchIndex(wrapped);
    const msgId = searchMatchIds[wrapped];
    setHighlightedId(msgId);
    document.querySelector(`[data-message-id="${msgId}"]`)?.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  // Jump to the first match whenever the query (or its result set) changes.
  useEffect(() => {
    if (!searchOpen) return;
    if (searchMatchIds.length > 0) {
      setSearchIndex(0);
      const msgId = searchMatchIds[0];
      setHighlightedId(msgId);
      document.querySelector(`[data-message-id="${msgId}"]`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    } else {
      setHighlightedId(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchMatchIds, searchOpen]);

  useEffect(() => {
    if (searchOpen) searchInputRef.current?.focus();
  }, [searchOpen]);

  const closeSearch = () => {
    setSearchOpen(false);
    setSearchQuery("");
    setSearchIndex(0);
    setHighlightedId(null);
  };

  const resumeCommand = (() => {
    if (!conv) return null;
    if (conv.platform === "claude-code") {
      const sessionId = conv.id.startsWith("cc:") ? conv.id.slice(3) : conv.id;
      return { cmd: `claude --resume ${sessionId}`, cwd: conv.summary || undefined };
    }
    if (conv.platform === "codex") {
      const sessionId = conv.id.startsWith("codex:") ? conv.id.slice(6) : conv.id;
      return { cmd: `codex resume ${sessionId}`, cwd: conv.summary || undefined };
    }
    return null;
  })();

  const handleResume = async () => {
    if (!resumeCommand) return;
    setResuming(true);
    try {
      await api.launchResumeTerminal(resumeCommand.cmd, resumeCommand.cwd);
      push(t("conversationDetail.resumeOpened"), "success");
    } catch {
      // Sandbox blocks process spawning — fall back to clipboard so the user
      // can paste the command in their own terminal.
      try {
        await clipboardWrite(resumeCommand.cmd);
        push(t("conversationDetail.resumeCopied"), "info");
      } catch {
        push(resumeCommand.cmd, "info");
      }
    } finally {
      setResuming(false);
    }
  };

  const deleteConversation = async () => {
    if (!id) return;
    setDeleting(true);
    try {
      await api.deleteConversation(id);
      push(t("common.deleteConversationToast"), "success");
      onDataChanged?.();
      navigate("/");
    } catch (e) {
      push(String(e), "error");
    } finally {
      setDeleting(false);
      setDeleteConfirmOpen(false);
    }
  };

  const jumpToMessage = (msgId: string) => {
    setHighlightedId(msgId);
    const el = document.querySelector(`[data-message-id="${msgId}"]`);
    el?.scrollIntoView({ behavior: "smooth", block: "center" });
    setTimeout(() => setHighlightedId(null), 2500);
  };

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
    const plainText = sel?.toString().trim();
    if (!plainText || !containerRef.current) return;
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
    // Reconstruct markdown from the selected DOM range so bold/code/lists/
    // links survive into the favorite instead of being flattened to plain text.
    const text = selectionToMarkdown(sel.getRangeAt(0)) || plainText;
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

  const platformLabelText = platformLabel(conv.platform, isChina);
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
          {platformLabelText[0]}
        </div>
        <div className="flex-1 min-w-0">
          <h1 className="truncate font-semibold text-stone-800">{conv.title || t("common.untitledConversation")}</h1>
          <div className="flex items-center gap-2 flex-wrap">
            <p className="text-xs text-stone-400">
              {platformLabelText} · {t("common.messageCount", { n: conv.message_count })}
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
          {conv.summary && (
            <button
              onClick={() => setPathDialogOpen(true)}
              title={t("conversationDetail.viewPathConversations")}
              className="mt-0.5 flex max-w-full items-center gap-1 rounded-md px-0.5 font-mono text-[11px] text-stone-400 transition-colors hover:text-orange-600"
            >
              <FolderOpen size={11} className="shrink-0" />
              <span className="truncate">{conv.summary}</span>
            </button>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setSearchOpen((v) => !v)}
            title={t("conversationDetail.searchInConversation")}
            className={`flex h-8 w-8 items-center justify-center rounded-xl transition-colors ${
              searchOpen ? "bg-amber-100 text-amber-600" : "text-stone-400 hover:bg-stone-100 hover:text-stone-700"
            }`}
          >
            <Search size={15} />
          </button>
          {resumeCommand && (
            <button
              onClick={handleResume}
              disabled={resuming}
              title={resumeCommand.cmd}
              className="flex items-center gap-1.5 rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-600 transition-colors hover:bg-stone-50 hover:text-stone-800 disabled:opacity-50"
            >
              <ExternalLink size={13} />
              {resuming ? t("conversationDetail.resumeLaunching") : t("conversationDetail.resumeConversation")}
            </button>
          )}
          {conv.url && !resumeCommand && (
            <button
              onClick={() => openUrl(conv.url!)}
              className="flex items-center gap-1.5 rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-600 transition-colors hover:bg-stone-50 hover:text-stone-800"
            >
              <ExternalLink size={13} />
              {t("conversationDetail.continueConversation")}
            </button>
          )}
          <button
            onClick={() => setDeleteConfirmOpen(true)}
            title={t("conversationDetail.deleteConversation")}
            className="flex h-8 w-8 items-center justify-center rounded-xl text-stone-400 transition-colors hover:bg-red-50 hover:text-red-600"
          >
            <Trash2 size={15} />
          </button>
        </div>
      </div>

      {/* In-conversation search bar */}
      {searchOpen && (
        <div className="flex items-center gap-2 border-b border-stone-200 bg-white px-5 py-2.5">
          <Search size={14} className="shrink-0 text-stone-400" />
          <input
            ref={searchInputRef}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                gotoSearchMatch(e.shiftKey ? searchIndex - 1 : searchIndex + 1);
              } else if (e.key === "Escape") {
                closeSearch();
              }
            }}
            placeholder={t("conversationDetail.searchInConversationPlaceholder")}
            className="h-8 flex-1 rounded-lg border border-stone-200 bg-stone-50 px-3 text-sm outline-none transition-colors focus:border-amber-300 focus:bg-white focus:ring-2 focus:ring-amber-100"
          />
          <span className="shrink-0 whitespace-nowrap text-xs tabular-nums text-stone-400">
            {searchQuery.trim()
              ? searchMatchIds.length > 0
                ? t("conversationDetail.searchMatchCount", { i: searchIndex + 1, n: searchMatchIds.length })
                : t("conversationDetail.searchNoMatches")
              : ""}
          </span>
          <button
            onClick={() => gotoSearchMatch(searchIndex - 1)}
            disabled={searchMatchIds.length === 0}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-stone-400 transition-colors hover:bg-stone-100 hover:text-stone-700 disabled:opacity-30 disabled:hover:bg-transparent"
          >
            <ChevronUp size={15} />
          </button>
          <button
            onClick={() => gotoSearchMatch(searchIndex + 1)}
            disabled={searchMatchIds.length === 0}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-stone-400 transition-colors hover:bg-stone-100 hover:text-stone-700 disabled:opacity-30 disabled:hover:bg-transparent"
          >
            <ChevronDown size={15} />
          </button>
          <button
            onClick={closeSearch}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-stone-400 transition-colors hover:bg-stone-100 hover:text-stone-700"
          >
            <X size={15} />
          </button>
        </div>
      )}

      {/* Body: messages + right panel */}
      <div className="flex flex-1 overflow-hidden">
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
                  highlightQuery={searchMatchIdSet.has(m.id) ? searchQuery.trim() : undefined}
                />
              ))}
            </div>
          </div>
        </div>

        {/* Right panel: user inputs */}
        {humanMessages.length > 0 && (
          <div className={`flex shrink-0 flex-col border-l border-stone-200 bg-white transition-all duration-200 ${panelOpen ? "w-52" : "w-8"}`}>
            {/* Panel header / toggle */}
            <div className="flex items-center justify-between border-b border-stone-100 px-2 py-2.5">
              {panelOpen && (
                <p className="ml-1 text-[11px] font-semibold uppercase tracking-wide text-stone-400">
                  {t("conversationDetail.userInputs")}
                </p>
              )}
              <button
                onClick={() => setPanelOpen((v) => !v)}
                title={panelOpen ? t("conversationDetail.collapsePanel") : t("conversationDetail.expandPanel")}
                className="flex h-5 w-5 items-center justify-center rounded text-stone-300 hover:bg-stone-100 hover:text-stone-500 transition-colors"
              >
                {panelOpen ? <PanelRightClose size={13} /> : <PanelRight size={13} />}
              </button>
            </div>

            {/* Message list — hidden when collapsed */}
            {panelOpen && (
            <div className="flex-1 overflow-y-auto py-1">
              {humanMessages.map((msg, idx) => {
                const preview = stripForPreview(msg.text, t("conversationDetail.imagePlaceholder"));
                const isActive = highlightedId === msg.id;
                return (
                  <button
                    key={msg.id}
                    onClick={() => jumpToMessage(msg.id)}
                    className={`flex w-full items-start gap-2 px-3 py-2.5 text-left transition-colors hover:bg-stone-50 ${
                      isActive ? "bg-amber-50" : ""
                    }`}
                  >
                    <span className="mt-0.5 shrink-0 font-mono text-[10px] text-stone-300">
                      #{idx + 1}
                    </span>
                    <p className="line-clamp-3 text-[11px] leading-relaxed text-stone-600">
                      {preview || t("conversationDetail.imagePlaceholder")}
                    </p>
                  </button>
                );
              })}
            </div>
            )}
          </div>
        )}
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

      {deleteConfirmOpen && (
        <ConfirmDialog
          title={t("common.deleteConversationConfirmTitle")}
          message={t("common.deleteConversationConfirmMessage", { n: conv.message_count })}
          confirmLabel={t("common.delete")}
          cancelLabel={t("common.cancel")}
          busy={deleting}
          onConfirm={deleteConversation}
          onCancel={() => setDeleteConfirmOpen(false)}
        />
      )}

      {pathDialogOpen && conv.summary && (
        <PathConversationsDialog
          path={conv.summary}
          currentId={conv.id}
          onClose={() => setPathDialogOpen(false)}
        />
      )}
    </div>
  );
}
