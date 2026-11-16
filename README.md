# Persist

Persist protects the directories you care about by storing **incremental,
deduplicated, versioned snapshots** in Google Drive (or a plain local folder).
Every snapshot is immutable and independently recoverable, so a machine that has
lost everything only needs the repository — never a copy of its previous state.

Persist is one Rust core with two front ends:

- **`persist`** — a complete command-line interface that works on its own.
- **Persist Desktop** — a Tauri 2 + React application for the same engine.

Both talk to the same `persist-core` crate, so they behave identically.

```
┌────────────┐     ┌────────────┐
│  persist   │     │  Persist   │
│    CLI     │     │  Desktop   │
└─────┬──────┘     └─────┬──────┘
      │                  │
      └────────┬─────────┘
               ▼
        ┌─────────────┐
        │ persist-core│  engine · repository · storage · auth
        └──────┬──────┘
               ▼
   ┌───────────┴───────────┐
   ▼                       ▼
Local folder          Google Drive
```

## What it does

- **Incremental** — only content that changed since the last snapshot is
  uploaded. Everything else is referenced by hash.
- **Content-addressable** — every file is stored once, keyed by its SHA-256.
  Identical files in different directories (or different snapshots) share one
  object.
- **Versioned and immutable** — snapshots are never rewritten. Deleting a file
  locally records the deletion; it never silently deletes data in the cloud.
- **Honest about storage** — *logical* size (the sum of your files) and
  *physical* size (what is actually stored after deduplication) are reported
  separately.
- **Safe by default** — restores refuse to overwrite anything unless you
  explicitly ask, paths are sanitised against traversal, symlinks are handled
  deliberately, and an interrupted run never leaves a snapshot that looks
  successful.

## Requirements

- Linux (developed and tested on Arch Linux), macOS, or Windows.
- For the CLI: a Rust toolchain to build from source.
- For Google Drive: your own OAuth client (see
  [docs/google-oauth.md](docs/google-oauth.md)). The local backend needs no
  account at all.

## Quick start (local repository — no account needed)

```bash
# Build the CLI
cargo build --release -p persist-cli

# Point Persist at a local repository folder
persist init "My backups" --local /mnt/backup/persist

# Protect a directory
persist add ~/projects

# See what would be uploaded, then upload
persist push --dry-run
persist push --yes

# Inspect history
persist log
persist status
```

## Quick start (Google Drive)

```bash
# 1. Provide an OAuth client (once)
export PERSIST_CLIENT_ID="…apps.googleusercontent.com"
export PERSIST_CLIENT_SECRET="…"      # if your client type requires it

# 2. Authorise
persist login

# 3. Create a repository in Drive and start backing up
persist init --drive-folder "Persist"
persist add ~/Documents
persist push --yes
```

On a fresh machine, recover with:

```bash
persist login
persist clone            # discover the repository in Drive and rebuild history
persist list
persist pull --dry-run   # preview
persist pull --yes       # restore the latest snapshot
```

## Documentation

| Document | Contents |
| --- | --- |
| [docs/install-arch.md](docs/install-arch.md) | Building and installing on Arch Linux |
| [docs/cli.md](docs/cli.md) | Complete command reference |
| [docs/backup-restore.md](docs/backup-restore.md) | Day-to-day backup and recovery guide |
| [docs/google-oauth.md](docs/google-oauth.md) | Creating an OAuth client and configuring Persist |
| [docs/architecture.md](docs/architecture.md) | How the engine, repository and storage fit together |
| [docs/repository-format.md](docs/repository-format.md) | On-disk/cloud format specification |
| [docs/security.md](docs/security.md) | Threat model, credentials, path safety |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Common problems and their fixes |

## Project layout

```
crates/
  persist-core/   engine, repository format, storage backends, auth, SQLite cache
  persist-cli/    the `persist` binary
  persist-app/    Tauri 2 desktop backend
    ui/           React + TypeScript + Tailwind frontend
docs/             documentation
packaging/        distribution packaging (Arch Linux)
```

## Testing

```bash
cargo test --workspace          # 84 tests: unit, engine, recovery, CLI e2e
cargo test -p persist-cli --test cli
```

The CLI end-to-end suite drives the real binary against temporary local
repositories, so it exercises the exact code paths a user would.

## Project status and known limitations

Persist is complete and usable, with the following honest caveats:

- **Google Drive is implemented but has not been exercised against a live
  account in this build** (no test credentials were available). The local
  backend is fully covered by the automated test suite. Treat the first Drive
  run as a supervised one.
- **Encryption is not implemented.** The repository format was designed to allow
  a future encrypted-object mode, but data is currently stored as-is. Do not
  treat a repository as confidential beyond the protections of the underlying
  provider. See [docs/security.md](docs/security.md).
- **One repository per Persist folder.** A Drive folder (or local directory)
  holds exactly one repository. See
  [docs/repository-format.md](docs/repository-format.md) for the rationale.
- **Excluded directories are walked, not pruned** — exclusion patterns skip
  files, but Persist still descends into excluded directories to look for
  matches. This is correct but can be slower on very large trees.

## License

MIT.
