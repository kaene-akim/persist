import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../store";
import { Badge, Button, Card, CardHeader, EmptyState, Field, cx } from "../components/ui";
import { humanBytes, humanCount, snapshotLabel } from "../lib/format";
import type { OverwritePolicy, RestorePlan, Snapshot } from "../lib/types";

type DestinationMode = "original" | "folder";

export function Restore() {
  const { snapshots, restoreTarget, run, toast, refreshAll } = useStore();

  const [snapshotId, setSnapshotId] = useState<string | null>(restoreTarget);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [rootId, setRootId] = useState<string>("");
  const [subPath, setSubPath] = useState("");
  const [destMode, setDestMode] = useState<DestinationMode>("original");
  const [destFolder, setDestFolder] = useState("");
  const [overwrite, setOverwrite] = useState<OverwritePolicy>("refuse");
  const [plan, setPlan] = useState<RestorePlan | null>(null);

  useEffect(() => {
    if (restoreTarget && restoreTarget !== snapshotId) setSnapshotId(restoreTarget);
  }, [restoreTarget, snapshotId]);

  useEffect(() => {
    if (!snapshotId && snapshots.length > 0) setSnapshotId(snapshots[0].snapshot_id);
  }, [snapshots, snapshotId]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!snapshotId) {
        setSnapshot(null);
        return;
      }
      try {
        const snap = await api.getSnapshot(snapshotId);
        if (!cancelled) setSnapshot(snap);
      } catch {
        if (!cancelled) setSnapshot(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [snapshotId]);

  const request = useMemo(
    () => ({
      snapshotId,
      rootId: rootId || null,
      paths: subPath.trim() ? [subPath.trim()] : [],
      destination: destMode === "folder" && destFolder.trim() ? destFolder.trim() : null,
      overwrite,
      dryRun: true,
    }),
    [snapshotId, rootId, subPath, destMode, destFolder, overwrite],
  );

  // Reset the preview whenever the request changes.
  useEffect(() => {
    setPlan(null);
  }, [request]);

  const preview = () =>
    run("Analysing restore", async () => {
      const p = await api.planRestore(request);
      setPlan(p);
      return p;
    });

  const doRestore = () =>
    run("Restoring", async () => {
      const p = plan ?? (await api.planRestore(request));
      setPlan(p);
      if (p.conflicts > 0 && overwrite === "refuse") {
        throw new Error(
          `${p.conflicts} file(s) already exist at the destination. Choose overwrite, skip or rename.`,
        );
      }
      const report = await api.runRestore({ ...request, dryRun: false });
      toast(
        "success",
        `Restored ${humanCount(report.files_restored)} files (${humanBytes(report.bytes_restored)}).`,
      );
      await refreshAll();
      setPlan(null);
    });

  if (snapshots.length === 0) {
    return (
      <div className="px-6 py-6">
        <Card>
          <div className="p-4">
            <EmptyState title="Nothing to restore yet">
              Once you have taken a snapshot, you can bring any part of it back here — into the
              original location or a folder you choose.
            </EmptyState>
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-1 gap-6 px-6 py-6 lg:grid-cols-2">
      <div className="space-y-6">
        <Card>
          <CardHeader title="What to restore" />
          <div className="space-y-4 px-4 py-4">
            <Field label="Snapshot">
              <select
                className="field"
                value={snapshotId ?? ""}
                onChange={(e) => {
                  setSnapshotId(e.target.value);
                  setRootId("");
                  setSubPath("");
                }}
              >
                {snapshots.map((s) => (
                  <option key={s.snapshot_id} value={s.snapshot_id}>
                    {snapshotLabel(s.snapshot_id)} — {new Date(s.created_at).toLocaleString()}
                  </option>
                ))}
              </select>
            </Field>

            <Field
              label="Directory"
              hint="Restrict the restore to one protected directory, or leave as everything."
            >
              <select
                className="field"
                value={rootId}
                onChange={(e) => setRootId(e.target.value)}
              >
                <option value="">Everything in the snapshot</option>
                {snapshot?.roots.map((r) => (
                  <option key={r.root_id} value={r.root_id}>
                    {r.name}
                  </option>
                ))}
              </select>
            </Field>

            <Field
              label="Specific path"
              hint="Optional. A single file or subfolder, relative to the directory above."
            >
              <input
                className="field font-mono text-xs"
                value={subPath}
                onChange={(e) => setSubPath(e.target.value)}
                placeholder="src/main.rs"
              />
            </Field>
          </div>
        </Card>

        <Card>
          <CardHeader title="Where to write it" />
          <div className="space-y-4 px-4 py-4">
            <div className="grid grid-cols-2 gap-2">
              <Choice
                active={destMode === "original"}
                onClick={() => setDestMode("original")}
                title="Original locations"
                hint="Paths recorded in the snapshot"
              />
              <Choice
                active={destMode === "folder"}
                onClick={() => setDestMode("folder")}
                title="A folder"
                hint="Nothing is overwritten outside it"
              />
            </div>

            {destMode === "folder" && (
              <Field label="Destination folder">
                <div className="flex gap-2">
                  <input
                    className="field font-mono text-xs"
                    value={destFolder}
                    onChange={(e) => setDestFolder(e.target.value)}
                    placeholder="/home/you/recovered"
                  />
                  <Button
                    variant="secondary"
                    onClick={async () => {
                      const chosen = await open({ directory: true, multiple: false });
                      if (typeof chosen === "string") setDestFolder(chosen);
                    }}
                  >
                    Browse
                  </Button>
                </div>
              </Field>
            )}

            <Field
              label="If a file already exists"
              hint="Persist never overwrites silently."
            >
              <select
                className="field"
                value={overwrite}
                onChange={(e) => setOverwrite(e.target.value as OverwritePolicy)}
              >
                <option value="refuse">Stop and report the conflict</option>
                <option value="overwrite">Replace it</option>
                <option value="skip">Keep the existing file</option>
                <option value="rename">Write alongside as "(restored)"</option>
              </select>
            </Field>
          </div>
        </Card>
      </div>

      <div className="space-y-6">
        <Card>
          <CardHeader
            title="Preview"
            hint="A restore is analysed before anything is written."
            action={
              <Button variant="secondary" size="sm" onClick={preview}>
                Analyse
              </Button>
            }
          />
          {plan ? (
            <div className="space-y-4 px-4 py-4">
              <div className="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3">
                <Metric label="Files" value={humanCount(plan.files)} />
                <Metric label="Directories" value={humanCount(plan.directories)} />
                <Metric label="Symlinks" value={humanCount(plan.symlinks)} />
                <Metric label="Total size" value={humanBytes(plan.total_bytes)} />
                <Metric label="Items" value={humanCount(plan.items.length)} />
                <Metric
                  label="Conflicts"
                  value={humanCount(plan.conflicts)}
                  tone={plan.conflicts > 0 ? "rust" : "moss"}
                />
              </div>
              {plan.items.length > 0 && (
                <ul className="max-h-72 divide-y divide-line overflow-y-auto border border-line">
                  {plan.items.slice(0, 300).map((item, i) => (
                    <li
                      key={`${item.dest_path}:${i}`}
                      className="flex items-center justify-between gap-3 px-3 py-1.5 text-xs"
                    >
                      <span className="truncate font-mono text-muted" title={item.dest_path}>
                        {item.root_name}/{item.rel_path}
                      </span>
                      {item.conflict && <Badge tone="rust">exists</Badge>}
                    </li>
                  ))}
                </ul>
              )}
              <Button variant="primary" className="w-full" onClick={doRestore}>
                Restore {humanCount(plan.files)} file{plan.files === 1 ? "" : "s"}
              </Button>
            </div>
          ) : (
            <div className="px-4 py-6 text-center text-xs text-faint">
              Analyse the restore to see exactly what will be written and where.
            </div>
          )}
        </Card>

        <p className="px-1 text-xs leading-relaxed text-faint">
          Files are written to a temporary name and moved into place, so an interrupted restore
          never leaves a half-written file. Every restored object is verified against its stored
          hash.
        </p>
      </div>
    </div>
  );
}

function Choice({
  active,
  onClick,
  title,
  hint,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  hint: string;
}) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "flex flex-col gap-1 border px-3 py-2.5 text-left transition-colors",
        active ? "border-moss bg-moss/10" : "border-line hover:border-line2",
      )}
    >
      <span className="text-sm font-medium text-ink">{title}</span>
      <span className="text-[11px] text-muted">{hint}</span>
    </button>
  );
}

function Metric({
  label,
  value,
  tone = "neutral",
}: {
  label: string;
  value: string;
  tone?: "neutral" | "moss" | "rust";
}) {
  const color =
    tone === "moss" ? "text-moss" : tone === "rust" ? "text-rust" : "text-ink";
  return (
    <div>
      <div className="text-xs text-faint">{label}</div>
      <div className={cx("mt-0.5 text-sm", color)}>{value}</div>
    </div>
  );
}
