import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { X, UploadCloud, CheckCircle2, FileArchive } from "lucide-react";
import { api, type ImportSummary, type ImportProgress as Progress } from "../lib/api";
import { useToast } from "../lib/toast";
import { useI18n } from "../lib/i18n";
import { useRegion } from "../lib/region";
import { platformLabel } from "../lib/platforms";

interface Props {
  onClose: () => void;
  onImported: () => void;
  preloadedPath?: string | null;
}

export function ImportDialog({ onClose, onImported, preloadedPath }: Props) {
  const [busy, setBusy]         = useState(false);
  const [summary, setSummary]   = useState<ImportSummary | null>(null);
  const [fileName, setFileName] = useState<string | null>(
    preloadedPath ? preloadedPath.split(/[\\/]/).pop() ?? null : null,
  );
  const [progress, setProgress] = useState<Progress | null>(null);
  const { push } = useToast();
  const { t } = useI18n();
  const { isChina } = useRegion();

  const runImport = async (path: string) => {
    setBusy(true);
    setSummary(null);
    setProgress(null);
    setFileName(path.split(/[\\/]/).pop() ?? path);
    try {
      const result = await api.importZip(path, (p) => setProgress(p));
      setSummary(result);
      onImported();
    } catch (e) {
      push(t("importDialog.importFailedToast", { error: String(e) }), "error");
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (preloadedPath && !busy && !summary) runImport(preloadedPath);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [preloadedPath]);

  const pickAndImport = async () => {
    const file = await open({
      multiple: false,
      filters: [{ name: t("importDialog.filePickerFilterName"), extensions: ["zip"] }],
      title: t(isChina ? "importDialog.filePickerTitleChina" : "importDialog.filePickerTitle"),
    });
    if (!file) return;
    await runImport(file as string);
  };

  const isDbPhase = progress?.phase === "db";
  const pct = isDbPhase && progress!.total > 0
    ? Math.round((progress!.current / progress!.total) * 100)
    : null;

  const platformLabelText = platformLabel(summary?.platform ?? "", isChina);

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-sm">
      <div className="w-[480px] overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200">

        {/* Header */}
        <div className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-orange-100 text-orange-600">
              <UploadCloud size={17} />
            </div>
            <div>
              <h2 className="font-semibold text-stone-800">{t("importDialog.title")}</h2>
              <p className="text-xs text-stone-400">{isChina ? "Claude · DeepSeek" : "Claude · ChatGPT · DeepSeek"}</p>
            </div>
          </div>
          {!busy && (
            <button
              onClick={onClose}
              className="rounded-full p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600"
            >
              <X size={17} />
            </button>
          )}
        </div>

        <div className="px-6 py-5">
          {/* ── Idle: file picker ── */}
          {!busy && !summary && (
            <>
              <button
                onClick={pickAndImport}
                className="group flex w-full flex-col items-center gap-3 rounded-2xl border-2 border-dashed border-stone-200 px-6 py-10 transition-all hover:border-orange-300 hover:bg-orange-50"
              >
                <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-stone-100 text-stone-400 transition-colors group-hover:bg-orange-100 group-hover:text-orange-600">
                  <FileArchive size={24} />
                </div>
                <div className="text-center">
                  <p className="font-medium text-stone-700 group-hover:text-orange-700">{t("importDialog.clickToSelect")}</p>
                  <p className="mt-0.5 text-sm text-stone-400">{t("importDialog.orDrag")}</p>
                </div>
              </button>
              <p className="mt-3 text-center text-xs text-stone-400">
                {t(isChina ? "importDialog.autoDetectHintChina" : "importDialog.autoDetectHint")}
              </p>
            </>
          )}

          {/* ── Importing: progress ── */}
          {busy && (
            <div className="flex flex-col gap-5 py-2">
              {/* File name */}
              {fileName && (
                <div className="flex items-center gap-2 rounded-xl bg-stone-50 px-3 py-2.5 ring-1 ring-stone-200">
                  <FileArchive size={14} className="shrink-0 text-stone-400" />
                  <span className="truncate font-mono text-xs text-stone-500">{fileName}</span>
                </div>
              )}

              {/* Phase label + count */}
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <div className="h-2 w-2 animate-pulse rounded-full bg-orange-400" />
                  <span className="text-sm font-medium text-stone-700">
                    {!progress
                      ? t("importDialog.preparing")
                      : progress.phase === "parse"
                      ? t("importDialog.parsing")
                      : t("importDialog.writingDb")}
                  </span>
                </div>
                {isDbPhase && progress!.total > 0 && (
                  <span className="tabular-nums text-sm text-stone-500">
                    {t("importDialog.dbCount", { current: progress!.current.toLocaleString(), total: progress!.total.toLocaleString() })}
                  </span>
                )}
              </div>

              {/* Progress bar */}
              <div className="space-y-1.5">
                <div className="h-3 w-full overflow-hidden rounded-full bg-stone-100">
                  {pct !== null ? (
                    <div
                      className="h-full rounded-full bg-gradient-to-r from-orange-400 to-rose-500 transition-all duration-200"
                      style={{ width: `${pct}%` }}
                    />
                  ) : (
                    /* Indeterminate shimmer during parse phase */
                    <div className="relative h-full w-full overflow-hidden rounded-full">
                      <div className="absolute inset-y-0 w-1/2 animate-[shimmer_1.4s_ease-in-out_infinite] rounded-full bg-gradient-to-r from-transparent via-orange-300 to-transparent" />
                    </div>
                  )}
                </div>
                <div className="flex justify-between text-xs text-stone-400">
                  <span>
                    {pct !== null
                      ? `${pct}%`
                      : t("importDialog.pleaseWait")}
                  </span>
                  {pct !== null && (
                    <span>{pct === 100 ? t("importDialog.almostDone") : t("importDialog.importing")}</span>
                  )}
                </div>
              </div>
            </div>
          )}

          {/* ── Success ── */}
          {summary && (
            <div className="flex flex-col gap-4">
              <div className="flex flex-col items-center gap-2 py-2">
                <CheckCircle2 size={36} className="text-emerald-500" />
                <p className="font-semibold text-stone-800">{t("importDialog.importComplete")}</p>
                {fileName && (
                  <p className="truncate text-xs font-mono text-stone-400 max-w-full px-4">{fileName}</p>
                )}
              </div>

              <div className="rounded-2xl bg-stone-50 p-4">
                <div className="mb-3 flex items-center gap-2">
                  <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium ${
                    summary.platform === "claude"    ? "bg-orange-100 text-orange-700"
                    : summary.platform === "deepseek" ? "bg-blue-100 text-blue-700"
                    : "bg-emerald-100 text-emerald-700"
                  }`}>
                    {platformLabelText}
                  </span>
                  <span className="text-sm text-stone-500">
                    {t("importDialog.totalInFile", { n: summary.total_in_file.toLocaleString() })}
                  </span>
                </div>
                <div className="grid grid-cols-3 gap-3 text-center">
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-emerald-600">{summary.added.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">{t("importDialog.added")}</p>
                  </div>
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-amber-500">{summary.updated.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">{t("importDialog.updated")}</p>
                  </div>
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-stone-400">{summary.skipped.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">{t("importDialog.skipped")}</p>
                  </div>
                </div>
              </div>

              <button
                onClick={onClose}
                className="w-full rounded-xl bg-stone-900 py-2.5 text-sm font-medium text-white transition-colors hover:bg-stone-800"
              >
                {t("importDialog.done")}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
