import { useState, useMemo, useRef, useEffect } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { readTextFile } from "@tauri-apps/plugin-fs";
import { markdownUrlTransform } from "../lib/markdown";
import {
  Upload,
  Search,
  Settings,
  X,
  FileJson,
  Tag,
  Eye,
  EyeOff,
  Loader2,
} from "lucide-react";
import { useI18n, type TranslationKey } from "../lib/i18n";

type FieldRole = "input" | "output" | "meta" | "search" | "hidden";

interface FieldConfig {
  role: FieldRole;
  label: string;
}

interface FieldStats {
  name: string;
  type: string;
  coverage: number; // 0-1
  sample: string;
}

function analyzeFields(records: Record<string, unknown>[]): FieldStats[] {
  if (records.length === 0) return [];
  const allKeys = new Set<string>();
  records.forEach((r) => Object.keys(r).forEach((k) => allKeys.add(k)));

  return Array.from(allKeys).map((key) => {
    let nonNull = 0;
    let type = "unknown";
    let sample = "";
    for (const rec of records) {
      const val = rec[key];
      if (val !== null && val !== undefined && val !== "") {
        nonNull++;
        if (!sample) {
          const str = typeof val === "object" ? JSON.stringify(val) : String(val);
          sample = str.length > 80 ? str.slice(0, 80) + "…" : str;
        }
        type = typeof val === "object" ? (Array.isArray(val) ? "array" : "object") : typeof val;
      }
    }
    return { name: key, type, coverage: nonNull / records.length, sample };
  });
}

function autoAssignRoles(stats: FieldStats[]): Record<string, FieldConfig> {
  const result: Record<string, FieldConfig> = {};
  const inputKeywords = ["input", "question", "instruction", "prompt", "query", "user", "human"];
  const outputKeywords = ["output", "answer", "response", "assistant", "result", "completion", "generated"];

  stats.forEach((s) => {
    const lower = s.name.toLowerCase();
    let role: FieldRole = "meta";
    if (inputKeywords.some((k) => lower.includes(k))) role = "input";
    else if (outputKeywords.some((k) => lower.includes(k))) role = "output";
    else if (s.type === "string" && s.coverage > 0.5) role = "search";
    result[s.name] = { role, label: s.name };
  });
  return result;
}

const roleBadge: Record<FieldRole, string> = {
  input: "bg-blue-100 text-blue-700 border-blue-200",
  output: "bg-emerald-100 text-emerald-700 border-emerald-200",
  meta: "bg-stone-100 text-stone-600 border-stone-200",
  search: "bg-violet-100 text-violet-700 border-violet-200",
  hidden: "bg-red-50 text-red-400 border-red-100",
};

const roleLabelKey: Record<FieldRole, TranslationKey> = {
  input: "jsonViewer.roleInput",
  output: "jsonViewer.roleOutput",
  meta: "jsonViewer.roleMeta",
  search: "jsonViewer.roleSearch",
  hidden: "jsonViewer.roleHidden",
};

