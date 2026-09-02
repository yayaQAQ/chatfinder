# ChatFinder

[中文](README.zh-CN.md) | English

A cross-platform desktop app (Tauri + React) for importing, browsing, and searching your AI chat history from **Claude**, **ChatGPT**, **DeepSeek**, and local coding agents (**Claude Code**, **Codex CLI**) — all stored locally in a single SQLite database.

## Links

- **Mac App Store** → [Download ChatFinder](https://apps.apple.com/app/chatfinder/id6796240541)
- **Website** → [chatfinder.newapiratio.com](https://chatfinder.newapiratio.com/)
- **Support & FAQ** → [chatfinder.newapiratio.com/support](https://chatfinder.newapiratio.com/support/)
- **Privacy Policy** → [chatfinder.newapiratio.com/privacy](https://chatfinder.newapiratio.com/privacy/)

## Features

- **Multi-source import** — drop in an official export ZIP and it auto-detects the platform:
  - Claude (`conversations.json` with `chat_messages`)
  - ChatGPT (`conversations.json` / split `conversations-NNN.json`, including inline image attachments)
  - DeepSeek (`conversations.json` with `mapping` + `fragments`, including model "thinking" blocks)
- **Local agent session import** — scans `~/.claude/projects` and `~/.codex/sessions` for Claude Code / Codex CLI transcripts and imports them as conversations too, keeping tool calls, tool results and thinking blocks as separately filterable content.
- **Resume in the terminal** — from a Claude Code / Codex session, open a terminal in that session's original working directory and run `claude --resume <id>` / `codex resume <id>`. Choose Terminal.app or iTerm2, and preset a proxy and extra CLI arguments. (macOS only)
- **Model tracking and filtering** — records which model produced each reply (Claude Code, Codex, ChatGPT and DeepSeek exports all carry it), so a session that switched models mid-way shows exactly where, and the whole library can be filtered by model.
- **Grouped by project directory** — sessions sharing a working directory are collected together regardless of which tool produced them, with full-text search scoped to that directory.
- **Full-text search** across all conversations and messages (SQLite FTS5 + BM25 ranking), with filters by platform, model, date range, message count, and sender (user input only / AI replies only).
- **Semantic search** — generate embeddings via any OpenAI-compatible `/v1/embeddings` endpoint and search by meaning, not just keywords.
- **Favorites** — save message snippets (Markdown preserved) with notes and auto-suggested/custom tags; browse and search your favorites separately.
- **Conversation viewer** with Markdown + syntax-highlighted code rendering, plus in-conversation search that steps through and highlights each match.
- **Command palette** (`⌘K`) for quick navigation.
- **JSON viewer** — drop in any `.json` file to browse it without importing.
- **Import management** — every import is recorded as a batch that can be rolled back as a whole, and single conversations can be deleted (messages, favorites, index rows and embeddings all cleaned up).
- **Incremental, idempotent imports** — re-importing the same export only adds new/changed conversations (content-hash based dedup), so you can safely import newer exports over time.
- **Bilingual UI** — Chinese and English, switchable at any time.

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
│   └── lib/                 # Tauri API bindings, markdown/highlight helpers, search history,
│                            #   model display names, build-variant flag (brand.ts)
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

### Build variants

The Mac App Store build cannot carry the ChatGPT brand (App Store Review
Guideline 5), so it shows a neutral "AI chat" name and omits the OpenAI
embedding preset. It is the same code behind a build-time flag — see
`src/lib/brand.ts` — not a separate branch:

```bash
npm run tauri build                # public build
npm run tauri:build:appstore       # App Store build (VITE_APP_STORE=1)
```

## Usage

1. Launch the app and use **Import** to select an export ZIP (from Claude, ChatGPT, or DeepSeek's "export data" feature), or use **Scan agent sessions** to pull in local Claude Code / Codex CLI history.
2. Browse and filter conversations in the sidebar, or use the search bar for full-text/semantic search.
3. Open a conversation to read it with rendered Markdown and syntax highlighting.
4. Select text to save it as a **favorite** with tags and notes for later reference.
5. (Optional) Configure an embeddings endpoint in **Settings** to enable semantic search.
