import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../store";
import { Badge, Button, Card, CardHeader, EmptyState, Stat, StatusDot } from "../components/ui";
import { IconRefresh } from "../components/icons";
import { absoluteDate, humanBytes, humanCount, relativeTime, snapshotLabel } from "../lib/format";
import type { ChangeSet } from "../lib/types";

export function Dashboard() {
  const {
    roots,
    snapshots,
    activity,
    storage,
    run,
    toast,
    refreshAll,
    refreshStorage,
    setPage,
  } = useStore();
  const [pending, setPending] = useState<ChangeSet | null>(null);

  const loadPending = useCallback(async () => {
    try {
      setPending(await api.diffLocal(null));
    } catch {
      setPending(null);
    }
  }, []);

  useEffect(() => {
    void loadPending();
  }, [loadPending, snapshots.length, roots.length]);

  const latest = snapshots[0];
  const usage = storage?.usage;

  const backupNow = () =>
    run("Backing up", async () => {
      const result = await api.runBackup(null, false, null);
      toast(
        "success",
        `Snapshot created — ${humanCount(result.files_backed_up)} files, ${humanBytes(result.new_bytes)} uploaded.`,
      );
      await refreshAll();
      await refreshStorage();
    });

  const pendingCount = pending
    ? pending.added + pending.modified + pending.deleted
    : 0;

  return (
    <div className="space-y-6 px-6 py-6">
      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <Card>
          <Stat label="Protected directories" value={humanCount(roots.length)} />
        </Card>
        <Card>
          <Stat label="Snapshots" value={humanCount(snapshots.length)} />
        </Card>
        <Card>
          <Stat
            label="Latest snapshot"
            value={latest ? snapshotLabel(latest.snapshot_id) : "—"}
            sub={latest ? relativeTime(latest.created_at) : "no backups yet"}
            tone="moss"
          />
        </Card>
        <Card>
          <Stat
            label="Stored in repository"
            value={usage ? humanBytes(usage.physical_bytes) : "—"}
            sub={usage ? `${humanCount(usage.object_count)} objects` : undefined}
          />
        </Card>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader
            title="Backup status"
            hint={
              latest
                ? `Last backup ${relativeTime(latest.created_at)}`
                : "Nothing has been backed up yet."
            }
            action={
              <div className="flex gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void loadPending()}
                  aria-label="Refresh status"
                >
                  <IconRefresh className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={backupNow}
                  disabled={roots.length === 0}
                >
                  Back up now
                </Button>
              </div>
            }
          />
          <div className="px-4 py-4">
            {roots.length === 0 ? (
              <EmptyState
                title="No directories are protected yet"
                action={
                  <Button variant="secondary" size="sm" onClick={() => setPage("backups")}>
                    Add a directory
                  </Button>
                }
              >
                Add a folder and Persist will track every change to it from now on.
              </EmptyState>
            ) : (
              <div className="space-y-4">
                <div className="flex flex-wrap items-center gap-3 text-sm">
                  {pendingCount === 0 ? (
                    <span className="flex items-center gap-2 text-muted">
                      <StatusDot tone="moss" />
                      Everything is backed up — no changes since the last snapshot.
                    </span>
                  ) : (
                    <>
                      <span className="flex items-center gap-2 text-muted">
                        <StatusDot tone="clay" />
                        {humanCount(pendingCount)} change{pendingCount === 1 ? "" : "s"} to back up
                      </span>
                      {pending && (
                        <span className="flex gap-2">
                          {pending.added > 0 && <Badge tone="moss">+{pending.added} added</Badge>}
                          {pending.modified > 0 && (
                            <Badge tone="clay">~{pending.modified} modified</Badge>
                          )}
                          {pending.deleted > 0 && (
                            <Badge tone="rust">−{pending.deleted} deleted</Badge>
                          )}
                        </span>
                      )}
                    </>
                  )}
                </div>
                {latest && (
                  <dl className="grid grid-cols-2 gap-x-6 gap-y-2 border-t border-line pt-4 text-sm sm:grid-cols-4">
                    <Meta label="Files" value={humanCount(latest.stats.file_count)} />
                    <Meta label="Logical size" value={humanBytes(latest.stats.logical_size)} />
                    <Meta label="Snapshot" value={snapshotLabel(latest.snapshot_id)} mono />
                    <Meta label="Taken" value={absoluteDate(latest.created_at)} />
                  </dl>
                )}
              </div>
            )}
          </div>
        </Card>

        <Card>
          <CardHeader
            title="Recent activity"
            action={
              <Button variant="ghost" size="sm" onClick={() => setPage("activity")}>
                View all
              </Button>
            }
          />
          <ul className="divide-y divide-line">
            {activity.slice(0, 6).map((a) => (
              <li key={a.id} className="flex items-start gap-3 px-4 py-3">
                <span className="mt-1.5">
                  <StatusDot
                    tone={
                      a.status === "success" ? "moss" : a.status === "failed" ? "rust" : "clay"
                    }
                  />
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-xs text-ink">{a.message}</p>
                  <p className="mt-0.5 text-[11px] text-faint">{relativeTime(a.timestamp)}</p>
                </div>
              </li>
            ))}
            {activity.length === 0 && (
              <li className="px-4 py-6 text-center text-xs text-faint">No activity yet.</li>
            )}
          </ul>
        </Card>
      </div>
    </div>
  );
}

function Meta({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <dt className="text-xs text-faint">{label}</dt>
      <dd className={mono ? "mt-0.5 font-mono text-xs text-ink" : "mt-0.5 text-ink"}>{value}</dd>
    </div>
  );
}
