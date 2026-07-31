import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { X, FolderSearch, FolderOpen, CheckCircle2, Bot, Loader2, RefreshCw } from "lucide-react";
import { api, type AgentSource, type AgentDirOverrides, type ImportSummary, type ImportProgress as Progress } from "../lib/api";
import { useToast } from "../lib/toast";
import { useI18n } from "../lib/i18n";

interface Props {
  onClose: () => void;
  onImported: () => void;
}

const toolIconColor: Record<string, string> = {
  "claude-code": "bg-orange-100 text-orange-600",
  codex: "bg-sky-100 text-sky-600",
};

const TOOL_IDS = ["claude-code", "codex"];
const settingKey = (tool: string) => `agent_dir:${tool}`;

const TOOL_DEFAULT_DIRS: Record<string, string> = {
  "claude-code": "~/.claude/projects",
  "codex": "~/.codex/sessions",
};

export function AgentScanDialog({ onClose, onImported }: Props) {
  const [scanning, setScanning] = useState(true);
  const [sources, setSources] = useState<AgentSource[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const [overrides, setOverrides] = useState<AgentDirOverrides>({});
  const [pickingTool, setPickingTool] = useState<string | null>(null);
  const { push } = useToast();
  const { t } = useI18n();

  const scan = async (withOverrides: AgentDirOverrides = overrides) => {
    setScanning(true);
    setSummary(null);
    try {
      const found = await api.scanAgentSources(withOverrides);
      setSources(found);
      setSelected(new Set(found.filter((s) => s.session_count > 0).map((s) => s.tool)));
    } catch (e) {
      push(t("agentScanDialog.scanFailedToast", { error: String(e) }), "error");
    } finally {
      setScanning(false);
    }
  };

  // Load persisted directory overrides first, then scan with them.
  useEffect(() => {
    const init = async () => {
      const saved: AgentDirOverrides = {};
      for (const tool of TOOL_IDS) {
        const val = await api.getSetting(settingKey(tool));
        if (val) saved[tool] = val;
      }
      setOverrides(saved);
      await scan(saved);
    };
    init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (tool: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(tool)) next.delete(tool);
      else next.add(tool);
      return next;
    });
  };

  const pickFolder = async (source: AgentSource) => {
    setPickingTool(source.tool);
    try {
      const dir = await open({ directory: true, multiple: false, title: t("agentScanDialog.chooseFolderTitle", { label: source.label }), defaultPath: source.dir ?? undefined });
      if (!dir || typeof dir !== "string") return;
      await api.setSetting(settingKey(source.tool), dir);
      const nextOverrides = { ...overrides, [source.tool]: dir };
      setOverrides(nextOverrides);
      await scan(nextOverrides);
    } finally {
      setPickingTool(null);
    }
  };

  const clearOverride = async (tool: string) => {
    await api.setSetting(settingKey(tool), "");
    const nextOverrides = { ...overrides };
    delete nextOverrides[tool];
    setOverrides(nextOverrides);
    await scan(nextOverrides);
  };

  const totalSessions = sources
    .filter((s) => selected.has(s.tool))
    .reduce((n, s) => n + s.session_count, 0);

  const runImport = async () => {
    if (selected.size === 0) return;
    setBusy(true);
    setProgress(null);
    try {
      const result = await api.importAgentSessions(
        [...selected],
        (p) => setProgress(p),
        overrides,
      );
      setSummary(result);
      onImported();
    } catch (e) {
      push(t("agentScanDialog.indexFailedToast", { error: String(e) }), "error");
    } finally {
      setBusy(false);
    }
  };

  const isDbPhase = progress?.phase === "db";
  const pct =
    isDbPhase && progress!.total > 0
      ? Math.round((progress!.current / progress!.total) * 100)
      : null;

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-sm">
      <div className="w-[480px] overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-violet-100 text-violet-600">
              <FolderSearch size={17} />
            </div>
            <div>
              <h2 className="font-semibold text-stone-800">{t("agentScanDialog.title")}</h2>
              <p className="text-xs text-stone-400">Claude Code · Codex</p>
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
          {/* ── Scanning ── */}
          {scanning && (
            <div className="flex flex-col items-center gap-3 py-10 text-stone-500">
              <Loader2 size={26} className="animate-spin text-violet-500" />
              <p className="text-sm">{t("agentScanDialog.scanning")}</p>
            </div>
          )}

          {/* ── Found: prompt to index ── */}
          {!scanning && !summary && !busy && (
            <>
              <div className="mb-3 flex items-center justify-between">
                <p className="text-sm text-stone-600">
                  {t("agentScanDialog.foundPrompt")}
                </p>
                <button
                  onClick={() => scan()}
                  title={t("agentScanDialog.rescanAll")}
                  className="shrink-0 rounded-lg p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600"
                >
                  <RefreshCw size={13} />
                </button>
              </div>
              <div className="space-y-2">
                {sources.map((s) => {
                  const on = selected.has(s.tool);
                  const hasSessions = s.session_count > 0;
                  const hasOverride = Boolean(overrides[s.tool]);

                  if (!hasSessions) {
                    return (
                      <div
                        key={s.tool}
                        className="flex w-full items-center gap-3 rounded-xl border border-dashed border-stone-200 bg-stone-50 px-3 py-3"
                      >
                        <div
                          className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-lg ${
                            toolIconColor[s.tool] ?? "bg-stone-100 text-stone-500"
                          }`}
                        >
                          <Bot size={18} />
                        </div>
                        <div className="min-w-0 flex-1">
                          <p className="font-medium text-stone-800">{s.label}</p>
                          <p className="truncate text-[11px] text-stone-400">
                            {s.accessible ? t("agentScanDialog.dirEmpty") : t("agentScanDialog.dirNotAccessible")}
                          </p>
                          <p className="truncate font-mono text-[10px] text-stone-300">
                            {TOOL_DEFAULT_DIRS[s.tool] ?? s.dir}
                          </p>
                        </div>
                        <div className="flex shrink-0 flex-col items-end gap-1">
                          <div className="flex items-center gap-1">
                            {hasOverride && (
                              <button
                                onClick={() => clearOverride(s.tool)}
                                title={t("agentScanDialog.clearFolder")}
                                className="flex items-center justify-center rounded-lg p-1.5 text-stone-400 hover:bg-stone-200 hover:text-stone-600"
                              >
                                <X size={12} />
                              </button>
                            )}
                            <button
                              onClick={() => pickFolder(s)}
                              disabled={pickingTool === s.tool}
                              className="flex shrink-0 items-center gap-1.5 rounded-lg border border-stone-200 bg-white px-2.5 py-1.5 text-xs font-medium text-stone-600 hover:bg-stone-100 disabled:opacity-50"
                            >
                              {pickingTool === s.tool ? (
                                <Loader2 size={12} className="animate-spin" />
                              ) : (
                                <FolderOpen size={12} />
                              )}
                              {t("agentScanDialog.chooseFolder")}
                            </button>
                          </div>
                          {!hasOverride && (
                            <p className="text-[10px] text-stone-300">{t("agentScanDialog.hiddenFolderHint")}</p>
                          )}
                        </div>
                      </div>
                    );
                  }

                  return (
                    <button
                      key={s.tool}
                      onClick={() => toggle(s.tool)}
                      className={`flex w-full items-center gap-3 rounded-xl border px-3 py-3 text-left transition-colors ${
                        on
                          ? "border-violet-300 bg-violet-50"
                          : "border-stone-200 bg-white hover:bg-stone-50"
                      }`}
                    >
                      <div
                        className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-lg ${
                          toolIconColor[s.tool] ?? "bg-stone-100 text-stone-500"
                        }`}
                      >
                        <Bot size={18} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <p className="font-medium text-stone-800">{s.label}</p>
                        <p className="truncate font-mono text-[11px] text-stone-400">{s.dir}</p>
                      </div>
                      <div className="shrink-0 text-right">
                        <p className="text-sm font-semibold tabular-nums text-stone-700">
                          {t("agentScanDialog.sessionsCount", { n: s.session_count.toLocaleString() })}
                        </p>
                        <p className="text-[11px] text-stone-400">
                          {t("agentScanDialog.messagesCount", { n: s.message_count.toLocaleString() })}
                        </p>
                      </div>
                      {/* Change-folder controls — stopPropagation so toggle doesn't fire */}
                      <div
                        className="flex shrink-0 items-center gap-0.5"
                        onClick={(e) => e.stopPropagation()}
                      >
                        {hasOverride && (
                          <button
                            onClick={() => clearOverride(s.tool)}
                            title={t("agentScanDialog.clearFolder")}
                            className="flex items-center justify-center rounded-lg p-1.5 text-stone-400 hover:bg-stone-200 hover:text-stone-600"
                          >
                            <X size={11} />
                          </button>
                        )}
                        <button
                          onClick={() => pickFolder(s)}
                          disabled={pickingTool === s.tool}
                          title={t("agentScanDialog.changeFolder")}
                          className="flex items-center justify-center rounded-lg p-1.5 text-stone-400 hover:bg-stone-200 hover:text-stone-600 disabled:opacity-50"
                        >
                          {pickingTool === s.tool ? (
                            <Loader2 size={13} className="animate-spin" />
                          ) : (
                            <FolderOpen size={13} />
                          )}
                        </button>
                      </div>
                      <div
                        className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md border ${
                          on ? "border-violet-500 bg-violet-500 text-white" : "border-stone-300"
                        }`}
                      >
                        {on && <CheckCircle2 size={14} />}
                      </div>
                    </button>
                  );
                })}
              </div>

              <button
                disabled={selected.size === 0}
                onClick={runImport}
                className="mt-4 w-full rounded-xl bg-gradient-to-r from-violet-500 to-fuchsia-500 py-2.5 text-sm font-medium text-white shadow-sm transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {totalSessions > 0 ? t("agentScanDialog.indexImportWithCount", { n: totalSessions.toLocaleString() }) : t("agentScanDialog.indexImport")}
              </button>
              <p className="mt-2 text-center text-[11px] text-stone-400">
                {t("agentScanDialog.readOnlyHint")}
              </p>
            </>
          )}

          {/* ── Importing ── */}
          {busy && (
            <div className="flex flex-col gap-5 py-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <div className="h-2 w-2 animate-pulse rounded-full bg-violet-400" />
                  <span className="text-sm font-medium text-stone-700">
                    {!progress
                      ? t("agentScanDialog.preparing")
                      : progress.phase === "parse"
                      ? t("agentScanDialog.parsingSessions")
                      : t("agentScanDialog.writingDb")}
                  </span>
                </div>
                {isDbPhase && progress!.total > 0 && (
                  <span className="tabular-nums text-sm text-stone-500">
                    {t("agentScanDialog.dbCount", { current: progress!.current.toLocaleString(), total: progress!.total.toLocaleString() })}
                  </span>
                )}
              </div>
              <div className="space-y-1.5">
                <div className="h-3 w-full overflow-hidden rounded-full bg-stone-100">
                  {pct !== null ? (
                    <div
                      className="h-full rounded-full bg-gradient-to-r from-violet-400 to-fuchsia-500 transition-all duration-200"
                      style={{ width: `${pct}%` }}
                    />
                  ) : (
                    <div className="relative h-full w-full overflow-hidden rounded-full">
                      <div className="absolute inset-y-0 w-1/2 animate-[shimmer_1.4s_ease-in-out_infinite] rounded-full bg-gradient-to-r from-transparent via-violet-300 to-transparent" />
                    </div>
                  )}
                </div>
                <div className="text-xs text-stone-400">
                  {pct !== null ? `${pct}%` : t("agentScanDialog.pleaseWaitSessions")}
                </div>
              </div>
            </div>
          )}

          {/* ── Success ── */}
          {summary && (
            <div className="flex flex-col gap-4">
              <div className="flex flex-col items-center gap-2 py-2">
                <CheckCircle2 size={36} className="text-emerald-500" />
                <p className="font-semibold text-stone-800">{t("agentScanDialog.indexComplete")}</p>
              </div>
              <div className="rounded-2xl bg-stone-50 p-4">
                <p className="mb-3 text-sm text-stone-500">
                  {t("agentScanDialog.totalScanned", { n: summary.total_in_file.toLocaleString() })}
                </p>
                <div className="grid grid-cols-3 gap-3 text-center">
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-emerald-600">{summary.added.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">{t("agentScanDialog.added")}</p>
                  </div>
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-amber-500">{summary.updated.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">{t("agentScanDialog.updated")}</p>
                  </div>
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-stone-400">{summary.skipped.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">{t("agentScanDialog.skipped")}</p>
                  </div>
                </div>
              </div>
              <button
                onClick={onClose}
                className="w-full rounded-xl bg-stone-900 py-2.5 text-sm font-medium text-white transition-colors hover:bg-stone-800"
              >
                {t("agentScanDialog.done")}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
