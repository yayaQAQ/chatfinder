use serde::Serialize;

#[derive(Debug, Clone)]
pub struct NormalizedMessage {
    pub id: String,
    pub sender: String, // "human" | "assistant" | "meta" (CLI-injected local content, not human-typed)
    pub text: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NormalizedConversation {
    pub id: String,
    pub platform: String, // "claude" | "chatgpt"
    pub title: String,
    pub summary: String,
    pub url: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub messages: Vec<NormalizedMessage>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ConversationSummary {
    pub id: String,
    pub platform: String,
    pub title: String,
    pub summary: String,
    pub url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub message_count: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct MessageRow {
    pub id: String,
    pub conversation_id: String,
    pub sender: String,
    pub text: String,
    pub created_at: Option<String>,
    pub seq: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TagRow {
    pub id: i64,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct FavoriteRow {
    pub id: String,
    pub conversation_id: String,
    pub conversation_title: String,
    pub message_id: Option<String>,
    pub selected_text: String,
    pub note: String,
    pub created_at: String,
    pub tags: Vec<TagRow>,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct ImportSummary {
    pub platform: String,
    pub added: i64,
    pub updated: i64,
    pub skipped: i64,
    pub total_in_file: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct ImportBatchRow {
    pub id: String,
    pub source_file: String,
    pub platform: String,
    pub imported_at: String,
    pub added_count: i64,
    pub updated_count: i64,
    pub skipped_count: i64,
    /// Conversations still tagged with this batch — may be lower than
    /// added_count+updated_count if some were deleted individually since,
    /// or reassigned to a later batch by a reimport.
    pub remaining_conversations: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchHit {
    pub conversation_id: String,
    pub conversation_title: String,
    pub kind: String, // "title" | "message" | "semantic:<score>"
    pub ref_id: String,
    pub snippet: String,
    pub platform: String,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct EmbeddingStats {
    pub total_messages: i64,
    pub indexed_messages: i64,
    pub model: Option<String>,
}
