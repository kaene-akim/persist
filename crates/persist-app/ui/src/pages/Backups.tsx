import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../store";
import {
  Badge,
  Button,
  Card,
  CardHeader,
  EmptyState,
  Field,
  Modal,
  StatusDot,
  Toggle,
} from "../components/ui";
import { IconPlus } from "../components/icons";

export function Backups() {
  const { roots, run, toast, refreshAll, refreshStorage, snapshots } = useStore();
  const [adding, setAdding] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [remote, setRemote] = useState("");
  const [exclusions, setExclusions] = useState("");

  const reset = () => {
    setPath("");
    setName("");
    setRemote("");
    setExclusions("");
  };

  const addRoot = () =>
    run("Adding directory", async () => {
      if (!path.trim()) throw new Error("Choose a directory to protect.");
      const ex = exclusions
        .split(/[\n,]/)
        .map((s) => s.trim())
        .filter(Boolean);
      await api.addRoot(path.trim(), remote.trim() || null, name.trim() || null, ex);
      toast("success", "Directory added. Run a backup to create the first snapshot.");
      setAdding(false);
      reset();
      await refreshAll();
    });

  const removeRoot = (spec: string, label: string) =>
    run("Removing directory", async () => {
      await api.removeRoot(spec);
      toast("success", `Removed ${label}. Its cloud history is untouched.`);
      setConfirmRemove(null);
      await refreshAll();
    });

  const backupRoot = (rootId: string, label: string) =>
    run(`Backing up ${label}`, async () => {
      const result = await api.runBackup(rootId, false, null);
      toast("success", `Snapshot created for ${label} (${result.new_objects} new objects).`);
      await refreshAll();
      await refreshStorage();
    });

  const setEnabled = (rootId: string, enabled: boolean) =>
    run("Updating directory", async () => {
      const root = roots.find((r) => r.root_id === rootId);
      if (!root) return;
      await api.updateRoot({ ...root, enabled });
      await refreshAll();
    });

  return (
    <div className="space-y-6 px-6 py-6">
      <Card>
        <CardHeader
          title="Protected directories"
          hint="Each directory is tracked independently and can be backed up on its own."
          action={
            <Button variant="primary" size="sm" onClick={() => setAdding(true)}>
              <IconPlus className="h-3.5 w-3.5" />
              Add directory
            </Button>
          }
        />
        {roots.length === 0 ? (
          <div className="p-4">
            <EmptyState
              title="Nothing is protected yet"
              action={
                <Button variant="secondary" size="sm" onClick={() => setAdding(true)}>
                  Add your first directory
                </Button>
              }
            >
              Persist will snapshot the folder you choose, upload only what changed, and keep
              every previous version.
            </EmptyState>
          </div>
        ) : (
          <ul className="divide-y divide-line">
            {roots.map((root) => (
              <li key={root.root_id} className="px-4 py-4">
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <StatusDot tone={root.enabled ? "moss" : "neutral"} />
                      <h3 className="text-sm font-semibold text-ink">{root.name}</h3>
                      <Badge tone="neutral">remote: {root.remote_path}</Badge>
                      {!root.enabled && <Badge tone="clay">paused</Badge>}
                    </div>
                    <p className="mt-1.5 truncate font-mono text-xs text-muted" title={root.local_path}>
                      {root.local_path}
                    </p>
                    {root.exclusions.length > 0 && (
                      <p className="mt-1 text-xs text-faint">
                        {root.exclusions.length} extra exclusion
                        {root.exclusions.length === 1 ? "" : "s"}
                      </p>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => backupRoot(root.root_id, root.name)}
                      disabled={!root.enabled}
                    >
                      Back up
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setConfirmRemove(root.root_id)}
                    >
                      Remove
                    </Button>
                  </div>
                </div>
                <div className="mt-3 border-t border-line pt-2">
                  <Toggle
                    checked={root.enabled}
                    onChange={(v) => setEnabled(root.root_id, v)}
                    label="Include in backups"
                    hint="Paused directories keep their history but are skipped."
                  />
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      {snapshots.length === 0 && roots.length > 0 && (
        <p className="text-xs text-faint">
          No snapshots yet. Use <span className="text-muted">Back up</span> on a directory, or{" "}
          <span className="text-muted">Back up now</span> from the dashboard.
        </p>
      )}

      {adding && (
        <Modal
          title="Add a directory"
          onClose={() => setAdding(false)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setAdding(false)}>
                Cancel
              </Button>
              <Button variant="primary" onClick={addRoot}>
                Add directory
              </Button>
            </>
          }
        >
          <div className="space-y-4">
            <Field label="Directory" hint="The folder on this machine to protect.">
              <div className="flex gap-2">
                <input
                  className="field font-mono text-xs"
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  placeholder="/home/you/projects"
                />
                <Button
                  variant="secondary"
                  onClick={async () => {
                    const chosen = await open({ directory: true, multiple: false });
                    if (typeof chosen === "string") setPath(chosen);
                  }}
                >
                  Browse
                </Button>
              </div>
            </Field>
            <div className="grid grid-cols-2 gap-4">
              <Field label="Name" hint="Defaults to the folder name.">
                <input
                  className="field"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="projects"
                />
              </Field>
              <Field label="Remote path" hint="Its name inside the repository.">
                <input
                  className="field"
                  value={remote}
                  onChange={(e) => setRemote(e.target.value)}
                  placeholder="projects"
                />
              </Field>
            </div>
            <Field
              label="Extra exclusions"
              hint="One glob per line, added to the global defaults."
            >
              <textarea
                className="field h-24 resize-none font-mono text-xs"
                value={exclusions}
                onChange={(e) => setExclusions(e.target.value)}
                placeholder={"*.iso\nsecrets/"}
              />
            </Field>
          </div>
        </Modal>
      )}

      {confirmRemove && (
        <Modal
          title="Remove this directory?"
          onClose={() => setConfirmRemove(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirmRemove(null)}>
                Keep it
              </Button>
              <Button
                variant="danger"
                onClick={() => {
                  const root = roots.find((r) => r.root_id === confirmRemove);
                  if (root) removeRoot(root.root_id, root.name);
                }}
              >
                Remove
              </Button>
            </>
          }
        >
          <p className="text-sm leading-relaxed text-muted">
            Persist will stop tracking this directory on this machine. Snapshots that already
            exist are <span className="text-ink">never deleted</span> — you can still restore
            from any of them, and re-adding the directory picks up its history.
          </p>
        </Modal>
      )}
    </div>
  );
}
