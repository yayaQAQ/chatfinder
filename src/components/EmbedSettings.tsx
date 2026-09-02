import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { X, BrainCircuit, Plug, Cpu, KeyRound, PlayCircle, CheckCircle2, AlertCircle } from "lucide-react";
import { api, type EmbeddingStats } from "../lib/api";
import { useToast } from "../lib/toast";
import { useI18n, type TranslationKey } from "../lib/i18n";

interface Props {
  onClose: () => void;
  onStatsChange?: (stats: EmbeddingStats) => void;
}

interface Progress { current: number; total: number; }

const PRESETS: { labelKey?: TranslationKey; label?: string; url: string; model: string }[] = [
  { labelKey: "embedSettings.presetOllama", url: "http://127.0.0.1:11434", model: "nomic-embed-text" },
  { label: "Jina AI", url: "https://api.jina.ai", model: "jina-embeddings-v3" },
];

export function EmbedSettings({ onClose, onStatsChange }: Props) {
  const { t } = useI18n();
  const [apiUrl, setApiUrl] = useState("http://127.0.0.1:11434");
  const [model, setModel] = useState("nomic-embed-text");
  const [apiKey, setApiKey] = useState("");
  const [stats, setStats] = useState<EmbeddingStats | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<"ok" | "err" | null>(null);
  const [testMsg, setTestMsg] = useState("");
  const [generating, setGenerating] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const { push } = useToast();

  // Load persisted settings
  useEffect(() => {
    Promise.all([
      api.getSetting("embed_api_url"),
      api.getSetting("embed_model"),
      api.getSetting("embed_api_key"),
    ]).then(([url, mdl, key]) => {
      if (url) setApiUrl(url);
      if (mdl) setModel(mdl);
      if (key) setApiKey(key);
    });
    api.getEmbeddingStats().then((s) => {
      setStats(s);
      onStatsChange?.(s);
    });
  }, [onStatsChange]);

  // Listen to embedding progress events
  useEffect(() => {
    const unsub = listen<Progress>("embed:progress", (e) => setProgress(e.payload));
    return () => { unsub.then((fn) => fn()); };
  }, []);

  const saveSettings = async () => {
    await Promise.all([
      api.setSetting("embed_api_url", apiUrl),
      api.setSetting("embed_model", model),
      api.setSetting("embed_api_key", apiKey),
    ]);
  };

  const testConnection = async () => {
    setTesting(true);
    setTestResult(null);
    setTestMsg("");
    try {
      await saveSettings();
      const msg = await api.testEmbedConnection(apiUrl, model, apiKey);
      setTestResult("ok");
      setTestMsg(msg);
    } catch (e: unknown) {
      setTestResult("err");
      // Show a helpful hint for common localhost issues
      const raw = String(e);
      const hint = raw.includes("Connection refused") || raw.includes("连接失败")
        ? `${raw} ${t("embedSettings.ollamaHint", { model })}`
        : raw;
      setTestMsg(hint.slice(0, 200));
    } finally {
      setTesting(false);
    }
  };

  const startGenerate = async () => {
    setGenerating(true);
    setProgress(null);
    try {
      await saveSettings();
      const newStats = await api.generateEmbeddings(apiUrl, model, apiKey);
      setStats(newStats);
      onStatsChange?.(newStats);
      push(t("embedSettings.generateSuccessToast", { n: newStats.indexed_messages }), "success");
    } catch (e: unknown) {
      push(t("embedSettings.generateFailToast", { error: String(e) }), "error");
    } finally {
      setGenerating(false);
      setProgress(null);
    }
  };

  const applyPreset = (preset: (typeof PRESETS)[0]) => {
    setApiUrl(preset.url);
    setModel(preset.model);
    setTestResult(null);
  };

  const pct = progress && progress.total > 0
    ? Math.round((progress.current / progress.total) * 100)
    : null;

  const indexed = stats?.indexed_messages ?? 0;
  const total = stats?.total_messages ?? 0;
  const coverage = total > 0 ? Math.round((indexed / total) * 100) : 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm">
      <div className="w-[520px] overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-violet-100 text-violet-600">
              <BrainCircuit size={17} />
            </div>
            <div>
              <h2 className="font-semibold text-stone-800">{t("embedSettings.title")}</h2>
              <p className="text-xs text-stone-400">{t("embedSettings.subtitle")}</p>
            </div>
          </div>
          <button onClick={onClose} className="rounded-full p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600">
            <X size={17} />
          </button>
        </div>

        <div className="px-6 py-5 space-y-5">
          {/* Presets */}
          <div>
            <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-stone-400">{t("embedSettings.quickConfig")}</label>
            <div className="flex gap-2">
              {PRESETS.map((p) => (
                <button
                  key={p.url}
                  onClick={() => applyPreset(p)}
                  className={`rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors ${
                    apiUrl === p.url
                      ? "border-violet-300 bg-violet-50 text-violet-700"
                      : "border-stone-200 bg-stone-50 text-stone-600 hover:bg-stone-100"
                  }`}
                >
                  {p.labelKey ? t(p.labelKey) : p.label}
                </button>
              ))}
            </div>
          </div>

          {/* API URL */}
          <div>
            <label className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-stone-600">
              <Plug size={12} /> {t("embedSettings.apiUrl")}
            </label>
            <input
              value={apiUrl}
              onChange={(e) => { setApiUrl(e.target.value); setTestResult(null); }}
              placeholder="http://localhost:11434"
              className="h-9 w-full rounded-xl border border-stone-200 bg-stone-50 px-3 text-sm outline-none transition-colors focus:border-violet-300 focus:bg-white focus:ring-2 focus:ring-violet-100"
            />
          </div>

          {/* Model */}
          <div>
            <label className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-stone-600">
              <Cpu size={12} /> {t("embedSettings.model")}
            </label>
            <input
              value={model}
              onChange={(e) => { setModel(e.target.value); setTestResult(null); }}
              placeholder="nomic-embed-text"
              className="h-9 w-full rounded-xl border border-stone-200 bg-stone-50 px-3 text-sm outline-none transition-colors focus:border-violet-300 focus:bg-white focus:ring-2 focus:ring-violet-100"
            />
          </div>

          {/* API Key */}
          <div>
            <label className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-stone-600">
              <KeyRound size={12} /> API Key <span className="font-normal text-stone-400">{t("embedSettings.apiKeyOptional")}</span>
            </label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => { setApiKey(e.target.value); setTestResult(null); }}
              placeholder="sk-..."
              className="h-9 w-full rounded-xl border border-stone-200 bg-stone-50 px-3 text-sm outline-none transition-colors focus:border-violet-300 focus:bg-white focus:ring-2 focus:ring-violet-100"
            />
          </div>

          {/* Test result */}
          {testResult && (
            <div className={`flex items-center gap-2 rounded-xl px-3 py-2 text-sm ${
              testResult === "ok"
                ? "bg-emerald-50 text-emerald-700"
                : "bg-red-50 text-red-700"
            }`}>
              {testResult === "ok" ? <CheckCircle2 size={15} /> : <AlertCircle size={15} />}
              <span className="line-clamp-2 text-xs">{testMsg}</span>
            </div>
          )}

          {/* Stats */}
          {stats && (
            <div className="rounded-xl bg-stone-50 p-4">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-500">{t("embedSettings.indexStatus")}</span>
                <span className="text-xs text-stone-400">
                  {t("embedSettings.messagesOfTotal", { indexed: indexed.toLocaleString(), total: total.toLocaleString() })}
                  {stats.model && <span className="ml-1 text-violet-500">· {stats.model}</span>}
                </span>
              </div>
              <div className="h-2 w-full overflow-hidden rounded-full bg-stone-200">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-violet-400 to-purple-500 transition-all duration-500"
                  style={{ width: `${coverage}%` }}
                />
              </div>
              <p className="mt-1.5 text-right text-[11px] text-stone-400">{t("embedSettings.percentIndexed", { pct: coverage })}</p>
            </div>
          )}

          {/* Generate progress */}
          {generating && (
            <div className="space-y-1.5">
              <div className="flex justify-between text-xs text-stone-500">
                <span>{pct !== null ? t("embedSettings.processedOf", { current: progress!.current, total: progress!.total }) : t("embedSettings.preparing")}</span>
                {pct !== null && <span>{pct}%</span>}
              </div>
              <div className="h-2 w-full overflow-hidden rounded-full bg-stone-100">
                {pct !== null ? (
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-violet-400 to-purple-500 transition-all duration-300"
                    style={{ width: `${pct}%` }}
                  />
                ) : (
                  <div className="h-full w-1/3 animate-pulse rounded-full bg-gradient-to-r from-violet-300 to-purple-400" />
                )}
              </div>
            </div>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2 border-t border-stone-100 px-6 py-4">
          <button
            onClick={testConnection}
            disabled={testing || generating}
            className="flex items-center gap-1.5 rounded-xl border border-stone-200 px-4 py-2 text-sm font-medium text-stone-600 transition-colors hover:bg-stone-50 disabled:opacity-50"
          >
            {testing ? (
              <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-stone-300 border-t-stone-600" />
            ) : (
              <Plug size={14} />
            )}
            {t("embedSettings.testConnection")}
          </button>
          <button
            onClick={startGenerate}
            disabled={generating || indexed >= total}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-xl bg-gradient-to-r from-violet-500 to-purple-600 py-2 text-sm font-medium text-white shadow-sm transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {generating ? (
              <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-white/40 border-t-white" />
            ) : (
              <PlayCircle size={15} />
            )}
            {generating
              ? t("embedSettings.generating")
              : indexed >= total && total > 0
              ? t("embedSettings.indexAlreadyComplete")
              : t("embedSettings.generateIndex", { n: (total - indexed).toLocaleString() })}
          </button>
        </div>
      </div>
    </div>
  );
}
