import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { zh } from "../locales/zh";
import { en } from "../locales/en";

export type Lang = "zh" | "en";

type Dict = typeof zh;
const DICTS: Record<Lang, Dict> = { zh, en };

// Recursively builds "a.b.c" dot-path keys for every leaf string in the dictionary,
// so t() calls are checked against the actual locale shape at compile time.
type Join<K, P> = K extends string ? (P extends string ? `${K}.${P}` : never) : never;
type Paths<T> = T extends string
  ? never
  : { [K in keyof T]: K extends string ? (T[K] extends string ? K : Join<K, Paths<T[K]>>) : never }[keyof T];
export type TranslationKey = Paths<Dict>;

const STORAGE_KEY = "chatvault:lang";

function detectDefaultLang(): Lang {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "zh" || stored === "en") return stored;
  } catch {
    // ignore
  }
  return typeof navigator !== "undefined" && navigator.language?.toLowerCase().startsWith("en") ? "en" : "zh";
}

function lookup(dict: Dict, key: string): string {
  const parts = key.split(".");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let node: any = dict;
  for (const p of parts) {
    node = node?.[p];
  }
  return typeof node === "string" ? node : key;
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{\{(\w+)\}\}/g, (_, k) => (k in vars ? String(vars[k]) : `{{${k}}}`));
}

interface I18nContextValue {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: (key: TranslationKey, vars?: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectDefaultLang);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // ignore write failures (e.g. storage disabled)
    }
  }, []);

  const t = useCallback(
    (key: TranslationKey, vars?: Record<string, string | number>) => interpolate(lookup(DICTS[lang], key), vars),
    [lang],
  );

  const value = useMemo(() => ({ lang, setLang, t }), [lang, setLang, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used within I18nProvider");
  return ctx;
}

function localeTag(lang: Lang) {
  return lang === "zh" ? "zh-CN" : "en-US";
}

// Shared "today / yesterday / N days ago / short date / full date" formatting
// used by Conversations and Favorites cards.
export function formatRelativeDate(iso: string | null, lang: Lang, t: I18nContextValue["t"]): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const diffDays = Math.floor((Date.now() - d.getTime()) / 86400000);
  if (diffDays === 0) return t("common.today");
  if (diffDays === 1) return t("common.yesterday");
  if (diffDays < 7) return t("common.daysAgo", { n: diffDays });
  if (diffDays < 365) return d.toLocaleDateString(localeTag(lang), { month: "numeric", day: "numeric" });
  return d.toLocaleDateString(localeTag(lang), { year: "numeric", month: "numeric", day: "numeric" });
}

// Compact variant (no distinct "yesterday") used by CommandPalette.
export function formatCompactRelativeDate(iso: string | null, lang: Lang, t: I18nContextValue["t"]): string {
  if (!iso) return "";
  const d = new Date(iso);
  const diffDays = Math.floor((Date.now() - d.getTime()) / 86400000);
  if (diffDays === 0) return t("common.today");
  if (diffDays < 7) return t("common.daysAgo", { n: diffDays });
  return d.toLocaleDateString(localeTag(lang), { month: "numeric", day: "numeric" });
}

export function formatLongDate(iso: string | null, lang: Lang): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return d.toLocaleDateString(localeTag(lang), { year: "numeric", month: "long", day: "numeric" });
}

export function formatDateTime(iso: string, lang: Lang): string {
  const d = new Date(iso);
  return d.toLocaleString(localeTag(lang), { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
