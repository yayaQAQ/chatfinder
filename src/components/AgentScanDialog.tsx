import { useEffect, useState } from "react";
import { X, FolderSearch, CheckCircle2, Bot, Loader2, RefreshCw } from "lucide-react";
import { api, type AgentSource, type ImportSummary, type ImportProgress as Progress } from "../lib/api";
import { useToast } from "../lib/toast";

interface Props {
  onClose: () => void;
  onImported: () => void;
}

const toolIconColor: Record<string, string> = {
  "claude-code": "bg-orange-100 text-orange-600",
  codex: "bg-sky-100 text-sky-600",
};

export function AgentScanDialog({ onClose, onImported }: Props) {
  const [scanning, setScanning] = useState(true);
  const [sources, setSources] = useState<AgentSource[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const { push } = useToast();

  const scan = async () => {
    setScanning(true);
    setSummary(null);
    try {
      const found = await api.scanAgentSources();
      setSources(found);
      setSelected(new Set(found.map((s) => s.tool))); // default: all selected
    } catch (e) {
      push(`扫描失败：${e}`, "error");
    } finally {
      setScanning(false);
    }
  };

  useEffect(() => {
    scan();
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
      );
      setSummary(result);
      onImported();
    } catch (e) {
      push(`索引失败：${e}`, "error");
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
              <h2 className="font-semibold text-stone-800">本地 Agent 会话</h2>
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
              <p className="text-sm">正在扫描本地 Agent 目录…</p>
            </div>
          )}

          {/* ── Empty ── */}
          {!scanning && !summary && sources.length === 0 && (
            <div className="flex flex-col items-center gap-3 py-8 text-center">
              <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-stone-100 text-stone-400">
                <Bot size={24} />
              </div>
              <p className="font-medium text-stone-700">未发现本地 Agent 会话</p>
              <p className="max-w-[22rem] text-sm text-stone-400">
                未在 <code className="rounded bg-stone-100 px-1">~/.claude/projects</code> 或{" "}
                <code className="rounded bg-stone-100 px-1">~/.codex/sessions</code> 找到会话记录。
              </p>
              <button
                onClick={scan}
                className="mt-1 flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-sm text-stone-600 hover:bg-stone-50"
              >
                <RefreshCw size={13} /> 重新扫描
              </button>
            </div>
          )}

          {/* ── Found: prompt to index ── */}
          {!scanning && !summary && sources.length > 0 && !busy && (
            <>
              <p className="mb-3 text-sm text-stone-600">
                扫描到以下本地 Agent 会话，选择要索引导入的来源：
              </p>
              <div className="space-y-2">
                {sources.map((s) => {
                  const on = selected.has(s.tool);
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
                          {s.session_count.toLocaleString()} 会话
                        </p>
                        <p className="text-[11px] text-stone-400">
                          {s.message_count.toLocaleString()} 条消息
                        </p>
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
                索引导入 {totalSessions > 0 ? `（${totalSessions.toLocaleString()} 会话）` : ""}
              </button>
              <p className="mt-2 text-center text-[11px] text-stone-400">
                仅读取会话文件，导入后可在「全部对话」中搜索、收藏
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
                      ? "准备中…"
                      : progress.phase === "parse"
                      ? "解析会话文件…"
                      : "写入数据库"}
                  </span>
                </div>
                {isDbPhase && progress!.total > 0 && (
                  <span className="tabular-nums text-sm text-stone-500">
                    {progress!.current.toLocaleString()}
                    <span className="text-stone-300"> / </span>
                    {progress!.total.toLocaleString()} 会话
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
                  {pct !== null ? `${pct}%` : "请稍候，正在处理会话…"}
                </div>
              </div>
            </div>
          )}

          {/* ── Success ── */}
          {summary && (
            <div className="flex flex-col gap-4">
              <div className="flex flex-col items-center gap-2 py-2">
                <CheckCircle2 size={36} className="text-emerald-500" />
                <p className="font-semibold text-stone-800">索引完成</p>
              </div>
              <div className="rounded-2xl bg-stone-50 p-4">
                <p className="mb-3 text-sm text-stone-500">
                  共扫描 <strong>{summary.total_in_file.toLocaleString()}</strong> 个会话
                </p>
                <div className="grid grid-cols-3 gap-3 text-center">
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-emerald-600">{summary.added.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">新增</p>
                  </div>
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-amber-500">{summary.updated.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">更新</p>
                  </div>
                  <div className="rounded-xl bg-white p-3 ring-1 ring-stone-200">
                    <p className="text-xl font-bold text-stone-400">{summary.skipped.toLocaleString()}</p>
                    <p className="mt-0.5 text-xs text-stone-400">跳过</p>
                  </div>
                </div>
              </div>
              <button
                onClick={onClose}
                className="w-full rounded-xl bg-stone-900 py-2.5 text-sm font-medium text-white transition-colors hover:bg-stone-800"
              >
                完成
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
