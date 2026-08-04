import { useEffect, useState } from "react";
import { X, Star } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../lib/api";
import { useToast } from "../lib/toast";
import { useI18n } from "../lib/i18n";
import { markdownUrlTransform } from "../lib/markdown";

export function FavoriteModal({
  conversationId,
  messageId,
  selectedText,
  onClose,
  onSaved,
}: {
  conversationId: string;
  messageId: string | null;
  selectedText: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [tagsInput, setTagsInput] = useState("");
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  const { push } = useToast();
  const { t } = useI18n();

  useEffect(() => {
    api.suggestTags(selectedText).then((tags) => setTagsInput(tags.join(", ")));
  }, [selectedText]);

  const save = async () => {
    setSaving(true);
    try {
      const tagNames = tagsInput
        .split(/[,，]/)
        .map((t) => t.trim())
        .filter(Boolean);
      await api.createFavorite(conversationId, messageId, selectedText, note, tagNames);
      push(t("favoriteModal.savedToast"), "success");
      onSaved();
      onClose();
    } catch (e) {
      push(t("favoriteModal.failToast", { error: String(e) }), "error");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div className="w-[440px] rounded-2xl bg-white p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="flex items-center gap-2 text-base font-semibold text-stone-800">
            <Star size={16} className="text-amber-500" />
            {t("favoriteModal.title")}
          </h2>
          <button onClick={onClose} className="rounded-full p-1 text-stone-400 hover:bg-stone-100">
            <X size={16} />
          </button>
        </div>

        <div className="prose prose-sm prose-stone mb-3 max-h-32 max-w-none overflow-y-auto rounded-lg bg-stone-50 p-3 text-sm text-stone-600">
          <ReactMarkdown remarkPlugins={[remarkGfm]} urlTransform={markdownUrlTransform}>
            {selectedText}
          </ReactMarkdown>
        </div>

        <label className="mb-1 block text-xs font-medium text-stone-500">{t("favoriteModal.tagsLabel")}</label>
        <input
          value={tagsInput}
          onChange={(e) => setTagsInput(e.target.value)}
          placeholder={t("favoriteModal.tagsPlaceholder")}
          className="mb-3 w-full rounded-lg border border-stone-200 px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-orange-200"
        />

        <label className="mb-1 block text-xs font-medium text-stone-500">{t("favoriteModal.noteLabel")}</label>
        <textarea
          value={note}
          onChange={(e) => setNote(e.target.value)}
          rows={2}
          className="mb-4 w-full resize-none rounded-lg border border-stone-200 px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-orange-200"
        />

        <button
          onClick={save}
          disabled={saving}
          className="w-full rounded-xl bg-stone-900 px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-stone-800 disabled:opacity-60"
        >
          {saving ? t("favoriteModal.saving") : t("favoriteModal.save")}
        </button>
      </div>
    </div>
  );
}
