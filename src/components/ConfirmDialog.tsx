import { useEffect } from "react";
import { AlertTriangle } from "lucide-react";

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  cancelLabel,
  danger = true,
  busy = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    const fn = (e: KeyboardEvent) => { if (e.key === "Escape") onCancel(); };
    window.addEventListener("keydown", fn);
    return () => window.removeEventListener("keydown", fn);
  }, [onCancel]);

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30 backdrop-blur-sm"
      onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}
    >
      <div className="w-[380px] rounded-2xl bg-white p-5 shadow-2xl">
        <div className="mb-3 flex items-center gap-2.5">
          <div
            className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full ${
              danger ? "bg-red-50 text-red-500" : "bg-stone-100 text-stone-500"
            }`}
          >
            <AlertTriangle size={17} />
          </div>
          <h2 className="text-base font-semibold text-stone-800">{title}</h2>
        </div>
        <p className="mb-5 whitespace-pre-line text-sm leading-relaxed text-stone-600">{message}</p>
        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            disabled={busy}
            className="rounded-xl border border-stone-200 px-4 py-2 text-sm text-stone-600 transition-colors hover:bg-stone-50 disabled:opacity-60"
          >
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            className={`rounded-xl px-4 py-2 text-sm font-medium text-white transition-colors disabled:opacity-60 ${
              danger ? "bg-red-600 hover:bg-red-700" : "bg-stone-900 hover:bg-stone-800"
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
