import { useState, useEffect, useCallback } from "react";
import { Routes, Route, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { FileArchive, FileJson } from "lucide-react";
import { Sidebar } from "./components/Sidebar";
import { ImportDialog } from "./components/ImportDialog";
import { AgentScanDialog } from "./components/AgentScanDialog";
import { ImportHistoryDialog } from "./components/ImportHistoryDialog";
import { CommandPalette } from "./components/CommandPalette";
import { EmbedSettings } from "./components/EmbedSettings";
import { McpSettings } from "./components/McpSettings";
import { Conversations } from "./pages/Conversations";
import { ConversationDetail } from "./pages/ConversationDetail";
import { Favorites } from "./pages/Favorites";
import { JsonViewer } from "./pages/JsonViewer";
import { ToastProvider, useToast } from "./lib/toast";
import { api } from "./lib/api";
import type { EmbeddingStats } from "./lib/api";
import { useI18n } from "./lib/i18n";

// ─── Drop overlay ────────────────────────────────────────────────────────────
type DropTarget = "zip" | "json" | "unknown" | null;

function classifyPaths(paths: string[]): DropTarget {
  if (paths.length === 0) return null;
  const first = paths[0].toLowerCase();
  if (first.endsWith(".zip")) return "zip";
  if (first.endsWith(".json")) return "json";
  return "unknown";
}

function DropOverlay({ target }: { target: DropTarget }) {
  const { t } = useI18n();
  if (!target) return null;
  const isZip = target === "zip";
  const isJson = target === "json";
  const isUnknown = target === "unknown";
  return (
    <div className="pointer-events-none fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-md">
      <div
        className={`flex flex-col items-center gap-4 rounded-3xl border-2 border-dashed p-12 transition-all ${
          isZip
            ? "border-orange-400 bg-orange-500/10 text-orange-200"
            : isJson
            ? "border-violet-400 bg-violet-500/10 text-violet-200"
            : "border-stone-400 bg-stone-500/10 text-stone-400"
        }`}
      >
        {isZip && <FileArchive size={52} strokeWidth={1.2} className="text-orange-300" />}
        {isJson && <FileJson size={52} strokeWidth={1.2} className="text-violet-300" />}
        {isUnknown && <div className="text-4xl">📂</div>}
        <div className="text-center">
          <p className="text-xl font-semibold text-white">
            {isZip ? t("app.dropZipTitle") : isJson ? t("app.dropJsonTitle") : t("app.dropUnknownTitle")}
          </p>
          <p className="mt-1 text-sm opacity-70">
            {isZip ? t("app.dropZipSubtitle") : isJson ? t("app.dropJsonSubtitle") : t("app.dropUnknownSubtitle")}
          </p>
        </div>
      </div>
    </div>
  );
}

// ─── Inner app (needs router context) ────────────────────────────────────────
function InnerApp() {
  const [importOpen, setImportOpen] = useState(false);
  const [importZipPath, setImportZipPath] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [mcpOpen, setMcpOpen] = useState(false);
  const [agentScanOpen, setAgentScanOpen] = useState(false);
  const [importHistoryOpen, setImportHistoryOpen] = useState(false);
  const [embedStats, setEmbedStats] = useState<EmbeddingStats | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget>(null);
  const [pendingJsonPath, setPendingJsonPath] = useState<string | null>(null);
  const navigate = useNavigate();
  const { push } = useToast();
  const { t } = useI18n();

  // Load initial embedding stats so the brain icon shows without opening settings first
  useEffect(() => {
    api.getEmbeddingStats().then(setEmbedStats).catch(() => {});
  }, []);

  // Register Tauri drag-drop listeners
  useEffect(() => {
    let dragLeaveTimer: ReturnType<typeof setTimeout>;

    const unlistenEnter = listen<{ paths: string[] }>("tauri://drag-enter", (e) => {
      clearTimeout(dragLeaveTimer);
      setDropTarget(classifyPaths(e.payload.paths));
    });

    const unlistenOver = listen<{ paths: string[] }>("tauri://drag-over", (e) => {
      clearTimeout(dragLeaveTimer);
      if (!e.payload?.paths?.length) return;
      setDropTarget(classifyPaths(e.payload.paths));
    });

    const unlistenLeave = listen("tauri://drag-leave", () => {
      // Small delay to avoid flicker when moving between elements
      dragLeaveTimer = setTimeout(() => setDropTarget(null), 80);
    });

    const unlistenDrop = listen<{ paths: string[] }>("tauri://drag-drop", async (e) => {
      setDropTarget(null);
      const paths = e.payload.paths;
      if (!paths?.length) return;

      const path = paths[0];
      if (path.toLowerCase().endsWith(".zip")) {
        setImportZipPath(path);
        setImportOpen(true);
      } else if (path.toLowerCase().endsWith(".json")) {
        navigate("/json");
        setPendingJsonPath(path);
      } else {
        push(t("app.unsupportedFileToast"), "error");
      }
    });

    return () => {
      unlistenEnter.then((fn) => fn());
      unlistenOver.then((fn) => fn());
      unlistenLeave.then((fn) => fn());
      unlistenDrop.then((fn) => fn());
      clearTimeout(dragLeaveTimer);
    };
  }, [navigate, push, t]);

  // Global keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      }
      if (meta && e.key === "i") {
        e.preventDefault();
        setImportOpen(true);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const handleImported = useCallback(() => {
    setRefreshKey((k) => k + 1);
    setImportZipPath(null);
  }, []);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#f7f5f2]">
      <DropOverlay target={dropTarget} />

      <Sidebar
        onImportClick={() => setImportOpen(true)}
        onSearchClick={() => setPaletteOpen(true)}
        onSettingsClick={() => setSettingsOpen(true)}
        onMcpClick={() => setMcpOpen(true)}
        onAgentScanClick={() => setAgentScanOpen(true)}
        onImportHistoryClick={() => setImportHistoryOpen(true)}
        refreshKey={refreshKey}
        embedIndexed={embedStats?.indexed_messages ?? 0}
      />
      <main className="flex-1 overflow-hidden">
        <Routes>
          <Route path="/" element={<Conversations refreshKey={refreshKey} embedIndexed={embedStats?.indexed_messages ?? 0} onDataChanged={handleImported} />} />
          <Route path="/conversation/:id" element={<ConversationDetail onDataChanged={handleImported} />} />
          <Route path="/favorites" element={<Favorites />} />
          <Route
            path="/json"
            element={
              <JsonViewer
                pendingFilePath={pendingJsonPath}
                onFileLoaded={() => setPendingJsonPath(null)}
              />
            }
          />
        </Routes>
      </main>

      {importOpen && (
        <ImportDialog
          preloadedPath={importZipPath}
          onClose={() => { setImportOpen(false); setImportZipPath(null); }}
          onImported={handleImported}
        />
      )}

      {agentScanOpen && (
        <AgentScanDialog
          onClose={() => setAgentScanOpen(false)}
          onImported={handleImported}
        />
      )}

      {importHistoryOpen && (
        <ImportHistoryDialog
          onClose={() => setImportHistoryOpen(false)}
          onDeleted={handleImported}
        />
      )}

      {paletteOpen && (
        <CommandPalette
          onClose={() => setPaletteOpen(false)}
          embedIndexed={embedStats?.indexed_messages ?? 0}
        />
      )}

      {mcpOpen && <McpSettings onClose={() => setMcpOpen(false)} />}

      {settingsOpen && (
        <EmbedSettings
          onClose={() => setSettingsOpen(false)}
          onStatsChange={setEmbedStats}
        />
      )}
    </div>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <InnerApp />
    </ToastProvider>
  );
}
