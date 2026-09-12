import { invoke, Channel } from "@tauri-apps/api/core";

export interface ConversationSummary {
  id: string;
  platform: "claude" | "chatgpt" | "deepseek" | "claude-code" | "codex";
  title: string;
  summary: string;
  url: string | null;
  created_at: string | null;
  updated_at: string | null;
  message_count: number;
  /** Distinct models used in this conversation, first-appearance order. */
  models: string[];
}

export interface MessageRow {
  id: string;
  conversation_id: string;
  sender: string;
  text: string;
  created_at: string | null;
  seq: number;
  kind: string; // "text" | "tool_use" | "tool_result" | "thinking"
  model: string | null; // model that produced this message (assistant turns only)
}

export interface TagRow {
  id: number;
  name: string;
  color: string;
}

export interface FavoriteRow {
  id: string;
  conversation_id: string;
  conversation_title: string;
  message_id: string | null;
  selected_text: string;
  note: string;
  created_at: string;
  tags: TagRow[];
}

export interface ImportSummary {
  platform: string;
  added: number;
  updated: number;
  skipped: number;
  total_in_file: number;
}

export interface ImportProgress {
  current: number;
  total: number;
  phase: string; // "parse" | "db"
}

export interface ImportBatchRow {
  id: string;
  source_file: string;
  platform: string;
  imported_at: string;
  added_count: number;
  updated_count: number;
  skipped_count: number;
  remaining_conversations: number;
}

export interface SearchHit {
  conversation_id: string;
  conversation_title: string;
  kind: string; // "title" | "message" | "semantic:<score>"
  ref_id: string;
  snippet: string;
  platform: string;
  updated_at: string | null;
  created_at: string | null;
}

export interface McpStatus {
  enabled: boolean;
  running: boolean;
  /** Port actually bound — may differ from configured_port if that was taken. */
  port: number;
  configured_port: number;
  token: string;
  url: string;
  /** Ready-to-paste registration for each supported client. */
  setup: McpSetupSnippet[];
  /** Seconds left on the self-service enrollment window; 0 when closed. */
  enrollment_seconds_left: number;
  last_error: string | null;
}

export interface AutoSyncStatus {
  enabled: boolean;
  interval_minutes: number;
  interval_choices: number[];
  last_run: string | null;
  /** Outcome of the last run, or the error it failed with. */
  last_result: string | null;
  in_flight: boolean;
}

export interface McpSetupSnippet {
  client: "claude_code" | "codex" | "json";
  kind: "shell" | "toml" | "json";
  content: string;
  /** Secret-free variant that fetches the token from /enroll at run time. */
  enroll: string | null;
}

export interface McpActivityEntry {
  at: string;
  tool: string;
  detail: string;
  duration_ms: number;
  ok: boolean;
}

export interface EmbeddingStats {
  total_messages: number;
  indexed_messages: number;
  model: string | null;
}

export interface ModelRow {
  model: string;
  conversation_count: number;
}

export interface EmbedConfig {
  apiUrl: string;
  model: string;
  apiKey: string;
}

export interface AgentSource {
  tool: string;          // "claude-code" | "codex"
  label: string;         // "Claude Code" | "Codex"
  dir: string;           // absolute directory scanned (empty if unresolvable)
  session_count: number;
  message_count: number;
  accessible: boolean;   // false if the directory couldn't be read at all
}

// tool -> directory, picked manually via a folder dialog this session (not persisted)
export type AgentDirOverrides = Record<string, string>;

export type SortOption = "newest" | "oldest" | "most_messages" | "fewest_messages";

export type RoleFilter = "" | "human" | "assistant";

export interface StructuralFilter {
  platform?: string;
  dateFrom?: string;
  dateTo?: string;
  minMessages?: number;
  maxMessages?: number;
  /** Raw model id — matches conversations that used it at least once. */
  model?: string;
  role?: RoleFilter;
}

export interface ConversationFilter extends StructuralFilter {
  sort?: SortOption;
}

