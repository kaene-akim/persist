import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../store";
import { Badge, Button, Card, CardHeader, Field, Toggle } from "../components/ui";
import { IconGoogle } from "../components/icons";
import { humanBytes } from "../lib/format";
import type { Settings as SettingsType } from "../lib/types";

export function Settings() {
  const {
    settings,
    account,
    info,
    refreshSettings,
    refreshAccount,
    refreshInfo,
    refreshAll,
    refreshStorage,
    run,
    toast,
  } = useStore();

  const [draft, setDraft] = useState<SettingsType | null>(settings);
  const [driveFolder, setDriveFolder] = useState(info?.driveFolder ?? "");
  const [backend, setBackend] = useState<"local" | "google-drive">(
    info?.backend === "google-drive" ? "google-drive" : "local",
  );
  const [localPath, setLocalPath] = useState(info?.localPath ?? "");

  useEffect(() => setDraft(settings), [settings]);
  useEffect(() => setDriveFolder(info?.driveFolder ?? ""), [info?.driveFolder]);
  useEffect(
    () => setBackend(info?.backend === "google-drive" ? "google-drive" : "local"),
    [info?.backend],
  );
  useEffect(() => setLocalPath(info?.localPath ?? ""), [info?.localPath]);

  const connect = () =>
    run("Waiting for Google authorisation", async () => {
      const user = await api.connectGoogle();
      toast("success", `Connected as ${user.email}.`);
      await refreshAccount();
      await refreshInfo();
    });

  const disconnect = () =>
    run("Disconnecting", async () => {
      await api.disconnectGoogle();
      toast("success", "Disconnected. Stored tokens were removed from this machine.");
      await refreshAccount();
      await refreshInfo();
    });

  const saveBackend = () =>
    run("Applying storage settings", async () => {
      await api.setBackend(backend, backend === "local" ? localPath.trim() || null : null);
      if (backend === "google-drive") {
        await api.setDriveFolder(driveFolder.trim() || "Persist");
      }
      toast("success", "Storage settings saved.");
      await refreshInfo();
      await refreshStorage();
    });

  const saveSettings = () =>
    run("Saving settings", async () => {
      if (!draft) return;
      await api.setSettings(draft);
      toast("success", "Settings saved.");
      await refreshSettings();
      await refreshAll();
    });

  const patch = (p: Partial<SettingsType>) => setDraft((d) => (d ? { ...d, ...p } : d));

  const dirty =
    draft != null &&
    settings != null &&
    JSON.stringify(draft) !== JSON.stringify(settings);

  return (
    <div className="mx-auto max-w-3xl space-y-6 px-6 py-6">
      <Card>
        <CardHeader
          title="Account"
          hint="Persist talks to Google Drive with an app-scoped permission — it can only see files it created."
        />
        <div className="px-4 py-4">
          {account ? (
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm text-ink">{account.name || account.email}</span>
                  <Badge tone="moss">connected</Badge>
                </div>
                <p className="mt-0.5 truncate font-mono text-xs text-muted">{account.email}</p>
              </div>
              <Button variant="secondary" size="sm" onClick={disconnect}>
                Disconnect
              </Button>
            </div>
          ) : (
            <div className="flex items-center justify-between gap-4">
              <p className="max-w-md text-xs leading-relaxed text-muted">
                Not connected. Authorising opens your browser once; the refresh token is stored in
                your system keyring (or a 0600 file when none is available).
              </p>
              <Button variant="primary" size="sm" onClick={connect}>
                <IconGoogle className="h-3.5 w-3.5" />
                Connect Google Drive
              </Button>
            </div>
          )}
        </div>
      </Card>

      <Card>
        <CardHeader
          title="Where snapshots are stored"
          hint="Changing this points Persist at a different repository. Existing history is never deleted."
        />
        <div className="space-y-4 px-4 py-4">
          <div className="grid grid-cols-2 gap-2">
            <Choice
              active={backend === "local"}
              onClick={() => setBackend("local")}
              title="A local folder"
              hint="A directory or mounted drive"
            />
            <Choice
              active={backend === "google-drive"}
              onClick={() => setBackend("google-drive")}
              title="Google Drive"
              hint="Back up to the cloud"
            />
          </div>

          {backend === "local" ? (
            <Field label="Repository folder">
              <input
                className="field font-mono text-xs"
                value={localPath}
                onChange={(e) => setLocalPath(e.target.value)}
                placeholder="/mnt/backup/persist"
              />
            </Field>
          ) : (
            <Field
              label="Google Drive folder name"
              hint="Persist keeps one repository inside this folder in your Drive."
            >
              <input
                className="field"
                value={driveFolder}
                onChange={(e) => setDriveFolder(e.target.value)}
                placeholder="Persist"
              />
            </Field>
          )}

          <div className="flex justify-end">
            <Button variant="secondary" size="sm" onClick={saveBackend}>
              Apply storage settings
            </Button>
          </div>
        </div>
      </Card>

      <Card>
        <CardHeader
          title="Backup behaviour"
          hint="These defaults apply to every protected directory."
        />
        {draft ? (
          <div className="divide-y divide-line px-4">
            <div className="py-2">
              <Toggle
                checked={draft.confirm_before_backup}
                onChange={(v) => patch({ confirm_before_backup: v })}
                label="Confirm before uploading"
                hint="Show a preview and wait for approval before sending data."
              />
            </div>
            <div className="py-2">
              <Toggle
                checked={draft.verify_uploads}
                onChange={(v) => patch({ verify_uploads: v })}
                label="Verify uploads"
                hint="Re-download and hash each object after upload. Slower, but proves integrity."
              />
            </div>
            <div className="py-2">
              <Toggle
                checked={draft.store_symlinks}
                onChange={(v) => patch({ store_symlinks: v })}
                label="Record symbolic links"
                hint="Store links as links instead of skipping them."
              />
            </div>
            <div className="py-2">
              <Toggle
                checked={draft.follow_symlinks}
                onChange={(v) => patch({ follow_symlinks: v })}
                label="Follow symbolic links"
                hint="Back up the contents a link points to. Loops are detected and stopped."
              />
            </div>
            <div className="grid grid-cols-2 gap-4 py-4">
              <Field label="Concurrent uploads" hint="Higher is faster but uses more bandwidth.">
                <input
                  type="number"
                  min={1}
                  max={16}
                  className="field"
                  value={draft.upload_concurrency}
                  onChange={(e) =>
                    patch({ upload_concurrency: clampInt(e.target.value, 1, 16, 4) })
                  }
                />
              </Field>
              <Field
                label="Skip files larger than"
                hint={draft.max_file_size === 0 ? "No limit." : humanBytes(draft.max_file_size)}
              >
                <input
                  type="number"
                  min={0}
                  step={1024 * 1024}
                  className="field"
                  value={draft.max_file_size}
                  onChange={(e) => patch({ max_file_size: clampInt(e.target.value, 0, 1 << 50, 0) })}
                />
              </Field>
            </div>
            <div className="py-4">
              <Field
                label="Default exclusions"
                hint="One glob per line. Applied to every directory, in addition to its own."
              >
                <textarea
                  className="field h-32 resize-none font-mono text-xs"
                  value={draft.default_exclusions.join("\n")}
                  onChange={(e) =>
                    patch({
                      default_exclusions: e.target.value
                        .split("\n")
                        .map((s) => s.trim())
                        .filter(Boolean),
                    })
                  }
                />
              </Field>
            </div>
            <div className="flex items-center justify-between py-3">
              <span className="text-xs text-faint">
                {dirty ? "You have unsaved changes." : "All changes are saved."}
              </span>
              <Button variant="primary" size="sm" onClick={saveSettings} disabled={!dirty}>
                Save settings
              </Button>
            </div>
          </div>
        ) : (
          <div className="px-4 py-6 text-center text-xs text-faint">Loading settings…</div>
        )}
      </Card>

      <Card>
        <CardHeader title="This installation" />
        <dl className="divide-y divide-line">
          <InfoRow label="Version" value={info?.version ?? "—"} />
          <InfoRow label="Repository" value={info?.repoName ?? "—"} />
          <InfoRow label="Config directory" value={info?.configDir ?? "—"} mono />
          <InfoRow label="Data directory" value={info?.dataDir ?? "—"} mono />
        </dl>
      </Card>
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
      className={
        "flex flex-col gap-1 border px-3 py-2.5 text-left transition-colors " +
        (active ? "border-moss bg-moss/10" : "border-line hover:border-line2")
      }
    >
      <span className="text-sm font-medium text-ink">{title}</span>
      <span className="text-[11px] text-muted">{hint}</span>
    </button>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3">
      <dt className="text-sm text-muted">{label}</dt>
      <dd
        className={
          mono
            ? "truncate font-mono text-xs text-ink"
            : "truncate text-sm text-ink"
        }
        title={value}
      >
        {value}
      </dd>
    </div>
  );
}

function clampInt(raw: string, min: number, max: number, fallback: number): number {
  const n = Number.parseInt(raw, 10);
  if (Number.isNaN(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}
