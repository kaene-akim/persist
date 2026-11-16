import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../store";
import { Badge, Button, Card, CardHeader, EmptyState, cx } from "../components/ui";
import { absoluteDate, humanBytes, humanCount, relativeTime, snapshotLabel } from "../lib/format";
import type { ChangeSet, Snapshot } from "../lib/types";

export function Snapshots() {
  const { snapshots, refreshSnapshots, setPage, setRestoreTarget, run, toast } = useStore();
  const [selected, setSelected] = useState<string | null>(null);
  const [detail, setDetail] = useState<Snapshot | null>(null);
  const [diff, setDiff] = useState<ChangeSet | null>(null);

  useEffect(() => {
    if (!selected && snapshots.length > 0) setSelected(snapshots[0].snapshot_id);
  }, [snapshots, selected]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!selected) {
        setDetail(null);
        setDiff(null);
        return;
      }
      try {
        const snap = await api.getSnapshot(selected);
        if (cancelled) return;
        setDetail(snap);
        if (snap?.parent) {
          setDiff(await api.diffSnapshots(snap.parent, snap.snapshot_id));
        } else {
          setDiff(null);
        }
      } catch {
        if (!cancelled) {
          setDetail(null);
          setDiff(null);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selected]);

  const refresh = () =>
    run("Refreshing history", async () => {
      await refreshSnapshots();
      toast("success", "Snapshot history refreshed from the repository.");
    });

  if (snapshots.length === 0) {
    return (
      <div className="px-6 py-6">
        <Card>
          <div className="p-4">
            <EmptyState
              title="No snapshots yet"
              action={
                <Button variant="secondary" size="sm" onClick={() => setPage("backups")}>
                  Go to My Backups
                </Button>
              }
            >
              A snapshot is an immutable record of everything Persist protected at one moment.
              Create the first one from the dashboard or a directory.
            </EmptyState>
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0">
      <div className="flex w-80 shrink-0 flex-col border-r border-line">
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <span className="text-xs text-muted">
            {snapshots.length} snapshot{snapshots.length === 1 ? "" : "s"}
          </span>
          <Button variant="ghost" size="sm" onClick={refresh}>
            Refresh
          </Button>
        </div>
        <ul className="min-h-0 flex-1 overflow-y-auto">
          {snapshots.map((s) => {
            const active = s.snapshot_id === selected;
            return (
              <li key={s.snapshot_id}>
                <button
                  onClick={() => setSelected(s.snapshot_id)}
                  className={cx(
                    "w-full border-b border-line px-4 py-3 text-left transition-colors",
                    active ? "bg-panel2" : "hover:bg-panel",
                  )}
                >
                  <div className="flex items-center justify-between">
                    <span className={cx("font-mono text-xs", active ? "text-moss" : "text-ink")}>
                      {snapshotLabel(s.snapshot_id)}
                    </span>
                    {!s.parent && <Badge tone="water">initial</Badge>}
                  </div>
                  <div className="mt-1 text-[11px] text-faint">{relativeTime(s.created_at)}</div>
                  <div className="mt-1 flex gap-2 text-[11px] text-muted">
                    <span>{humanCount(s.stats.file_count)} files</span>
                    <span>·</span>
                    <span>{humanBytes(s.stats.logical_size)}</span>
                  </div>
                </button>
              </li>
            );
          })}
        </ul>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
        {detail ? (
          <div className="space-y-6">
            <Card>
              <CardHeader
                title={snapshotLabel(detail.snapshot_id)}
                hint={`Created ${absoluteDate(detail.created_at)}`}
                action={
                  <Button
                    variant="primary"
                    size="sm"
                    onClick={() => {
                      setRestoreTarget(detail.snapshot_id);
                      setPage("restore");
                    }}
                  >
                    Restore this snapshot
                  </Button>
                }
              />
              <div className="grid grid-cols-2 gap-x-6 gap-y-4 px-4 py-4 sm:grid-cols-4">
                <Metric label="Files" value={humanCount(detail.stats.file_count)} />
                <Metric label="Directories" value={humanCount(detail.stats.dir_count)} />
                <Metric label="Logical size" value={humanBytes(detail.stats.logical_size)} />
                <Metric
                  label="New this snapshot"
                  value={`${humanCount(detail.stats.new_objects)} obj · ${humanBytes(detail.stats.new_bytes)}`}
                />
              </div>
              {detail.message && (
                <p className="border-t border-line px-4 py-3 text-sm text-muted">{detail.message}</p>
              )}
            </Card>

            {diff && (
              <Card>
                <CardHeader
                  title="Changes since the previous snapshot"
                  hint={diff.changes.length === 0 ? "No path-level changes recorded." : undefined}
                />
                <div className="flex gap-2 border-b border-line px-4 py-3">
                  <Badge tone="moss">{diff.added} added</Badge>
                  <Badge tone="clay">{diff.modified} modified</Badge>
                  <Badge tone="rust">{diff.deleted} deleted</Badge>
                </div>
                {diff.changes.length > 0 && (
                  <ul className="max-h-64 divide-y divide-line overflow-y-auto">
                    {diff.changes.slice(0, 200).map((c, i) => (
                      <li
                        key={`${c.root_id}:${c.path}:${i}`}
                        className="flex items-center gap-3 px-4 py-2 text-xs"
                      >
                        <ChangeBadge kind={c.kind} />
                        <span className="truncate font-mono text-muted" title={c.path}>
                          {c.root_name}/{c.path}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </Card>
            )}

            <Card>
              <CardHeader title="Contents by directory" />
              <ul className="divide-y divide-line">
                {detail.roots.map((root) => {
                  const files = root.entries.filter((e) => e.kind === "file").length;
                  const size = root.entries.reduce((n, e) => n + e.size, 0);
                  return (
                    <li key={root.root_id} className="flex items-center justify-between px-4 py-3">
                      <div className="min-w-0">
                        <p className="text-sm text-ink">{root.name}</p>
                        <p className="truncate font-mono text-[11px] text-faint">
                          {root.remote_path}
                        </p>
                      </div>
                      <span className="shrink-0 text-xs text-muted">
                        {humanCount(files)} files · {humanBytes(size)}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </Card>
          </div>
        ) : (
          <div className="grid h-full place-items-center text-sm text-faint">
            Select a snapshot to inspect it.
          </div>
        )}
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs text-faint">{label}</div>
      <div className="mt-0.5 text-sm text-ink">{value}</div>
    </div>
  );
}

function ChangeBadge({ kind }: { kind: string }) {
  const map: Record<string, { tone: "moss" | "clay" | "rust"; sign: string }> = {
    added: { tone: "moss", sign: "+" },
    modified: { tone: "clay", sign: "~" },
    deleted: { tone: "rust", sign: "−" },
    unchanged: { tone: "moss", sign: "=" },
  };
  const info = map[kind] ?? map.unchanged;
  return <Badge tone={info.tone}>{info.sign}</Badge>;
}
