import { useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../store";
import { Button, Card, CardHeader, Field, cx } from "../components/ui";
import { IconGoogle } from "../components/icons";

type Mode = "create" | "recover";
type Backend = "local" | "google-drive";

export function Setup() {
  const { run, refreshAll, refreshStorage, toast } = useStore();
  const [mode, setMode] = useState<Mode>("create");
  const [backend, setBackend] = useState<Backend>("local");
  const [name, setName] = useState("Persist");
  const [localPath, setLocalPath] = useState("");
  const [destination, setDestination] = useState("");
  const [repoId, setRepoId] = useState("");

  const pickFolder = async (set: (v: string) => void) => {
    const chosen = await open({ directory: true, multiple: false });
    if (typeof chosen === "string") set(chosen);
  };

  const create = () =>
    run(mode === "create" ? "Initialising repository" : "Adopting repository", async () => {
      if (backend === "local" && !localPath.trim()) {
        throw new Error("Choose a folder for the local repository.");
      }
      if (mode === "create") {
        await api.setBackend(backend, backend === "local" ? localPath : null);
        await api.initRepository(name.trim() || "Persist", backend === "local" ? localPath : null);
        toast("success", "Repository created. Add a directory to protect next.");
      } else {
        await api.setBackend(backend, backend === "local" ? localPath : null);
        await api.cloneRepository(repoId.trim() || null, destination.trim() || null);
        toast("success", "Repository adopted. Your history is ready to restore.");
      }
      await refreshAll();
      await refreshStorage();
    });

  return (
    <div className="mx-auto max-w-2xl px-6 py-10 animate-fade-in">
      <div className="mb-6">
        <h1 className="font-pixel text-xl text-ink">Set up Persist</h1>
        <p className="mt-2 max-w-prose text-sm leading-relaxed text-muted">
          Persist stores incremental, deduplicated snapshots of the directories you choose.
          Every snapshot is independently recoverable, so a new machine only needs the
          repository — never a copy of this computer's state.
        </p>
      </div>

      <div className="mb-4 flex border border-line">
        {(["create", "recover"] as Mode[]).map((m) => (
          <button
            key={m}
            onClick={() => setMode(m)}
            className={cx(
              "flex-1 px-4 py-2 text-sm font-medium transition-colors",
              mode === m ? "bg-panel2 text-ink" : "text-muted hover:text-ink",
            )}
          >
            {m === "create" ? "Create a new repository" : "Recover an existing one"}
          </button>
        ))}
      </div>

      <Card>
        <CardHeader
          title={mode === "create" ? "New repository" : "Adopt a repository"}
          hint={
            mode === "create"
              ? "Choose where snapshots will be stored."
              : "Point Persist at an existing repository to rebuild its history here."
          }
        />
        <div className="space-y-5 px-4 py-4">
          <div>
            <span className="label">Where should snapshots live?</span>
            <div className="mt-2 grid grid-cols-2 gap-2">
              <BackendOption
                active={backend === "local"}
                onClick={() => setBackend("local")}
                title="A local folder"
                hint="A directory or mounted drive"
              />
              <BackendOption
                active={backend === "google-drive"}
                onClick={() => setBackend("google-drive")}
                title="Google Drive"
                hint="Back up to the cloud"
                icon={<IconGoogle className="h-4 w-4" />}
              />
            </div>
          </div>

          {backend === "local" ? (
            <Field
              label="Repository folder"
              hint="Persist keeps its objects, snapshots and index here."
            >
              <div className="flex gap-2">
                <input
                  className="field font-mono text-xs"
                  value={localPath}
                  onChange={(e) => setLocalPath(e.target.value)}
                  placeholder="/mnt/backup/persist"
                />
                <Button variant="secondary" onClick={() => pickFolder(setLocalPath)}>
                  Browse
                </Button>
              </div>
            </Field>
          ) : (
            <p className="border border-line bg-panel2 px-3 py-2.5 text-xs leading-relaxed text-muted">
              Backing up to Google Drive needs a connected account. You can connect one now
              from Settings, then return here.
            </p>
          )}

          {mode === "create" ? (
            <Field label="Repository name">
              <input
                className="field"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Persist"
              />
            </Field>
          ) : (
            <>
              <Field
                label="Repository id"
                hint="Optional. Verified against the repository found at the location."
              >
                <input
                  className="field font-mono text-xs"
                  value={repoId}
                  onChange={(e) => setRepoId(e.target.value)}
                  placeholder="leave blank to accept whatever is found"
                />
              </Field>
              <Field
                label="Restore directories to"
                hint="Optional. Where recovered folders should be written on this machine."
              >
                <div className="flex gap-2">
                  <input
                    className="field font-mono text-xs"
                    value={destination}
                    onChange={(e) => setDestination(e.target.value)}
                    placeholder="leave blank to use the recorded paths"
                  />
                  <Button variant="secondary" onClick={() => pickFolder(setDestination)}>
                    Browse
                  </Button>
                </div>
              </Field>
            </>
          )}
        </div>
        <div className="flex justify-end border-t border-line px-4 py-3">
          <Button variant="primary" onClick={create}>
            {mode === "create" ? "Create repository" : "Adopt repository"}
          </Button>
        </div>
      </Card>
    </div>
  );
}

function BackendOption({
  active,
  onClick,
  title,
  hint,
  icon,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  hint: string;
  icon?: ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "flex flex-col gap-1 border px-3 py-3 text-left transition-colors",
        active ? "border-moss bg-moss/10" : "border-line hover:border-line2",
      )}
    >
      <span className="flex items-center gap-2 text-sm font-medium text-ink">
        {icon}
        {title}
      </span>
      <span className="text-xs text-muted">{hint}</span>
    </button>
  );
}
