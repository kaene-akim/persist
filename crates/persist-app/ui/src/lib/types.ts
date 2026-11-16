// TypeScript mirrors of the Rust types returned by the Tauri commands.
// Field names match the serde output exactly (snake_case unless the struct
// opts into camelCase).

export interface AppInfo {
  version: string;
  backend: "local" | "google-drive";
  repoName: string;
  activeRepo: string | null;
  connected: boolean;
  initialised: boolean;
  localPath: string | null;
  configDir: string;
  dataDir: string;
  driveFolder: string;
}

export interface RootConfig {
  root_id: string;
  name: string;
  local_path: string;
  remote_path: string;
  exclusions: string[];
  enabled: boolean;
  created_at: string;
}

export interface SnapshotStats {
  file_count: number;
  dir_count: number;
  symlink_count: number;
  logical_size: number;
  new_objects: number;
  new_bytes: number;
  added: number;
  modified: number;
  deleted: number;
}

export interface CachedSnapshot {
  snapshot_id: string;
  parent: string | null;
  created_at: string;
  stats: SnapshotStats;
}

export type EntryKind = "file" | "directory" | "symlink";

export interface FileEntry {
  path: string;
  kind: EntryKind;
  size: number;
  hash?: string | null;
  link_target?: string | null;
  mode: number;
  mtime: number;
}

export interface RootState {
  root_id: string;
  name: string;
  local_path: string;
  remote_path: string;
  entries: FileEntry[];
  exclusions: string[];
}

export interface Snapshot {
  format_version: number;
  snapshot_id: string;
  parent?: string | null;
  created_at: string;
  message?: string | null;
  roots: RootState[];
  deleted: string[];
  stats: SnapshotStats;
}

export type ChangeKind = "added" | "modified" | "deleted" | "unchanged";

export interface FileChange {
  root_id: string;
  root_name: string;
  path: string;
  kind: ChangeKind;
  old?: FileEntry | null;
  new?: FileEntry | null;
}

export interface ChangeSet {
  changes: FileChange[];
  added: number;
  modified: number;
  deleted: number;
  unchanged: number;
  changed_bytes: number;
}

export interface PlannedUpload {
  hash: string;
  size: number;
  local_path: string;
}

export interface BackupPlan {
  roots: unknown[];
  uploads: PlannedUpload[];
  change_set: ChangeSet;
  total_files: number;
  total_logical_size: number;
  upload_bytes: number;
  excluded_files: number;
  excluded_bytes: number;
  is_initial: boolean;
  head_snapshot: string | null;
}

export interface BackupResult {
  snapshot_id: string;
  parent: string | null;
  created_at: string;
  files_backed_up: number;
  logical_size: number;
  new_objects: number;
  new_bytes: number;
  added: number;
  modified: number;
  deleted: number;
  unchanged: number;
  duration_secs: number;
}

export interface RestoreItem {
  root_id: string;
  root_name: string;
  rel_path: string;
  kind: EntryKind;
  size: number;
  hash?: string | null;
  link_target?: string | null;
  dest_path: string;
  conflict: boolean;
  mode: number;
  mtime: number;
}

export interface RestorePlan {
  snapshot_id: string;
  items: RestoreItem[];
  files: number;
  directories: number;
  symlinks: number;
  total_bytes: number;
  conflicts: number;
}

export interface RestoreReport {
  snapshot_id: string;
  files_restored: number;
  directories_created: number;
  symlinks_restored: number;
  bytes_restored: number;
  skipped: number;
  overwritten: number;
  renamed: number;
  conflicts: number;
}

export interface CorruptRecord {
  hash: string;
  expected: string;
  actual: string;
}

export interface VerifyReport {
  snapshots_checked: number;
  objects_referenced: number;
  objects_checked: number;
  missing_objects: string[];
  corrupt_objects: CorruptRecord[];
  snapshot_issues: string[];
  orphan_objects: string[];
  missing_from_index: string[];
  ok: boolean;
}

export interface Quota {
  limit: number;
  usage: number;
  usage_in_drive: number;
  usage_in_drive_trash: number;
}

export interface RepoUsage {
  object_count: number;
  physical_bytes: number;
  snapshot_count: number;
  logical_bytes: number;
  total_logical_bytes: number;
}

export interface StorageInfo {
  backend: string;
  quota?: Quota | null;
  usage: RepoUsage;
}

export type ActivityKind =
  | "init"
  | "clone"
  | "add_root"
  | "remove_root"
  | "backup"
  | "restore"
  | "verify"
  | "config"
  | "auth"
  | "prune";

export type ActivityStatus = "running" | "success" | "failed" | "interrupted";

export interface ActivityEntry {
  id: number;
  timestamp: string;
  kind: ActivityKind;
  status: ActivityStatus;
  root_id?: string | null;
  message: string;
  detail: unknown;
}

export interface UserInfo {
  sub: string;
  email: string;
  name: string;
  picture: string | null;
}

export interface Settings {
  default_exclusions: string[];
  confirm_before_backup: boolean;
  verify_uploads: boolean;
  upload_chunk_size: number;
  follow_symlinks: boolean;
  store_symlinks: boolean;
  max_file_size: number;
  upload_concurrency: number;
}

export type OverwritePolicy = "refuse" | "overwrite" | "skip" | "rename";

export interface RestoreRequest {
  snapshotId: string | null;
  rootId: string | null;
  paths: string[];
  destination: string | null;
  overwrite: OverwritePolicy;
  dryRun: boolean;
}

export interface CommandError {
  code: string;
  message: string;
}

export type ProgressEvent =
  | { kind: "phase"; phase: string; total: number }
  | { kind: "progress"; done: number; total: number; bytes_done: number; bytes_total: number }
  | { kind: "message"; message: string };
