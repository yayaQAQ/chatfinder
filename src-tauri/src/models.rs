use serde::Serialize;

#[derive(Debug, Clone)]
pub struct NormalizedMessage {
    pub id: String,
    pub sender: String, // "human" | "assistant"
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
