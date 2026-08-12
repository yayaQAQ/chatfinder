import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { X, FolderOpen, Loader2, MessageSquare } from "lucide-react";
import { api, type ConversationSummary } from "../lib/api";
import { useI18n, formatRelativeDate } from "../lib/i18n";

const platformLabel: Record<string, string> = {
  claude: "Claude",
  chatgpt: "ChatGPT",
  deepseek: "DeepSeek",
  "claude-code": "Claude Code",
  codex: "Codex",
};
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
  const navigate = useNavigate();
  const { t, lang } = useI18n();

  useEffect(() => {
    api.listConversationsByPath(path).then(setItems).finally(() => setLoading(false));
  }, [path]);

  const goTo = (id: string) => {
    onClose();
    navigate(`/conversation/${id}`);
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

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-3 py-3">
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
                  {(platformLabel[c.platform] ?? c.platform)[0]}
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-stone-700">
                    {c.title || t("common.untitledConversation")}
                  </p>
                  <div className="flex items-center gap-1.5 text-xs text-stone-400">
                    <span>{platformLabel[c.platform] ?? c.platform}</span>
                    <span>·</span>
                    <MessageSquare size={10} />
                    <span>{t("common.messageCount", { n: c.message_count })}</span>
                    <span>·</span>
                    <span>{formatRelativeDate(c.updated_at ?? c.created_at, lang, t)}</span>
                  </div>
                </div>
              </button>
            ))}
        </div>
      </div>
    </div>
  );
}
