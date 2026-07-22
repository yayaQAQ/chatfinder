# ChatFinder

[中文](README.zh-CN.md) | English

A cross-platform desktop app (Tauri + React) for importing, browsing, and searching your AI chat history from **Claude**, **ChatGPT**, **DeepSeek**, and local coding agents (**Claude Code**, **Codex CLI**) — all stored locally in a single SQLite database.

## Features

- **Multi-source import** — drop in an official export ZIP and it auto-detects the platform:
  - Claude (`conversations.json` with `chat_messages`)
  - ChatGPT (`conversations.json` / split `conversations-NNN.json`, including inline image attachments)
  - DeepSeek (`conversations.json` with `mapping` + `fragments`, including model "thinking" blocks)
- **Local agent session import** — scans `~/.claude/projects` and `~/.codex/sessions` for Claude Code / Codex CLI transcripts and imports them as conversations too.
- **Full-text search** across all conversations and messages (SQLite FTS5 + BM25 ranking), with filters by platform, date range, message count, and sender (user input only / AI replies only).
- **Semantic search** — generate embeddings via any OpenAI-compatible `/v1/embeddings` endpoint and search by meaning, not just keywords.
- **Favorites** — save message snippets with notes and auto-suggested/custom tags; browse and search your favorites separately.
- **Conversation viewer** with Markdown + syntax-highlighted code rendering.
- **Command palette** for quick navigation.
- **Incremental, idempotent imports** — re-importing the same export only adds new/changed conversations (content-hash based dedup), so you can safely import newer exports over time.

## Tech stack

- **Frontend:** React 19 + TypeScript, Vite, Tailwind CSS, Zustand, React Router, react-markdown
- **Backend:** Rust + Tauri 2, `rusqlite` (bundled SQLite with FTS5), `reqwest` for embedding API calls
- **Storage:** a single SQLite database in the OS app-data directory (`chatvault.db`)

## Project structure

```
app/
├── src/                    # React frontend
│   ├── pages/               # Conversations, ConversationDetail, Favorites, JsonViewer
│   ├── components/          # Sidebar, ImportDialog, AgentScanDialog, CommandPalette, EmbedSettings, ...
│   └── lib/                 # Tauri API bindings, markdown/highlight helpers, search history
└── src-tauri/               # Rust backend
    └── src/
        ├── import.rs         # ZIP parsing + platform-specific normalization
        ├── agent_scan.rs     # Claude Code / Codex local session discovery + import
        ├── db.rs             # SQLite schema + migrations
        ├── embed.rs          # Embedding API client + cosine similarity search
        ├── keywords.rs       # Tag auto-suggestion
        └── commands.rs       # All Tauri IPC commands
```

## Getting started

```bash
cd app
npm install
npm run tauri dev     # launch the desktop app in dev mode
```

To build a release binary:

```bash
npm run tauri build
```

## Usage

1. Launch the app and use **Import** to select an export ZIP (from Claude, ChatGPT, or DeepSeek's "export data" feature), or use **Scan agent sessions** to pull in local Claude Code / Codex CLI history.
2. Browse and filter conversations in the sidebar, or use the search bar for full-text/semantic search.
3. Open a conversation to read it with rendered Markdown and syntax highlighting.
4. Select text to save it as a **favorite** with tags and notes for later reference.
5. (Optional) Configure an embeddings endpoint in **Settings** to enable semantic search.
