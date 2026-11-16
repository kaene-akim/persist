# Architecture

Persist is deliberately built as **one engine with two thin interfaces**. All
behaviour lives in `persist-core`; the CLI and the desktop app only translate
arguments and render results. This is what guarantees that a backup taken in the
GUI can be restored from the CLI, and vice versa.

## Layers

```
                ┌──────────────────────────────┐
                │          interfaces          │
                │   persist-cli   persist-app  │
                └───────────────┬──────────────┘
                                │  Session facade
                ┌───────────────▼──────────────┐
                │            engine            │
                │  backup · restore · diff ·   │
                │  verify        (Progress)    │
                └───────────────┬──────────────┘
        ┌───────────────────────┼───────────────────────┐
        ▼                       ▼                       ▼
 ┌─────────────┐        ┌──────────────┐        ┌──────────────┐
 │  repository │        │   scanner    │        │   exclude    │
 │  (format)   │        │  (walk fs)   │        │  (patterns)  │
 └──────┬──────┘        └──────────────┘        └──────────────┘
        ▼
 ┌─────────────┐   ┌──────────────┐   ┌──────────────┐
 │   storage   │──▶│    local     │   │    memory    │  (tests)
 │   trait     │──▶│    drive     │   │              │
 └─────────────┘   └──────────────┘   └──────────────┘

        auth (OAuth + token store)      db (SQLite cache)      config
```

## Modules (`persist-core`)

| Module | Responsibility |
| --- | --- |
| `engine` | Backup planning/execution, restoration, diffs and verification. Provider-agnostic. |
| `repo` | The repository format: metadata, head ref, snapshots, objects and the sharded object index. The only component that knows the storage layout. |
| `storage` | The `Storage` trait and its backends (`local`, `memory`, and `drive`). |
| `scanner` | Walks a directory tree into a list of entries, applying exclusion rules and symlink policy. |
| `exclude` | Compiles glob patterns into a matcher. |
| `hashing` | SHA-256 helpers (streaming file hashing, hash validation). |
| `auth` | OAuth 2.0 loopback + PKCE flow, token refresh, and secure token storage. |
| `drive` | `DriveStorage`: the Google Drive API v3 implementation of `Storage`. |
| `db` | Local SQLite cache: managed directories, settings, snapshot metadata, activity log. |
| `config` | Filesystem locations (`Paths`) and the persisted `AppConfig`. |
| `model` | All serialisable data types and their invariants. |
| `session` | `Session`: wires everything together and exposes the operations the interfaces call. |
| `util` | Small shared helpers (safe path joins, snapshot ids, size formatting). |
| `error` | `Error`, stable machine-readable codes, and process exit codes. |

## The `Session` facade

`Session` is the single entry point used by both front ends. It owns:

- the resolved `Paths`,
- the `AppConfig`,
- the `Db` (SQLite cache),
- the `AuthManager`,
- the current `StorageRef`, and
- the `Repository` built on top of that storage.

It exposes high-level operations — `init_repository`, `clone_repository`,
`add_root`, `plan_backup`, `run_backup`, `plan_restore`, `restore`, `verify`,
`snapshots`, `diff_local`, `diff_snapshots`, `storage_info`, `activity` — each of
which is a thin orchestration over the engine. Because the CLI and GUI call the
same methods, their semantics cannot drift.

## The engine

The engine never touches a provider directly; it is driven by a `Repository` and
reports progress through the `Progress` trait.

### Backup

Backup is split into two phases so interfaces can preview before committing:

1. **Plan** (`plan_backup`) — scan the roots, diff against the head snapshot,
   decide which objects are new, and produce a `BackupPlan` (including a full
   `ChangeSet`). No data is written.
2. **Execute** (`execute_backup`) — upload every new object, then write the
   snapshot manifest, then advance the head pointer.

The ordering matters. A snapshot is written **only after** every object it
references is durable in storage. If the process dies mid-upload, no snapshot
exists, and re-running simply reuses the objects that did make it.

When `verify_uploads` is enabled, each object is re-downloaded and re-hashed
before the snapshot is committed.

### Restore

Restoration is the most safety-critical path, so it is built around three rules:

1. **Never silently overwrite.** The default policy is `Refuse`. Existing files
   abort the operation before anything is written. `--overwrite`, `--skip` and
   `--rename` are explicit opt-ins.
2. **Never escape the destination.** Every relative path is sanitised, and each
   ancestor directory is checked for symlinks so a crafted snapshot cannot write
   outside the target.
3. **Never leave a half-written file.** Objects are streamed to a temporary file
   in the destination directory and atomically renamed into place.

Objects are verified against their stored hash as they are written.

### Diff

`diff` compares either the working tree against the head snapshot, or two
snapshots. It produces a `ChangeSet` of per-path `FileChange`s and aggregate
counters. It never downloads file contents unless `--text` is requested.

### Verify

`verify` walks every snapshot, checks that every referenced object exists, and
optionally (`--deep`) downloads and re-hashes each one. It also reports objects
present in the index but unreferenced (orphans) and objects referenced but
missing from the index.

## Storage abstraction

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    fn backend_name(&self) -> &'static str;
    async fn put(&self, path: &str, data: &[u8]) -> Result<()>;
    async fn get(&self, path: &str) -> Result<Vec<u8>>;
    async fn get_opt(&self, path: &str) -> Result<Option<Vec<u8>>>;
    async fn get_to_file(&self, path: &str, dest: &Path) -> Result<()>;
    async fn put_from_file(&self, path: &str, src: &Path) -> Result<()>;
    async fn exists(&self, path: &str) -> Result<bool>;
    async fn delete(&self, path: &str) -> Result<()>;
    async fn list(&self, prefix: &str) -> Result<Vec<DirEntry>>;
    async fn quota(&self) -> Result<Option<Quota>>;
    // …
}
```

The trait exposes a flat, `/`-separated logical namespace. Backends:

- **`LocalStorage`** — a directory on disk. Used for the local backend and
  throughout the tests. Fully implemented and exercised.
- **`MemoryStorage`** — an in-memory map used by unit tests.
- **`DriveStorage`** — Google Drive API v3, mapping logical paths to a folder
  tree, with resumable uploads for large objects.

Because the engine only sees `Storage`, adding S3, Backblaze or WebDAV later
means implementing one trait, not touching the engine.

## The local cache (`db`) versus the cloud

The cloud repository is **authoritative**. The local SQLite database is a cache:
managed directories, user settings, a copy of snapshot metadata for fast
listing, and the activity log. Deleting it must never lose a backup — only local
convenience state. `clone` proves this by rebuilding everything from the
repository (`rebuild_index`).

## Progress and cancellation

Long operations report progress through the `Progress` trait:

- the CLI renders an `indicatif` progress bar (only on a TTY, never in `--json`
  mode), and
- the desktop app forwards events named `persist://progress` to the webview.

A shared cancellation flag lets `Ctrl-C` (CLI) or the Cancel button (GUI) stop an
operation at a safe point — between objects, never mid-write.

## Error handling

Every fallible operation returns `persist_core::Error`. The variants are
fine-grained enough for both interfaces to react meaningfully:

- the CLI maps them to stable **exit codes** (see [cli.md](cli.md)),
- `--json` mode emits `{ "ok": false, "error": { "code", "message" } }`, and
- the desktop app maps the same `code` into a structured command error.

No operation ever reports success for work that did not complete.
