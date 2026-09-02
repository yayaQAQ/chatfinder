import { useEffect, useState } from "react";
import { X, TerminalSquare, Save } from "lucide-react";
import { Select } from "./Select";
import { api } from "../lib/api";
import { useToast } from "../lib/toast";
import { useI18n } from "../lib/i18n";

interface Props {
  onClose: () => void;
  onSaved?: () => void;
}

export function TerminalSettings({ onClose, onSaved }: Props) {
  const [terminal, setTerminal] = useState("terminal");
  const [proxy, setProxy] = useState("");
  const [claudeArgs, setClaudeArgs] = useState("");
  const [codexArgs, setCodexArgs] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const { push } = useToast();
  const { t } = useI18n();

  // Load persisted settings
  useEffect(() => {
    Promise.all([
      api.getSetting("preferred_terminal"),
      api.getSetting("resume_proxy"),
      api.getSetting("claude_code_args"),
      api.getSetting("codex_args"),
    ]).then(([term, proxyUrl, claude, codex]) => {
      if (term) setTerminal(term);
      if (proxyUrl) setProxy(proxyUrl);
      if (claude) setClaudeArgs(claude);
      if (codex) setCodexArgs(codex);
      setLoaded(true);
    }).catch(() => setLoaded(true));
  }, []);

  const save = async () => {
    setSaving(true);
    try {
      await Promise.all([
        api.setSetting("preferred_terminal", terminal),
        api.setSetting("resume_proxy", proxy.trim()),
        api.setSetting("claude_code_args", claudeArgs.trim()),
        api.setSetting("codex_args", codexArgs.trim()),
      ]);
      push(t("terminalSettings.savedToast"), "success");
      onSaved?.();
      onClose();
    } catch (e) {
      push(String(e), "error");
    } finally {
      setSaving(false);
    }
  };

  const inputCls =
    "h-9 w-full rounded-xl border border-stone-200 bg-stone-50 px-3 text-sm outline-none transition-colors focus:border-amber-300 focus:bg-white focus:ring-2 focus:ring-amber-100";
  const labelCls = "mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-stone-600";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm">
      <div className="w-[520px] overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-stone-100 text-stone-600">
              <TerminalSquare size={17} />
            </div>
            <div>
              <h2 className="font-semibold text-stone-800">{t("terminalSettings.title")}</h2>
              <p className="text-xs text-stone-400">{t("terminalSettings.subtitle")}</p>
            </div>
          </div>
          <button onClick={onClose} className="rounded-full p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600">
            <X size={17} />
          </button>
        </div>

        <div className="px-6 py-5 space-y-5">
          {/* Terminal app */}
          <div>
            <label className={labelCls}>{t("terminalSettings.terminalLabel")}</label>
            <Select
              value={terminal}
              onChange={setTerminal}
              className={`${inputCls} cursor-pointer`}
              options={[
                { value: "terminal", label: t("terminalSettings.terminalOptionDefault") },
                { value: "iterm2", label: t("terminalSettings.terminalOptionIterm") },
              ]}
            />
          </div>

          {/* Proxy URL */}
          <div>
            <label className={labelCls}>{t("terminalSettings.proxyLabel")}</label>
            <input
              value={proxy}
              onChange={(e) => setProxy(e.target.value)}
              placeholder={t("terminalSettings.proxyPlaceholder")}
              className={inputCls}
            />
            <p className="mt-1 text-[11px] text-stone-400">{t("terminalSettings.proxyHint")}</p>
          </div>

          {/* Claude Code extra args */}
          <div>
            <label className={labelCls}>{t("terminalSettings.claudeArgsLabel")}</label>
            <input
              value={claudeArgs}
              onChange={(e) => setClaudeArgs(e.target.value)}
              placeholder={t("terminalSettings.claudeArgsPlaceholder")}
              className={inputCls}
            />
          </div>

          {/* Codex extra args */}
          <div>
            <label className={labelCls}>{t("terminalSettings.codexArgsLabel")}</label>
            <input
              value={codexArgs}
              onChange={(e) => setCodexArgs(e.target.value)}
              placeholder={t("terminalSettings.codexArgsPlaceholder")}
              className={inputCls}
            />
          </div>
        </div>

        {/* Actions */}
        <div className="flex items-center justify-end gap-2 border-t border-stone-100 px-6 py-4">
          <button
            onClick={onClose}
            className="rounded-xl border border-stone-200 px-4 py-2 text-sm font-medium text-stone-600 transition-colors hover:bg-stone-50"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={save}
            disabled={saving || !loaded}
            className="flex items-center gap-1.5 rounded-xl bg-stone-900 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-stone-800 disabled:opacity-50"
          >
            <Save size={14} />
            {t("terminalSettings.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
