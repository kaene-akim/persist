# Backup and restore guide

This is the practical, day-to-day guide. For every flag, see
[cli.md](cli.md).

## The model in one minute

- You add one or more **directories** (roots) to Persist.
- Each **backup** (`push`) records a **snapshot**: an immutable list of every
  file, its size, its permissions and a SHA-256 hash of its contents.
- File contents are stored once as **objects**, keyed by hash. A file that did
  not change since the last snapshot costs nothing to include again.
- Restoring reconstructs files from objects. It is always safe: Persist refuses
  to overwrite anything unless you say so.

Two sizes are reported throughout:

- **logical** — the sum of all your files across snapshots (what you think you
  have).
- **physical** — what is actually stored after deduplication (what it costs).

## First-time setup

```bash
# Local repository (no account required)
persist init "My backups" --local /mnt/backup/persist

# …or Google Drive (see docs/google-oauth.md first)
persist login
persist init "My backups" --drive-folder "Persist"
```

Add the directories you want to protect:

```bash
persist add ~/projects
persist add ~/Documents --exclude '*.iso' --exclude 'tmp/'
persist list
```

Exclusions combine the global defaults (`.git/`, `node_modules/`, `target/`,
`*.tmp`, …) with any per-directory patterns. See `persist config` and
`persist config set default_exclusions "…"`.

## Taking a backup

Always preview first — it costs nothing and tells you exactly what would change:

```bash
persist push --dry-run
```

```
Backup preview
  files                 1284
  logical size          412.7 MiB
  new / modified / deleted  3 / 9 / 1
  unchanged             1271
  objects to upload     12
  estimated upload      39.2 MiB
```

Then commit it:

```bash
persist push --yes
```

- Only changed content is uploaded.
- The snapshot is written **after** every object it references is safely stored,
  so an interruption leaves your history intact.
- The previous snapshot is never modified.

Useful variants:

```bash
persist push projects              # only one directory
persist push -m "before refactor"  # attach a note
persist push --yes --json | jq -r .snapshot_id   # scriptable
```

## Seeing what changed

```bash
persist diff            # working tree vs. the latest snapshot
persist log             # snapshot history
persist diff <old> <new> --text   # two snapshots, with text diffs
persist status          # repository + storage overview
```

## Restoring

### Preview a restore

`pull` and `restore` always analyse first:

```bash
persist pull --dry-run
persist restore <snapshot-id> --path src/main.rs --dry-run
```

The preview reports files, directories, symlinks, total size and **conflicts**
(files that already exist at the destination).

### Restore to a safe place first

The safest restore writes into a fresh directory and never touches the original:

```bash
persist pull --destination /tmp/recovered/ --yes
```

Because `--destination` ends with `/`, it is treated as a directory and files
keep their relative names inside it.

### Restore a single file

```bash
# To an exact path:
persist restore <id> --path src/main.rs --destination /tmp/main.rs --yes

# Or into a directory:
persist restore <id> --path src/main.rs --destination /tmp/out/ --yes
```

### Restore in place

Restoring over the original locations requires an explicit conflict policy:

```bash
persist pull --overwrite --yes   # replace what is there
persist pull --skip --yes        # keep existing files
persist pull --rename --yes      # write alongside as "(restored)"
```

Without one of these, Persist stops and reports the conflicts rather than
guessing. Nothing is ever overwritten silently.

### Restore one directory

```bash
persist pull --root projects --destination /tmp/out/ --yes
```

`--root` accepts a directory id, its name, or its local path.

## Recovering on a new machine

You need: the repository (the Drive folder, or the local directory), and — for
Drive — an OAuth client and an authorisation.

```bash
# 1. Configure and authorise (Drive only)
export PERSIST_CLIENT_ID="…apps.googleusercontent.com"
export PERSIST_CLIENT_SECRET="…"
persist login

# 2. Adopt the repository and rebuild history from it
persist clone

# 3. See what is there
persist list
persist log

# 4. Recover
persist pull --destination ~/recovered/ --yes
```

`clone` reads the repository and rebuilds the local cache. Nothing about the
original machine's state is required — only the repository matters.

## Verifying integrity

```bash
persist verify          # fast: every referenced object exists
persist verify --deep   # thorough: download and re-hash every object
```

Run a quick `verify` after a large backup, and a `--deep` verify occasionally
(for example, before trusting a repository you did not create). A non-zero exit
means something is missing or corrupt; the report names the objects.

## Removing a directory

```bash
persist remove projects
```

This stops managing the directory on this machine. **Snapshots are never
deleted** — you can still restore from any of them, and re-adding the directory
re-attaches its history.

## Housekeeping

- `persist status` and `persist storage` show logical vs physical usage and the
  provider quota.
- Snapshots accumulate by design. Persist does not prune history automatically,
  and it never deletes cloud data on its own.
