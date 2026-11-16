import { invoke } from "@tauri-apps/api/core";
import type {
  ActivityEntry,
  AppInfo,
  BackupPlan,
  BackupResult,
  CachedSnapshot,
  ChangeSet,
  CommandError,
  RestorePlan,
  RestoreReport,
  RootConfig,
  Settings,
  Snapshot,
  StorageInfo,
  UserInfo,
  VerifyReport,
} from "./types";

/** Metadata describing a repository, returned by init/clone. */
export interface RepoMeta {
  repo_id: string;
  name: string;
  backend: string;
  format_version: number;
  created_at: string;
}

/** Normalise whatever the backend threw into a readable message. */
export function errorMessage(e: unknown): string {
  if (e && typeof e === "object") {
    const err = e as Partial<CommandError>;
    if (typeof err.message === "string") return err.message;
  }
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Unexpected error";
}

export function errorCode(e: unknown): string | null {
  if (e && typeof e === "object") {
    const err = e as Partial<CommandError>;
    if (typeof err.code === "string") return err.code;
  }
  return null;
}

export const api = {
  // application / configuration
  appInfo: () => invoke<AppInfo>("app_info"),
  getConfig: () => invoke<Record<string, unknown>>("get_config"),
  setBackend: (backend: string, localPath: string | null) =>
    invoke<void>("set_backend", { backend, localPath }),
  setDriveFolder: (folder: string) => invoke<void>("set_drive_folder", { folder }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),

  // authentication
  account: () => invoke<UserInfo | null>("account"),
  connectGoogle: () => invoke<UserInfo>("connect_google"),
  disconnectGoogle: () => invoke<void>("disconnect_google"),

  // repository lifecycle
  initRepository: (name: string, localPath: string | null) =>
    invoke<RepoMeta>("init_repository", { name, localPath }),
  cloneRepository: (repositoryId: string | null, destination: string | null) =>
    invoke<RepoMeta>("clone_repository", { repositoryId, destination }),

  // managed directories
  listRoots: () => invoke<RootConfig[]>("list_roots"),
  addRoot: (path: string, remote: string | null, name: string | null, exclude: string[]) =>
    invoke<RootConfig>("add_root", { path, remote, name, exclude }),
  removeRoot: (spec: string) => invoke<string>("remove_root", { spec }),
  updateRoot: (root: RootConfig) => invoke<void>("update_root", { root }),

  // backup
  planBackup: (onlyRoot: string | null) => invoke<BackupPlan>("plan_backup", { onlyRoot }),
  runBackup: (onlyRoot: string | null, dryRun: boolean, message: string | null) =>
    invoke<BackupResult>("run_backup", { onlyRoot, dryRun, message }),

  // history
  listSnapshots: (limit: number) => invoke<CachedSnapshot[]>("list_snapshots", { limit }),
  getSnapshot: (snapshotId: string | null) =>
    invoke<Snapshot | null>("get_snapshot", { snapshotId }),
  diffLocal: (onlyRoot: string | null) => invoke<ChangeSet>("diff_local", { onlyRoot }),
  diffSnapshots: (snapshotA: string, snapshotB: string) =>
    invoke<ChangeSet>("diff_snapshots", { snapshotA, snapshotB }),

  // restore
  planRestore: (request: unknown) => invoke<RestorePlan>("plan_restore", { request }),
  runRestore: (request: unknown) => invoke<RestoreReport>("run_restore", { request }),

  // verification, storage, activity
  verify: (deep: boolean) => invoke<VerifyReport>("verify", { deep }),
  storageInfo: () => invoke<StorageInfo>("storage_info"),
  listActivity: (limit: number) => invoke<ActivityEntry[]>("list_activity", { limit }),
  cancelOperation: () => invoke<void>("cancel_operation"),
};
