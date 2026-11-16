import { useStore } from "../store";
import { cx } from "./ui";
import { IconAlert, IconCheck, IconClose } from "./icons";

export function Toasts() {
  const { toasts, dismissToast } = useStore();

  return (
    <div className="pointer-events-none fixed bottom-5 right-5 z-50 flex w-80 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          role="status"
          className={cx(
            "pointer-events-auto flex items-start gap-3 border bg-panel px-3.5 py-3 shadow-pixel animate-slide-in",
            t.kind === "error"
              ? "border-rust/60"
              : t.kind === "success"
                ? "border-moss/60"
                : "border-line",
          )}
        >
          <span
            className={cx(
              "mt-0.5 shrink-0",
              t.kind === "error" ? "text-rust" : t.kind === "success" ? "text-moss" : "text-water",
            )}
          >
            {t.kind === "error" ? (
              <IconAlert className="h-4 w-4" />
            ) : (
              <IconCheck className="h-4 w-4" />
            )}
          </span>
          <p className="flex-1 text-xs leading-relaxed text-ink">{t.message}</p>
          <button
            onClick={() => dismissToast(t.id)}
            className="shrink-0 text-faint hover:text-ink"
            aria-label="Dismiss"
          >
            <IconClose className="h-3.5 w-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
