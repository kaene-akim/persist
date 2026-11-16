import { useMemo, useState } from "react";
import { useStore } from "../store";
import { Badge, Button, Card, CardHeader, EmptyState, cx } from "../components/ui";
import { absoluteDate, relativeTime } from "../lib/format";
import type { ActivityEntry, ActivityStatus } from "../lib/types";

const STATUS_TONE: Record<ActivityStatus, "moss" | "rust" | "clay" | "neutral"> = {
  success: "moss",
  failed: "rust",
  running: "clay",
  interrupted: "clay",
};

const KIND_LABEL: Record<string, string> = {
  init: "Initialised",
  clone: "Adopted",
  add_root: "Added directory",
  remove_root: "Removed directory",
  backup: "Backup",
  restore: "Restore",
  verify: "Verify",
  config: "Settings",
  auth: "Account",
  prune: "Prune",
};

export function Activity() {
  const { activity, refreshActivity, run } = useStore();
  const [filter, setFilter] = useState<"all" | ActivityStatus>("all");

  const filtered = useMemo(
    () => (filter === "all" ? activity : activity.filter((a) => a.status === filter)),
    [activity, filter],
  );

  const refresh = () =>
    run("Refreshing activity", async () => {
      await refreshActivity();
    });

  return (
    <div className="space-y-6 px-6 py-6">
      <Card>
        <CardHeader
          title="Activity log"
          hint="A local record of what Persist has done. It is a convenience, not a source of truth."
          action={
            <div className="flex items-center gap-2">
              <div className="flex border border-line">
                {(["all", "success", "failed"] as const).map((f) => (
                  <button
                    key={f}
                    onClick={() => setFilter(f)}
                    className={cx(
                      "px-2.5 py-1 text-xs transition-colors",
                      filter === f ? "bg-panel2 text-ink" : "text-muted hover:text-ink",
                    )}
                  >
                    {f === "all" ? "All" : f === "success" ? "Succeeded" : "Failed"}
                  </button>
                ))}
              </div>
              <Button variant="secondary" size="sm" onClick={refresh}>
                Refresh
              </Button>
            </div>
          }
        />
        {filtered.length === 0 ? (
          <div className="p-4">
            <EmptyState title="Nothing recorded yet">
              Backups, restores, verifications and account changes appear here as they happen.
            </EmptyState>
          </div>
        ) : (
          <ul className="divide-y divide-line">
            {filtered.map((entry) => (
              <ActivityRow key={entry.id} entry={entry} />
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

function ActivityRow({ entry }: { entry: ActivityEntry }) {
  return (
    <li className="flex items-start gap-4 px-4 py-3.5">
      <div className="w-28 shrink-0 pt-0.5">
        <div className="text-xs text-ink">{KIND_LABEL[entry.kind] ?? entry.kind}</div>
        <div className="mt-0.5 text-[11px] text-faint" title={absoluteDate(entry.timestamp)}>
          {relativeTime(entry.timestamp)}
        </div>
      </div>
      <p className="min-w-0 flex-1 text-sm leading-relaxed text-muted">{entry.message}</p>
      <Badge tone={STATUS_TONE[entry.status]}>{entry.status}</Badge>
    </li>
  );
}
