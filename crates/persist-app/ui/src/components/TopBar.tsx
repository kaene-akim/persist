import type { ReactNode } from "react";
import { useStore } from "../store";
import { Button, ProgressBar, Spinner } from "./ui";
import { humanBytes } from "../lib/format";

export function TopBar({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="flex items-center justify-between gap-4 border-b border-line bg-panel px-6 py-4">
      <div className="min-w-0">
        <h1 className="text-lg font-semibold tracking-tight text-ink">{title}</h1>
        {subtitle && <p className="mt-0.5 text-xs text-muted">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

export function OperationBar() {
  const { busy, progress, cancel } = useStore();
  if (!busy) return null;

  const pct =
    progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : null;

  return (
    <div className="flex items-center gap-4 border-b border-line bg-panel2 px-6 py-2.5">
      <Spinner className="h-4 w-4 text-moss" />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <span className="truncate text-xs font-medium text-ink">
            {progress?.message || progress?.phase || busy}
          </span>
          <span className="shrink-0 font-mono text-[11px] text-muted">
            {progress && progress.total > 0 && (
              <>
                {progress.done}/{progress.total}
                {progress.bytesTotal > 0 && ` · ${humanBytes(progress.bytesDone)}`}
                {pct !== null && ` · ${pct}%`}
              </>
            )}
          </span>
        </div>
        <div className="mt-1.5">
          <ProgressBar value={progress?.done ?? 0} max={progress?.total ?? 0} />
        </div>
      </div>
      <Button variant="ghost" size="sm" onClick={cancel}>
        Cancel
      </Button>
    </div>
  );
}