export const api = {
  importZip: (zipPath: string, onProgress: (p: ImportProgress) => void) => {
    const channel = new Channel<ImportProgress>();
    channel.onmessage = onProgress;
    return invoke<ImportSummary>("import_zip_file", { zipPath, onProgress: channel });
  },
  scanAgentSources: (overrides?: AgentDirOverrides) =>
    invoke<AgentSource[]>("scan_agent_sources", { overrides: overrides ?? {} }),
  importAgentSessions: (
    tools: string[],
    onProgress: (p: ImportProgress) => void,
    overrides?: AgentDirOverrides,
  ) => {
    const channel = new Channel<ImportProgress>();
    channel.onmessage = onProgress;
    return invoke<ImportSummary>("import_agent_sessions", { tools, overrides: overrides ?? {}, onProgress: channel });
  },
  listConversations: (filter: ConversationFilter, limit: number, offset: number) =>
    invoke<ConversationSummary[]>("list_conversations", {
      query: "",
      platform: filter.platform ?? "",
      dateFrom: filter.dateFrom ?? "",
      dateTo: filter.dateTo ?? "",
      minMessages: filter.minMessages ?? 0,
      maxMessages: filter.maxMessages ?? 0,
      modelFilter: filter.model ?? "",
      sort: filter.sort ?? "newest",
      limit,
      offset,
    }),
  countConversations: () => invoke<number>("count_conversations"),
  listModels: () => invoke<ModelRow[]>("list_models"),
  getConversation: (id: string) =>
    invoke<[ConversationSummary, MessageRow[]]>("get_conversation", { id }),
  listConversationsByPath: (path: string) =>
    invoke<ConversationSummary[]>("list_conversations_by_path", { path }),
  searchConversationsByPath: (path: string, query: string) =>
    invoke<SearchHit[]>("search_conversations_by_path", { path, query }),
  deleteConversation: (id: string) => invoke<void>("delete_conversation", { id }),
  listImportBatches: () => invoke<ImportBatchRow[]>("list_import_batches"),
  deleteImportBatch: (batchId: string) => invoke<number>("delete_import_batch", { batchId }),
  searchAll: (query: string, filter: StructuralFilter = {}) =>
    invoke<SearchHit[]>("search_all", {
      query,
      platform: filter.platform ?? "",
      dateFrom: filter.dateFrom ?? "",
      dateTo: filter.dateTo ?? "",
      minMessages: filter.minMessages ?? 0,
      maxMessages: filter.maxMessages ?? 0,
      modelFilter: filter.model ?? "",
      role: filter.role ?? "",
    }),
  searchFavorites: (query: string) => invoke<FavoriteRow[]>("search_favorites", { query }),
  createFavorite: (
    conversationId: string,
    messageId: string | null,
    selectedText: string,
    note: string,
    tagNames: string[],
  ) =>
    invoke<FavoriteRow>("create_favorite", {
      conversationId,
      messageId,
      selectedText,
      note,
      tagNames,
    }),
  listFavorites: (tagId: number | null) => invoke<FavoriteRow[]>("list_favorites", { tagId }),
  deleteFavorite: (id: string) => invoke<void>("delete_favorite", { id }),
  listTags: () => invoke<TagRow[]>("list_tags"),
  createTag: (name: string, color: string | null) => invoke<TagRow>("create_tag", { name, color }),
  setFavoriteTags: (favoriteId: string, tagNames: string[]) =>
    invoke<TagRow[]>("set_favorite_tags", { favoriteId, tagNames }),
  suggestTags: (text: string) => invoke<string[]>("suggest_tags", { text }),
  autoOrganizeFavorites: () => invoke<number>("auto_organize_favorites"),
  testEmbedConnection: (apiUrl: string, model: string, apiKey: string) =>
    invoke<string>("test_embed_connection", { apiUrl, model, apiKey }),
  getSetting: (key: string) => invoke<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  getEmbeddingStats: () => invoke<EmbeddingStats>("get_embedding_stats"),
  generateEmbeddings: (apiUrl: string, model: string, apiKey: string) =>
    invoke<EmbeddingStats>("generate_embeddings", { apiUrl, model, apiKey }),
  semanticSearch: (
    query: string,
    apiUrl: string,
    model: string,
    apiKey: string,
    limit: number,
    filter: StructuralFilter = {},
  ) =>
    invoke<SearchHit[]>("semantic_search", {
      query,
      apiUrl,
      model,
      apiKey,
      limit,
      platform: filter.platform ?? "",
      dateFrom: filter.dateFrom ?? "",
      dateTo: filter.dateTo ?? "",
      minMessages: filter.minMessages ?? 0,
      maxMessages: filter.maxMessages ?? 0,
      modelFilter: filter.model ?? "",
      role: filter.role ?? "",
    }),
  launchResumeTerminal: (command: string, cwd?: string) =>
    invoke<void>("launch_resume_terminal", { command, cwd }),
  autoSyncStatus: () => invoke<AutoSyncStatus>("autosync_status"),
  autoSyncSetEnabled: (enabled: boolean) => invoke<AutoSyncStatus>("autosync_set_enabled", { enabled }),
  autoSyncSetInterval: (minutes: number) => invoke<AutoSyncStatus>("autosync_set_interval", { minutes }),
  autoSyncRunNow: () => invoke<AutoSyncStatus>("autosync_run_now"),
  mcpStatus: () => invoke<McpStatus>("mcp_status"),
  mcpSetEnabled: (enabled: boolean) => invoke<McpStatus>("mcp_set_enabled", { enabled }),
  mcpSetPort: (port: number) => invoke<McpStatus>("mcp_set_port", { port }),
  mcpRegenerateToken: () => invoke<McpStatus>("mcp_regenerate_token"),
  mcpOpenEnrollment: () => invoke<McpStatus>("mcp_open_enrollment"),
  mcpCancelEnrollment: () => invoke<McpStatus>("mcp_cancel_enrollment"),
  mcpActivity: () => invoke<McpActivityEntry[]>("mcp_activity"),
};
