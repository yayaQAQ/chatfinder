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
}

export interface MessageRow {
  id: string;
  conversation_id: string;
  sender: string;
  text: string;
  created_at: string | null;
  seq: number;
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

export interface EmbeddingStats {
  total_messages: number;
  indexed_messages: number;
  model: string | null;
}

export interface EmbedConfig {
  apiUrl: string;
  model: string;
  apiKey: string;
}

export interface AgentSource {
  tool: string;          // "claude-code" | "codex"
  label: string;         // "Claude Code" | "Codex"
  dir: string;           // absolute directory scanned
  session_count: number;
  message_count: number;
}

export type SortOption = "newest" | "oldest" | "most_messages" | "fewest_messages";

export interface StructuralFilter {
  platform?: string;
  dateFrom?: string;
  dateTo?: string;
  minMessages?: number;
  maxMessages?: number;
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
  scanAgentSources: () => invoke<AgentSource[]>("scan_agent_sources"),
  importAgentSessions: (tools: string[], onProgress: (p: ImportProgress) => void) => {
    const channel = new Channel<ImportProgress>();
    channel.onmessage = onProgress;
    return invoke<ImportSummary>("import_agent_sessions", { tools, onProgress: channel });
  },
  listConversations: (filter: ConversationFilter, limit: number, offset: number) =>
    invoke<ConversationSummary[]>("list_conversations", {
      query: "",
      platform: filter.platform ?? "",
      dateFrom: filter.dateFrom ?? "",
      dateTo: filter.dateTo ?? "",
      minMessages: filter.minMessages ?? 0,
      maxMessages: filter.maxMessages ?? 0,
      sort: filter.sort ?? "newest",
      limit,
      offset,
    }),
  countConversations: () => invoke<number>("count_conversations"),
  getConversation: (id: string) =>
    invoke<[ConversationSummary, MessageRow[]]>("get_conversation", { id }),
  searchAll: (query: string, filter: StructuralFilter = {}) =>
    invoke<SearchHit[]>("search_all", {
      query,
      platform: filter.platform ?? "",
      dateFrom: filter.dateFrom ?? "",
      dateTo: filter.dateTo ?? "",
      minMessages: filter.minMessages ?? 0,
      maxMessages: filter.maxMessages ?? 0,
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
    }),
};
