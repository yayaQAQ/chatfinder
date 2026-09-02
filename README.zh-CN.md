# ChatFinder

中文 | [English](README.md)

一款跨平台桌面应用（Tauri + React），用于导入、浏览和搜索你的 AI 聊天记录，支持 **Claude**、**ChatGPT**、**DeepSeek**，以及本地编程 Agent（**Claude Code**、**Codex CLI**）——所有数据都保存在本地的单一 SQLite 数据库中，不上传任何服务器。

## 链接

- **Mac App Store** → [下载 ChatFinder](https://apps.apple.com/app/chatfinder/id6796240541)
- **官网** → [chatfinder.newapiratio.com](https://chatfinder.newapiratio.com/)
- **支持与常见问题** → [chatfinder.newapiratio.com/support](https://chatfinder.newapiratio.com/support/)
- **隐私政策** → [chatfinder.newapiratio.com/privacy](https://chatfinder.newapiratio.com/privacy/)

## 功能特性

- **多来源导入** — 直接拖入官方导出的 ZIP 包，自动识别平台：
  - Claude（`conversations.json`，含 `chat_messages`）
  - ChatGPT（`conversations.json` / 分卷的 `conversations-NNN.json`，包含内嵌图片附件）
  - DeepSeek（`conversations.json`，含 `mapping` + `fragments`，包括模型的"思考过程"）
- **本地 Agent 会话导入** — 扫描 `~/.claude/projects` 和 `~/.codex/sessions`，将 Claude Code / Codex CLI 的历史会话也一并导入为对话记录。
- **全文搜索** — 覆盖所有对话与消息（基于 SQLite FTS5 + BM25 排序），支持按平台、日期范围、消息数量、以及发送者（仅用户输入 / 仅 AI 回复）筛选。
- **语义搜索** — 通过任意兼容 OpenAI 的 `/v1/embeddings` 接口生成向量索引，按语义而非关键词进行搜索。
- **收藏夹** — 保存消息片段并添加备注、自动建议或自定义标签；可单独浏览和搜索收藏内容。
- **对话查看器** — 支持 Markdown 渲染与代码语法高亮。
- **命令面板** — 快速导航。
- **增量、幂等导入** — 重复导入同一份导出文件时，只会新增/更新有变化的对话（基于内容哈希去重），可放心地随时导入更新后的导出数据。

## 技术栈

- **前端：** React 19 + TypeScript、Vite、Tailwind CSS、Zustand、React Router、react-markdown
- **后端：** Rust + Tauri 2、`rusqlite`（内置 SQLite，支持 FTS5）、`reqwest` 用于调用嵌入向量接口
- **存储：** 单一 SQLite 数据库，位于系统应用数据目录下（`chatvault.db`）

## 项目结构

```
app/
├── src/                    # React 前端
│   ├── pages/               # Conversations、ConversationDetail、Favorites、JsonViewer
│   ├── components/          # Sidebar、ImportDialog、AgentScanDialog、CommandPalette、EmbedSettings 等
│   └── lib/                 # Tauri API 绑定、Markdown/高亮辅助函数、搜索历史
└── src-tauri/               # Rust 后端
    └── src/
        ├── import.rs         # ZIP 解析 + 各平台数据归一化
        ├── agent_scan.rs     # Claude Code / Codex 本地会话发现与导入
        ├── db.rs             # SQLite 表结构 + 迁移
        ├── embed.rs          # 嵌入向量 API 客户端 + 余弦相似度搜索
        ├── keywords.rs       # 标签自动建议
        └── commands.rs       # 所有 Tauri IPC 命令
```

## 快速开始

```bash
cd app
npm install
npm run tauri dev     # 以开发模式启动桌面应用
```

打包发布版本：

```bash
npm run tauri build
```

### 构建变体

上架 Mac App Store 的版本不能出现 ChatGPT 品牌名（App Store 审核指南 5），
因此会改用中性的「AI 对话」，并去掉 OpenAI 的嵌入预设。两个版本共用同一份代码，
靠编译期开关区分（见 `src/lib/brand.ts`），不再需要单独维护分支：

```bash
npm run tauri build                # 公开版本
npm run tauri:build:appstore       # App Store 版本（VITE_APP_STORE=1）
```

## 使用说明

1. 启动应用后，点击 **导入** 选择导出的 ZIP 文件（来自 Claude、ChatGPT 或 DeepSeek 的"导出数据"功能），或使用 **扫描 Agent 会话** 导入本地的 Claude Code / Codex CLI 历史记录。
2. 在侧边栏浏览、筛选对话，或使用搜索框进行全文/语义搜索。
3. 打开对话可查看渲染后的 Markdown 内容与代码高亮。
4. 选中文本即可保存为 **收藏**，并添加标签和备注以便日后查找。
5. （可选）在 **设置** 中配置嵌入向量接口以启用语义搜索。
