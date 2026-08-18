import { useEffect, useState } from "react";
import { X, History, Loader2, Trash2, Archive } from "lucide-react";
import { api, type ImportBatchRow } from "../lib/api";
import { ConfirmDialog } from "./ConfirmDialog";
import { useToast } from "../lib/toast";
import { useI18n, formatLongDate } from "../lib/i18n";
import { useRegion } from "../lib/region";
import { platformLabel } from "../lib/platforms";

function sourceLabel(sourceFile: string, agentScanLabel: string): string {
  if (sourceFile === "local-agent-scan") return agentScanLabel;
  const basename = sourceFile.split(/[/\\]/).pop();
  return basename || sourceFile;
}

export function ImportHistoryDialog({ onClose, onDeleted }: { onClose: () => void; onDeleted: () => void }) {
  const [loading, setLoading] = useState(true);
  const [batches, setBatches] = useState<ImportBatchRow[]>([]);
  const [pendingDelete, setPendingDelete] = useState<ImportBatchRow | null>(null);
  const [deleting, setDeleting] = useState(false);
  const { push } = useToast();
  const { t, lang } = useI18n();
  const { isChina } = useRegion();

  const reload = () => {
    setLoading(true);
    api.listImportBatches().then(setBatches).finally(() => setLoading(false));
  };

  useEffect(reload, []);

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setDeleting(true);
    try {
      const removed = await api.deleteImportBatch(pendingDelete.id);
      push(t("importHistory.deletedToast", { n: removed }), "success");
      setBatches((prev) => prev.filter((b) => b.id !== pendingDelete.id));
      setPendingDelete(null);
      onDeleted();
    } catch (e) {
      push(String(e), "error");
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div
        className="flex max-h-[80vh] w-[520px] flex-col overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-stone-100 text-stone-600">
              <History size={17} />
            </div>
            <h2 className="font-semibold text-stone-800">{t("importHistory.title")}</h2>
          </div>
          <button onClick={onClose} className="rounded-full p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600">
            <X size={17} />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-3 py-3">
          {loading && (
            <div className="flex flex-col items-center gap-3 py-10 text-stone-400">
              <Loader2 size={22} className="animate-spin" />
              <p className="text-sm">{t("importHistory.loading")}</p>
            </div>
          )}

          {!loading && batches.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-10 text-stone-400">
              <Archive size={28} strokeWidth={1.5} />
              <p className="text-sm">{t("importHistory.empty")}</p>
            </div>
          )}

          {!loading &&
            batches.map((b) => (
              <div
                key={b.id}
                className="flex items-center gap-3 rounded-xl px-3 py-2.5 transition-colors hover:bg-stone-50"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="rounded-full border border-stone-200 bg-stone-50 px-2 py-0.5 text-[10px] font-medium text-stone-600">
                      {platformLabel(b.platform, isChina)}
                    </span>
                    <span className="text-xs text-stone-400">{formatLongDate(b.imported_at, lang)}</span>
                  </div>
                  <p className="mt-1 truncate text-sm text-stone-700" title={b.source_file}>
                    {sourceLabel(b.source_file, t("importHistory.agentScanSource"))}
                  </p>
                  <p className="mt-0.5 text-xs text-stone-400">
                    {t("importHistory.conversationsCount", { n: b.remaining_conversations })}
                    {b.remaining_conversations !== b.added_count + b.updated_count && (
                      <span className="text-stone-300"> · {t("importHistory.originallyCount", { n: b.added_count + b.updated_count })}</span>
                    )}
                  </p>
                </div>
                <button
                  onClick={() => setPendingDelete(b)}
                  disabled={b.remaining_conversations === 0}
                  title={t("importHistory.deleteButton")}
                  className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-stone-400 transition-colors hover:bg-red-50 hover:text-red-600 disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-stone-400"
                >
                  <Trash2 size={15} />
                </button>
              </div>
            ))}
        </div>
      </div>

      {pendingDelete && (
        <ConfirmDialog
          title={t("importHistory.deleteConfirmTitle")}
          message={t("importHistory.deleteConfirmMessage", {
            file: sourceLabel(pendingDelete.source_file, t("importHistory.agentScanSource")),
            n: pendingDelete.remaining_conversations,
          })}
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
