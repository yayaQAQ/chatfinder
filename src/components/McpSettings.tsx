import { useCallback, useEffect, useState } from "react";
import { X, Plug, Copy, Check, RefreshCw, ShieldCheck, Ban, Activity, Timer, Info } from "lucide-react";
import { api, type McpStatus, type McpActivityEntry, type McpSetupSnippet } from "../lib/api";
import { useToast } from "../lib/toast";
import { useI18n } from "../lib/i18n";

interface Props {
  onClose: () => void;
}

const CLIENT_LABEL = {
  claude_code: "mcpSettings.clientClaudeCode",
  codex: "mcpSettings.clientCodex",
  json: "mcpSettings.clientJson",
} as const;

const CLIENT_HINT = {
  claude_code: "mcpSettings.hintClaudeCode",
  codex: "mcpSettings.hintCodex",
  json: "mcpSettings.hintJson",
} as const;

/** Masks the token until the user asks to see it — the panel is the kind of
 *  screen people share in screenshots and bug reports. */
function maskToken(token: string) {
  return token.length > 8 ? `${token.slice(0, 4)}${"•".repeat(24)}${token.slice(-4)}` : token;
}

export function McpSettings({ onClose }: Props) {
  const { t } = useI18n();
  const { push } = useToast();
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [activity, setActivity] = useState<McpActivityEntry[]>([]);
  const [portDraft, setPortDraft] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const [revealToken, setRevealToken] = useState(false);
  const [client, setClient] = useState<McpSetupSnippet["client"]>("claude_code");
  const [mode, setMode] = useState<"manual" | "enroll">("manual");
  const [secondsLeft, setSecondsLeft] = useState(0);
  const [busy, setBusy] = useState(false);

  const apply = useCallback((next: McpStatus) => {
    setStatus(next);
    setPortDraft(String(next.configured_port));
    setSecondsLeft(next.enrollment_seconds_left);
  }, []);

  useEffect(() => {
    api.mcpStatus().then(apply).catch((e) => push(String(e), "error"));
  }, [apply, push]);

  // Poll while the server is up so a call made by an agent shows without the
  // user having to reopen the panel.
  useEffect(() => {
    if (!status?.running) return;
    let alive = true;
    const tick = () => {
      api.mcpActivity().then((a) => { if (alive) setActivity(a); }).catch(() => {});
      // Also re-reads the enrollment window: it can close without any UI action
      // when a client collects the token.
      api.mcpStatus().then((s) => {
        if (alive) setSecondsLeft(s.enrollment_seconds_left);
      }).catch(() => {});
    };
    tick();
    const timer = setInterval(tick, 2000);
    return () => { alive = false; clearInterval(timer); };
  }, [status?.running]);

  // Ticks down locally between polls so the countdown looks live; the poll
  // below is what actually detects the window being consumed by a client.
  useEffect(() => {
    if (secondsLeft <= 0) return;
    const timer = setInterval(() => setSecondsLeft((n) => Math.max(0, n - 1)), 1000);
    return () => clearInterval(timer);
  }, [secondsLeft]);

  const run = async (fn: () => Promise<McpStatus>) => {
    setBusy(true);
    try {
      apply(await fn());
    } catch (e: unknown) {
      push(t("mcpSettings.failed", { error: String(e) }), "error");
    } finally {
      setBusy(false);
    }
  };

  const copy = async (value: string, key: string) => {
    await navigator.clipboard.writeText(value);
    setCopied(key);
    setTimeout(() => setCopied((c) => (c === key ? null : c)), 1500);
  };

  const regenerate = () => {
    if (!confirm(t("mcpSettings.regenerateConfirm"))) return;
    run(async () => {
      const next = await api.mcpRegenerateToken();
      push(t("mcpSettings.regenerated"), "success");
      return next;
    });
  };

  if (!status) return null;

  const snippet = status.setup.find((s) => s.client === client) ?? status.setup[0];
  const portMoved = status.running && status.port !== status.configured_port;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm">
      <div className="flex max-h-[86vh] w-[560px] flex-col overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-stone-200">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-stone-100 px-6 py-4">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-sky-100 text-sky-600">
              <Plug size={17} />
            </div>
            <div>
              <h2 className="font-semibold text-stone-800">{t("mcpSettings.title")}</h2>
              <p className="text-xs text-stone-400">{t("mcpSettings.subtitle")}</p>
            </div>
          </div>
          <button onClick={onClose} className="rounded-full p-1.5 text-stone-400 hover:bg-stone-100 hover:text-stone-600">
            <X size={17} />
          </button>
        </div>

        <div className="space-y-5 overflow-y-auto px-6 py-5">
          {/* Master switch */}
          <div className="flex items-start justify-between gap-4 rounded-xl border border-stone-200 bg-stone-50 px-4 py-3">
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-stone-800">{t("mcpSettings.enable")}</span>
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
                    status.running ? "bg-emerald-100 text-emerald-700" : "bg-stone-200 text-stone-500"
                  }`}
                >
                  {status.running ? t("mcpSettings.running", { port: status.port }) : t("mcpSettings.stopped")}
                </span>
              </div>
              <p className="mt-1 text-xs leading-relaxed text-stone-500">{t("mcpSettings.enableHint")}</p>
              {portMoved && (
                <p className="mt-1 text-xs text-amber-600">
                  {t("mcpSettings.portFallback", { configured: status.configured_port, port: status.port })}
                </p>
              )}
              {status.last_error && <p className="mt-1 text-xs text-rose-600">{status.last_error}</p>}
            </div>
            <button
              disabled={busy}
              onClick={() => run(() => api.mcpSetEnabled(!status.enabled))}
              className={`relative h-6 w-11 shrink-0 rounded-full transition-colors disabled:opacity-50 ${
                status.enabled ? "bg-emerald-500" : "bg-stone-300"
              }`}
              aria-pressed={status.enabled}
            >
              <span
                className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all ${
                  status.enabled ? "left-[22px]" : "left-0.5"
                }`}
              />
            </button>
          </div>

          {status.enabled && (
            <>
              {/* Registration — any MCP client works, so let the user pick theirs */}
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-stone-400">
                  {t("mcpSettings.connectTitle")}
                </label>
                <div className="mb-2 flex gap-1 rounded-lg bg-stone-100 p-0.5">
                  {status.setup.map((s) => (
                    <button
                      key={s.client}
                      onClick={() => setClient(s.client)}
                      className={`flex-1 rounded-md px-2 py-1 text-xs font-medium transition-colors ${
                        client === s.client
                          ? "bg-white text-stone-800 shadow-sm"
                          : "text-stone-500 hover:text-stone-700"
                      }`}
                    >
                      {t(CLIENT_LABEL[s.client])}
                    </button>
                  ))}
                </div>
                <div className="mb-2 flex gap-3 text-xs">
                  {(["manual", "enroll"] as const).map((m) => (
                    <label key={m} className="flex cursor-pointer items-center gap-1.5 text-stone-600">
                      <input
                        type="radio"
                        checked={mode === m}
                        onChange={() => setMode(m)}
                        className="accent-sky-600"
                      />
                      {t(m === "manual" ? "mcpSettings.modeManual" : "mcpSettings.modeEnroll")}
                    </label>
                  ))}
                </div>

                {mode === "manual" ? (
                  <>
                    <p className="mb-2 text-xs text-stone-500">{t(CLIENT_HINT[client])}</p>
                    <div className="flex items-stretch gap-2">
                      <code className="min-w-0 flex-1 overflow-x-auto whitespace-pre rounded-lg border border-stone-200 bg-stone-900 px-3 py-2.5 text-[11px] leading-relaxed text-stone-100">
                        {snippet?.content}
                      </code>
                      <button
                        onClick={() => snippet && copy(snippet.content, "cmd")}
                        className="shrink-0 rounded-lg border border-stone-200 bg-white px-3 text-xs font-medium text-stone-600 hover:bg-stone-50"
                      >
                        {copied === "cmd" ? <Check size={14} className="text-emerald-600" /> : <Copy size={14} />}
                      </button>
                    </div>
                  </>
                ) : !snippet?.enroll ? (
                  <p className="rounded-lg bg-stone-50 px-3 py-2.5 text-xs text-stone-500">
                    {t("mcpSettings.enrollUnsupported")}
                  </p>
                ) : (
                  <>
                    <p className="mb-2 flex items-start gap-1.5 text-xs leading-relaxed text-stone-500">
                      <Info size={13} className="mt-px shrink-0 text-stone-400" />
                      <span>{secondsLeft > 0 ? t("mcpSettings.enrollHint") : t("mcpSettings.enrollIdle")}</span>
                    </p>

                    {secondsLeft > 0 ? (
                      <>
                        <div className="mb-2 flex items-stretch gap-2">
                          <code className="min-w-0 flex-1 overflow-x-auto whitespace-pre rounded-lg border border-stone-200 bg-stone-900 px-3 py-2.5 text-[11px] leading-relaxed text-stone-100">
                            {snippet.enroll}
                          </code>
                          <button
                            onClick={() => copy(snippet.enroll!, "enroll")}
                            className="shrink-0 rounded-lg border border-stone-200 bg-white px-3 text-xs font-medium text-stone-600 hover:bg-stone-50"
                          >
                            {copied === "enroll" ? <Check size={14} className="text-emerald-600" /> : <Copy size={14} />}
                          </button>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="flex items-center gap-1.5 rounded-full bg-emerald-50 px-2.5 py-1 text-xs font-medium text-emerald-700">
                            <Timer size={12} />
                            {t("mcpSettings.enrollCountdown", { seconds: secondsLeft })}
                          </span>
                          <button
                            disabled={busy}
                            onClick={() => run(api.mcpCancelEnrollment)}
                            className="text-xs text-stone-500 underline underline-offset-2 hover:text-stone-700 disabled:opacity-40"
                          >
                            {t("mcpSettings.enrollCancel")}
                          </button>
                        </div>
                      </>
                    ) : (
                      <button
                        disabled={busy || !status.running}
                        onClick={() => run(api.mcpOpenEnrollment)}
                        title={status.running ? undefined : t("mcpSettings.enrollNeedsRunning")}
                        className="flex items-center gap-1.5 rounded-lg bg-sky-600 px-3 py-2 text-xs font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
                      >
                        <Timer size={13} />
                        {t("mcpSettings.enrollOpen")}
                      </button>
                    )}

                    <p className="mt-2 text-[11px] leading-relaxed text-amber-600">{t("mcpSettings.enrollWhy")}</p>
                  </>
                )}
                <p className="mt-2 text-[11px] text-stone-400">{t("mcpSettings.restartHint")}</p>
              </div>

              {/* Endpoint + port */}
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-stone-400">
                    {t("mcpSettings.endpoint")}
                  </label>
                  <div className="flex items-center gap-1.5">
                    <input
                      readOnly
                      value={status.url}
                      className="min-w-0 flex-1 rounded-lg border border-stone-200 bg-stone-50 px-2.5 py-1.5 text-xs text-stone-600"
                    />
                    <button
                      onClick={() => copy(status.url, "url")}
                      className="shrink-0 rounded-lg border border-stone-200 p-1.5 text-stone-500 hover:bg-stone-50"
                    >
                      {copied === "url" ? <Check size={13} className="text-emerald-600" /> : <Copy size={13} />}
                    </button>
                  </div>
                </div>
                <div>
                  <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-stone-400">
                    {t("mcpSettings.port")}
                  </label>
                  <div className="flex items-center gap-1.5">
                    <input
                      value={portDraft}
                      onChange={(e) => setPortDraft(e.target.value.replace(/\D/g, ""))}
                      className="min-w-0 flex-1 rounded-lg border border-stone-200 px-2.5 py-1.5 text-xs text-stone-700 focus:border-sky-400 focus:outline-none"
                    />
                    <button
                      disabled={busy || !portDraft || Number(portDraft) === status.configured_port}
                      onClick={() => run(() => api.mcpSetPort(Number(portDraft)))}
                      className="shrink-0 rounded-lg border border-stone-200 px-2.5 py-1.5 text-xs font-medium text-stone-600 hover:bg-stone-50 disabled:opacity-40"
                    >
                      {t("mcpSettings.portApply")}
                    </button>
                  </div>
                </div>
              </div>

              {/* Token */}
              <div>
                <label className="mb-1.5 block text-xs font-semibold uppercase tracking-wider text-stone-400">
                  {t("mcpSettings.token")}
                </label>
                <div className="flex items-center gap-1.5">
                  <input
                    readOnly
                    onFocus={() => setRevealToken(true)}
                    value={revealToken ? status.token : maskToken(status.token)}
                    className="min-w-0 flex-1 rounded-lg border border-stone-200 bg-stone-50 px-2.5 py-1.5 font-mono text-xs text-stone-600"
                  />
                  <button
                    onClick={() => copy(status.token, "token")}
                    className="shrink-0 rounded-lg border border-stone-200 p-1.5 text-stone-500 hover:bg-stone-50"
                  >
                    {copied === "token" ? <Check size={13} className="text-emerald-600" /> : <Copy size={13} />}
                  </button>
                  <button
                    disabled={busy}
                    onClick={regenerate}
                    title={t("mcpSettings.regenerate")}
                    className="shrink-0 rounded-lg border border-stone-200 p-1.5 text-stone-500 hover:bg-stone-50 disabled:opacity-40"
                  >
                    <RefreshCw size={13} />
                  </button>
                </div>
                <p className="mt-1.5 text-xs leading-relaxed text-amber-600">{t("mcpSettings.tokenHint")}</p>
              </div>

              {/* Capability summary */}
              <div className="rounded-xl border border-stone-200 px-4 py-3">
                <div className="mb-2 text-xs font-semibold uppercase tracking-wider text-stone-400">
                  {t("mcpSettings.toolsTitle")}
                </div>
                <div className="space-y-1.5 text-xs text-stone-600">
                  <div className="flex items-start gap-2">
                    <ShieldCheck size={14} className="mt-px shrink-0 text-emerald-600" />
                    <span>{t("mcpSettings.toolsRead")}</span>
                  </div>
                  <div className="flex items-start gap-2">
                    <Ban size={14} className="mt-px shrink-0 text-stone-400" />
                    <span>{t("mcpSettings.toolsNoWrite")}</span>
                  </div>
                </div>
              </div>

              {/* Activity log — what makes an open port feel accountable */}
              <div>
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-xs font-semibold uppercase tracking-wider text-stone-400">
                    {t("mcpSettings.activityTitle")}
                  </span>
                  <span className="text-[10px] text-stone-400">{t("mcpSettings.activityHint")}</span>
                </div>
                <div className="max-h-44 overflow-y-auto rounded-xl border border-stone-200">
                  {activity.length === 0 ? (
                    <div className="flex items-center gap-2 px-4 py-3 text-xs text-stone-400">
                      <Activity size={13} />
                      {t("mcpSettings.activityEmpty")}
                    </div>
                  ) : (
                    activity.map((entry, i) => (
                      <div
                        key={`${entry.at}-${i}`}
                        className="flex items-center gap-2 border-b border-stone-100 px-3 py-2 text-xs last:border-0"
                      >
                        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${entry.ok ? "bg-emerald-500" : "bg-rose-500"}`} />
                        <span className="shrink-0 font-medium text-stone-700">{entry.tool}</span>
                        <span className="min-w-0 flex-1 truncate text-stone-400">{entry.detail}</span>
                        <span className="shrink-0 tabular-nums text-stone-400">{entry.duration_ms}ms</span>
                        <span className="shrink-0 tabular-nums text-stone-300">
                          {new Date(entry.at).toLocaleTimeString()}
                        </span>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
