# ChatFinder

[中文](README.zh-CN.md) | English

A cross-platform desktop app (Tauri + React) for importing, browsing, and searching your AI chat history from **Claude**, **ChatGPT**, **DeepSeek**, and local coding agents (**Claude Code**, **Codex CLI**) — all stored locally in a single SQLite database.

## Links

- **Mac App Store** → [Download ChatFinder](https://apps.apple.com/app/chatfinder/id6796240541)
- **Downloads (macOS / Windows / Linux)** → [GitHub Releases](https://github.com/yayaQAQ/chatfinder/releases/latest)
- **Website** → [chatfinder.newapiratio.com](https://chatfinder.newapiratio.com/)
- **Support & FAQ** → [chatfinder.newapiratio.com/support](https://chatfinder.newapiratio.com/support/)
- **Privacy Policy** → [chatfinder.newapiratio.com/privacy](https://chatfinder.newapiratio.com/privacy/)

## Features

- **Multi-source import** — drop in an official export ZIP and it auto-detects the platform:
  - Claude (`conversations.json` with `chat_messages`)
  - ChatGPT (`conversations.json` / split `conversations-NNN.json`, including inline image attachments)
  - DeepSeek (`conversations.json` with `mapping` + `fragments`, including model "thinking" blocks)
- **Local agent session import** — scans `~/.claude/projects` and `~/.codex/sessions` for Claude Code / Codex CLI transcripts and imports them as conversations too, keeping tool calls, tool results and thinking blocks as separately filterable content.
- **Resume in the terminal** — from a Claude Code / Codex session, open a terminal in that session's original working directory and run `claude --resume <id>` / `codex resume <id>`, with a proxy and extra CLI arguments preset. Works on all three platforms: Terminal.app or iTerm2 on macOS, Windows Terminal / PowerShell / cmd on Windows, and whichever of gnome-terminal, konsole, kitty and friends is installed on Linux. iTerm2 and Windows Terminal can open a tab in the window you already have open instead of a new one, and a copy button hands you the same command — spelled for the shell that will run it — if you would rather paste it yourself.
- **MCP server** — a read-only, loopback-only HTTP MCP endpoint that lets local AI agents search this archive themselves: scope by working directory, full-text search, read a conversation, or pull up another Claude Code / Codex session by its uuid to see what it did. One-paste registration snippets for Claude Code, Codex and generic clients, with a self-service enrollment window so the token never lands in a transcript.
- **Automatic agent sync** — rescans local agent sessions on a timer, so work you finish in a terminal turns up in the archive without a manual scan.
- **Model tracking and filtering** — records which model produced each reply (Claude Code, Codex, ChatGPT and DeepSeek exports all carry it), so a session that switched models mid-way shows exactly where, and the whole library can be filtered by model.
- **Grouped by project directory** — sessions sharing a working directory are collected together regardless of which tool produced them, with full-text search scoped to that directory.
- **Full-text search** across all conversations and messages (SQLite FTS5 + BM25 ranking), with filters by platform, model, date range, message count, and sender (user input only / AI replies only).
- **Semantic search** — generate embeddings via any OpenAI-compatible `/v1/embeddings` endpoint and search by meaning, not just keywords.
- **Favorites** — save message snippets (Markdown preserved) with notes and auto-suggested/custom tags; browse and search your favorites separately.
- **Conversation viewer** with Markdown + syntax-highlighted code rendering, plus in-conversation search that steps through and highlights each match.
- **Command palette** (`⌘K` / `Ctrl+K`) for quick navigation.
- **JSON viewer** — drop in any `.json` file to browse it without importing.
- **Import management** — every import is recorded as a batch that can be rolled back as a whole, and single conversations can be deleted (messages, favorites, index rows and embeddings all cleaned up).
- **Incremental, idempotent imports** — re-importing the same export only adds new/changed conversations (content-hash based dedup), so you can safely import newer exports over time.
- **Bilingual UI** — Chinese and English, switchable at any time.

## Screenshots

| Favorites | Local agent sessions |
| --- | --- |
| ![Favorites with tags and notes](docs/screenshots/favorites.jpg) | ![Import Claude Code / Codex sessions with auto sync](docs/screenshots/agent-sessions.jpg) |
| **MCP server** | **Import chat history** |
| ![Read-only MCP server for local AI agents](docs/screenshots/mcp-server.jpg) | ![Import Claude / ChatGPT / DeepSeek export ZIP](docs/screenshots/import.jpg) |

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
        ├── autosync.rs       # Timed rescan of local agent sessions
        ├── mcp.rs            # Loopback MCP server exposing the archive to agents
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

## Community

Questions, suggestions, or just want to see how others use it — join the QQ group **534908951** ([join link](https://qm.qq.com/q/9RDpzJrF4s)):

<img src="docs/qq-group.jpg" width="240" alt="ChatFinder QQ group QR code">

## Support this project

ChatFinder is open source — build it yourself and you get the whole app, with nothing held back.

If you find it useful and would like to give something back, you can buy a copy on the [Mac App Store](https://apps.apple.com/app/chatfinder/id6796240541). Think of it as the "buy me a coffee" button, with the build step taken care of for you. A star on the repo is just as welcome.
