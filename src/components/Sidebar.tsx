import { NavLink } from "react-router-dom";
import { MessageSquareText, Star, UploadCloud, FileJson, Search, BrainCircuit, FolderSearch } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import appLogo from "../assets/app-logo.png";

const navItem = ({ isActive }: { isActive: boolean }) =>
  `flex items-center gap-2.5 rounded-xl px-3 py-2.5 text-sm font-medium transition-colors ${
    isActive
      ? "bg-orange-50 text-orange-800 ring-1 ring-orange-200"
      : "text-stone-600 hover:bg-stone-100 hover:text-stone-900"
  }`;

interface Props {
  onImportClick: () => void;
  onSearchClick: () => void;
  onSettingsClick: () => void;
  onAgentScanClick: () => void;
  refreshKey: number;
  embedIndexed?: number;
}

export function Sidebar({ onImportClick, onSearchClick, onSettingsClick, onAgentScanClick, refreshKey, embedIndexed = 0 }: Props) {
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    api.countConversations().then(setCount).catch(() => setCount(0));
  }, [refreshKey]);

  return (
    <aside className="flex h-full w-56 flex-col border-r border-stone-200 bg-white px-3 py-4 gap-1">
      {/* Logo */}
      <div className="mb-4 flex items-center gap-2.5 px-2 py-1">
        <img src={appLogo} alt="ChatFinder" className="h-8 w-8 rounded-xl shadow-sm" />
        <span className="text-base font-semibold tracking-tight text-stone-800">ChatFinder</span>
      </div>

      {/* Quick search */}
      <button
        onClick={onSearchClick}
        className="flex items-center gap-2 rounded-xl border border-stone-200 bg-stone-50 px-3 py-2 text-sm text-stone-500 transition-colors hover:bg-white hover:text-stone-700 mb-2"
      >
        <Search size={14} className="shrink-0" />
        <span className="flex-1 text-left text-xs">搜索对话…</span>
        <kbd className="rounded border border-stone-200 bg-white px-1.5 py-0.5 text-[10px] text-stone-400">⌘K</kbd>
      </button>

      {/* Nav */}
      <nav className="flex flex-1 flex-col gap-0.5">
        <div className="mb-1 px-2 text-[10px] font-semibold uppercase tracking-widest text-stone-400">对话</div>
        <NavLink to="/" className={navItem} end>
          <MessageSquareText size={15} />
          全部对话
          {count !== null && (
            <span className="ml-auto rounded-full bg-stone-100 px-2 py-0.5 text-[11px] font-normal tabular-nums text-stone-500">
              {count >= 1000 ? `${(count / 1000).toFixed(1)}k` : count}
            </span>
          )}
        </NavLink>
        <NavLink to="/favorites" className={navItem}>
          <Star size={15} />
          收藏夹
        </NavLink>

        <div className="mb-1 mt-3 px-2 text-[10px] font-semibold uppercase tracking-widest text-stone-400">工具</div>
        <NavLink to="/json" className={navItem}>
          <FileJson size={15} />
          JSON 查看器
        </NavLink>
        <button onClick={onAgentScanClick} className={`flex w-full items-center gap-2.5 rounded-xl px-3 py-2.5 text-sm font-medium transition-colors text-stone-600 hover:bg-stone-100 hover:text-stone-900`}>
          <FolderSearch size={15} />
          本地 Agent 会话
        </button>
        <button onClick={onSettingsClick} className={`flex w-full items-center gap-2.5 rounded-xl px-3 py-2.5 text-sm font-medium transition-colors text-stone-600 hover:bg-stone-100 hover:text-stone-900`}>
          <BrainCircuit size={15} />
          语义搜索
          {embedIndexed > 0 && (
            <span className="ml-auto rounded-full bg-violet-100 px-2 py-0.5 text-[11px] font-normal text-violet-600">
              {embedIndexed >= 1000 ? `${(embedIndexed / 1000).toFixed(1)}k` : embedIndexed}
            </span>
          )}
        </button>
      </nav>

      {/* Import button */}
      <div className="space-y-2 pt-2">
        <div className="flex items-center justify-between px-1">
          <span className="text-[10px] text-stone-400">拖入 .zip 或 .json 快速导入</span>
        </div>
        <button
          onClick={onImportClick}
          className="flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-orange-500 to-rose-500 px-3 py-2.5 text-sm font-medium text-white shadow-sm transition-opacity hover:opacity-90"
        >
          <UploadCloud size={15} />
          导入对话数据
        </button>
        <p className="text-center text-[10px] text-stone-400">
          <kbd className="rounded border border-stone-200 bg-stone-50 px-1 py-0.5">⌘I</kbd> 快捷键
        </p>
      </div>
    </aside>
  );
}