function RecordCard({
  record,
  fields,
  index,
}: {
  record: Record<string, unknown>;
  fields: Record<string, FieldConfig>;
  index: number;
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const inputs = Object.entries(fields).filter(([, cfg]) => cfg.role === "input");
  const outputs = Object.entries(fields).filter(([, cfg]) => cfg.role === "output");
  const metas = Object.entries(fields).filter(([, cfg]) => cfg.role === "meta" || cfg.role === "search");

  const renderValue = (val: unknown, role: FieldRole) => {
    if (val === null || val === undefined || val === "") return <span className="italic text-stone-400">{t("common.empty")}</span>;
    const str = typeof val === "object" ? JSON.stringify(val, null, 2) : String(val);

    if (role === "input" || role === "output") {
      if (str.length > 300 && !expanded) {
        return (
          <>
            <div className="message-content text-sm leading-relaxed">
              <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]} urlTransform={markdownUrlTransform}>
                {str.slice(0, 300) + "…"}
              </ReactMarkdown>
            </div>
            <button
              onClick={(e) => {
                e.stopPropagation();
                setExpanded(true);
              }}
              className="mt-1 text-xs text-orange-500 hover:underline"
            >
              {t("jsonViewer.expandAll")}
            </button>
          </>
        );
      }
      return (
        <div className="message-content text-sm leading-relaxed">
          <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
            {str}
          </ReactMarkdown>
        </div>
      );
    }

    return <span className="text-xs text-stone-600 font-mono break-all">{str}</span>;
  };

  return (
    <div className="rounded-2xl border border-stone-200 bg-white shadow-sm overflow-hidden">
      {/* Index */}
      <div className="flex items-center gap-2 border-b border-stone-100 bg-stone-50 px-4 py-2">
        <span className="text-xs font-mono text-stone-400">#{index + 1}</span>
        {metas.map(([key]) => {
          const val = record[key];
          if (!val) return null;
          return (
            <span key={key} className="text-xs text-stone-500 truncate max-w-[200px]">
              {typeof val === "string" || typeof val === "number" ? String(val) : key}
            </span>
          );
        })}
      </div>

      <div className="divide-y divide-stone-100">
        {inputs.map(([key, cfg]) => (
          <div key={key} className="px-4 py-3">
            <div className="mb-1.5 flex items-center gap-2">
              <span className="text-xs font-medium text-blue-600">{cfg.label}</span>
              <span className="rounded-full border px-1.5 py-0.5 text-[10px] bg-blue-50 text-blue-500 border-blue-100">{t("jsonViewer.input")}</span>
            </div>
            <div className="rounded-xl bg-blue-50 px-3 py-2.5">
              {renderValue(record[key], "input")}
            </div>
          </div>
        ))}

        {outputs.map(([key, cfg]) => (
          <div key={key} className="px-4 py-3">
            <div className="mb-1.5 flex items-center gap-2">
              <span className="text-xs font-medium text-emerald-600">{cfg.label}</span>
              <span className="rounded-full border px-1.5 py-0.5 text-[10px] bg-emerald-50 text-emerald-500 border-emerald-100">{t("jsonViewer.output")}</span>
            </div>
            <div className="rounded-xl bg-emerald-50 px-3 py-2.5">
              {renderValue(record[key], "output")}
            </div>
          </div>
        ))}

        {expanded && (
          <div className="px-4 py-2">
            <button onClick={() => setExpanded(false)} className="text-xs text-stone-400 hover:text-stone-600">
              {t("jsonViewer.collapse")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function FieldConfigPanel({
  stats,
  configs,
  onChange,
  onClose,
}: {
  stats: FieldStats[];
  configs: Record<string, FieldConfig>;
  onChange: (key: string, cfg: FieldConfig) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const roles: FieldRole[] = ["input", "output", "meta", "search", "hidden"];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm">
      <div className="w-[600px] max-h-[80vh] flex flex-col rounded-2xl bg-white shadow-2xl">
        <div className="flex items-center justify-between border-b border-stone-200 px-6 py-4">
          <div>
            <h2 className="font-semibold text-stone-800">{t("jsonViewer.fieldConfigTitle")}</h2>
            <p className="text-xs text-stone-400 mt-0.5">{t("jsonViewer.fieldConfigSubtitle")}</p>
          </div>
          <button onClick={onClose} className="rounded-full p-1 text-stone-400 hover:bg-stone-100 hover:text-stone-600">
            <X size={18} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6 space-y-3">
          {stats.map((s) => {
            const cfg = configs[s.name] ?? { role: "meta", label: s.name };
            return (
              <div key={s.name} className="rounded-xl border border-stone-200 bg-stone-50 p-3">
                <div className="flex items-center gap-3 mb-2">
                  <code className="text-sm font-mono font-medium text-stone-800">{s.name}</code>
                  <span className="text-xs text-stone-400">{s.type}</span>
                  <span className="ml-auto text-xs text-stone-400">
                    {t("jsonViewer.coverage", { pct: Math.round(s.coverage * 100) })}
                  </span>
                </div>
                {s.sample && (
                  <p className="mb-2 text-xs text-stone-500 truncate">{s.sample}</p>
                )}
                <div className="flex gap-1.5 flex-wrap">
                  {roles.map((r) => (
                    <button
                      key={r}
                      onClick={() => onChange(s.name, { ...cfg, role: r })}
                      className={`rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors ${
                        cfg.role === r ? roleBadge[r] : "border-stone-200 bg-white text-stone-500 hover:bg-stone-100"
                      }`}
                    >
                      {t(roleLabelKey[r])}
                    </button>
                  ))}
                </div>
                <div className="mt-2">
                  <input
                    value={cfg.label}
                    onChange={(e) => onChange(s.name, { ...cfg, label: e.target.value })}
                    placeholder={t("jsonViewer.displayLabelPlaceholder")}
                    className="w-full rounded-lg border border-stone-200 bg-white px-2.5 py-1 text-xs outline-none focus:ring-1 focus:ring-orange-200"
                  />
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

interface JsonViewerProps {
  pendingFilePath?: string | null;
  onFileLoaded?: () => void;
}

export function JsonViewer({ pendingFilePath, onFileLoaded }: JsonViewerProps) {
  const { t } = useI18n();
  const [records, setRecords] = useState<Record<string, unknown>[]>([]);
  const [stats, setStats] = useState<FieldStats[]>([]);
  const [configs, setConfigs] = useState<Record<string, FieldConfig>>({});
  const [query, setQuery] = useState("");
  const [showConfig, setShowConfig] = useState(false);
  const [showStats, setShowStats] = useState(false);
  const [pathLoading, setPathLoading] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const ingestJson = (text: string) => {
    const raw = JSON.parse(text);
    const arr = Array.isArray(raw) ? raw : [raw];
    setRecords(arr);
    const s = analyzeFields(arr);
    setStats(s);
    setConfigs(autoAssignRoles(s));
    setQuery("");
  };

  // Auto-load file when a path is pushed from drag-drop
  useEffect(() => {
    if (!pendingFilePath) return;
    setPathLoading(true);
    readTextFile(pendingFilePath)
      .then((text) => {
        try {
          ingestJson(text);
        } catch {
          alert(t("jsonViewer.parseFailedAlert"));
        }
      })
      .catch(() => alert(t("jsonViewer.readFailedAlert")))
      .finally(() => {
        setPathLoading(false);
        onFileLoaded?.();
      });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingFilePath]);

  const loadFile = (file: File) => {
    const reader = new FileReader();
    reader.onload = (e) => {
      try {
        ingestJson(e.target?.result as string);
      } catch {
        alert(t("jsonViewer.parseFailedAlert"));
      }
    };
    reader.readAsText(file);
  };

  const filtered = useMemo(() => {
    if (!query.trim()) return records;
    const q = query.toLowerCase();
    const searchFields = Object.entries(configs)
      .filter(([, cfg]) => cfg.role === "search" || cfg.role === "input" || cfg.role === "output")
      .map(([key]) => key);

    return records.filter((r) =>
      searchFields.some((k) => {
        const val = r[k];
        return val && String(val).toLowerCase().includes(q);
      })
    );
  }, [records, query, configs]);

  const visibleFields = Object.fromEntries(
    Object.entries(configs).filter(([, cfg]) => cfg.role !== "hidden")
  );

  return (
    <div className="flex h-full flex-col bg-[#f7f5f2]">
      {/* Header */}
      <div className="border-b border-stone-200 bg-white px-6 pt-5 pb-4 shadow-sm">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-violet-100 text-violet-600">
              <FileJson size={15} />
            </div>
            <h1 className="text-lg font-semibold text-stone-800">{t("jsonViewer.title")}</h1>
          </div>
          <div className="flex items-center gap-2">
            {records.length > 0 && (
              <>
                <button
                  onClick={() => setShowStats(!showStats)}
                  className="flex items-center gap-1.5 rounded-lg border border-stone-200 bg-stone-50 px-3 py-1.5 text-xs text-stone-600 hover:bg-white"
                >
                  {showStats ? <EyeOff size={13} /> : <Eye size={13} />}
                  {t("jsonViewer.fieldStats")}
                </button>
                <button
                  onClick={() => setShowConfig(true)}
                  className="flex items-center gap-1.5 rounded-lg border border-violet-200 bg-violet-50 px-3 py-1.5 text-xs text-violet-700 hover:bg-violet-100"
                >
                  <Settings size={13} />
                  {t("jsonViewer.configureFields")}
                </button>
              </>
            )}
            <button
              onClick={() => fileRef.current?.click()}
              className="flex items-center gap-1.5 rounded-lg bg-stone-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-stone-800"
            >
              <Upload size={13} />
              {t("jsonViewer.uploadJson")}
            </button>
            <input
              ref={fileRef}
              type="file"
              accept=".json"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) loadFile(f);
                e.target.value = "";
              }}
            />
          </div>
        </div>

        {records.length > 0 && (
          <div className="flex gap-2 items-center">
            <div className="relative flex-1">
              <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-stone-400 pointer-events-none" />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("jsonViewer.searchPlaceholder")}
                className="h-9 w-full rounded-xl border border-stone-200 bg-stone-50 py-2 pl-9 pr-3 text-sm outline-none focus:border-orange-300 focus:bg-white focus:ring-2 focus:ring-orange-100"
              />
            </div>
            <span className="text-xs text-stone-400 whitespace-nowrap">
              {t("jsonViewer.countOfTotal", { filtered: filtered.length, total: records.length })}
            </span>
          </div>
        )}
      </div>

      {/* Field stats panel */}
      {showStats && records.length > 0 && (
        <div className="border-b border-stone-200 bg-amber-50 px-6 py-3">
          <div className="flex flex-wrap gap-2">
            {stats.map((s) => {
              const cfg = configs[s.name];
              return (
                <div
                  key={s.name}
                  className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs ${
                    cfg ? roleBadge[cfg.role] : "bg-stone-100 text-stone-500 border-stone-200"
                  }`}
                >
                  <Tag size={10} />
                  <span className="font-mono font-medium">{s.name}</span>
                  <span className="opacity-60">{Math.round(s.coverage * 100)}%</span>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-6 py-6">
        {pathLoading ? (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-3 text-stone-400">
              <Loader2 size={28} className="animate-spin text-violet-400" />
              <span className="text-sm">{t("jsonViewer.readingFile")}</span>
            </div>
          </div>
        ) : records.length === 0 ? (
          <div
            className="flex h-full flex-col items-center justify-center gap-6 rounded-2xl border-2 border-dashed border-stone-200 bg-white"
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.preventDefault();
              const f = e.dataTransfer.files[0];
              if (f) loadFile(f);
            }}
          >
            <div className="flex h-20 w-20 items-center justify-center rounded-2xl bg-violet-50 text-violet-400">
              <FileJson size={36} strokeWidth={1.2} />
            </div>
            <div className="text-center">
              <p className="font-semibold text-stone-700">{t("jsonViewer.dropHintTitle")}</p>
              <p className="mt-1 text-sm text-stone-400">{t("jsonViewer.dropHintSubtitle")}</p>
              <p className="mt-3 text-xs text-stone-400 max-w-sm">
                {t("jsonViewer.dropHintDesc")}
              </p>
            </div>
            <button
              onClick={() => fileRef.current?.click()}
              className="rounded-xl bg-violet-600 px-5 py-2.5 text-sm font-medium text-white hover:bg-violet-700"
            >
              {t("jsonViewer.chooseFile")}
            </button>
          </div>
        ) : (
          <div className="mx-auto max-w-3xl space-y-4">
            {filtered.length === 0 ? (
              <div className="py-16 text-center text-stone-400">
                <Search size={28} className="mx-auto mb-3 opacity-50" strokeWidth={1.5} />
                <p className="text-sm">{t("jsonViewer.noMatchingRecords")}</p>
              </div>
            ) : (
              filtered.map((r, i) => (
                <RecordCard key={i} record={r} fields={visibleFields} index={i} />
              ))
            )}
          </div>
        )}
      </div>

      {showConfig && (
        <FieldConfigPanel
          stats={stats}
          configs={configs}
          onChange={(key, cfg) => setConfigs((prev) => ({ ...prev, [key]: cfg }))}
          onClose={() => setShowConfig(false)}
        />
      )}
    </div>
  );
}
