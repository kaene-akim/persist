# Repository format

A Persist **repository** is a self-describing tree of files. The same layout is
used whether the repository lives in a local directory or inside Google Drive.
The repository — not the local machine — is the source of truth: given only the
repository, Persist can rebuild its entire history and restore any snapshot.

This document is the specification. It corresponds to **repository format
version 1** and **snapshot manifest version 1**.

## Layout

```
repository.json                 repository metadata
refs/head.json                  pointer to the current head snapshot
snapshots/<snapshot-id>.json    one immutable manifest per snapshot
objects/<aa>/<sha256>           content objects, sharded by first two hex digits
index/objects/<aa>.json         sharded object index (hash -> size)
```

The `objects/` and `index/objects/` trees are sharded on the first two
hexadecimal characters of the SHA-256. This keeps any single directory small —
a real constraint of the Google Drive API — and spreads lookups evenly.

## `repository.json`

```jsonc
{
  "format_version": 1,
  "repo_id": "9f2c…",            // opaque, unique
  "name": "My backups",
  "backend": "google-drive",     // or "local"
  "created_at": "2026-11-16T09:00:00Z",
  "extensions": {}               // reserved for forward-compatible additions
}
```

`format_version` gates compatibility. A repository whose version is **newer**
than the running build is refused rather than misinterpreted.

## `refs/head.json`

```jsonc
{
  "snapshot": "2026-11-16T09-31-12Z-a1b2c3",  // or null when empty
  "updated_at": "2026-11-16T09:31:12Z"
}
```

The head is a movable pointer. Advancing it is the last step of a backup, which
is why an interrupted backup never changes what `head` refers to.

## Snapshots

A snapshot id is **sortable**: it begins with a UTC timestamp and ends with a
short random suffix.

```
2026-11-16T09-31-12Z-a1b2c3
└────── timestamp ─────┘ └ rand ┘
```

Manifests are immutable. Persist refuses to overwrite an existing snapshot file,
so the id doubles as a content address for the whole snapshot.

### Manifest schema

```jsonc
{
  "format_version": 1,
  "snapshot_id": "2026-11-16T09-31-12Z-a1b2c3",
  "parent": "2026-11-16T09-10-00Z-9f8e7d",   // absent for the first snapshot
  "created_at": "2026-11-16T09:31:12Z",
  "message": "before refactor",               // optional, from `push -m`
  "roots": [
    {
      "root_id": "b1c2…",
      "name": "projects",
      "local_path": "/home/you/projects",     // informative only
      "remote_path": "projects",              // logical path in the repo
      "exclusions": ["*.iso"],                // active at snapshot time
      "entries": [
        {
          "path": "src/main.rs",              // always /-separated, root-relative
          "kind": "file",                     // file | directory | symlink
          "size": 4096,
          "hash": "5f3a…",                    // SHA-256; files only
          "mode": 420,                        // unix permission bits
          "mtime": 1763281200
        }
      ]
    }
  ],
  "deleted": ["b1c2…:old/notes.txt"],         // root_id:path, removed since parent
  "stats": {
    "file_count": 128,
    "dir_count": 21,
    "symlink_count": 2,
    "logical_size": 918273,
    "new_objects": 12,
    "new_bytes": 40960,
    "added": 3,
    "modified": 9,
    "deleted": 1
  }
}
```

Notes:

- **`path` is validated on read.** Any entry whose path escapes its root
  (`../`, absolute paths, …) makes the whole manifest invalid. A hostile or
  corrupted manifest therefore cannot drive a restore outside its destination.
- **A `file` entry must carry a `hash`.** Directories and symlinks must not.
- **`parent` chains snapshots** into history. `deleted` records removals so the
  chain stays self-describing without re-listing the entire tree every time.
- **`local_path` is informative.** When restoring on another machine, Persist
  never writes to a recorded path unless you explicitly ask it to.

## Objects

Content is stored as `objects/<aa>/<sha256>`, where the file name is the full
lowercase hex SHA-256 of the bytes and `aa` is its first two characters.

- Objects are **immutable** and **content-addressed**: the same bytes are stored
  once no matter how many files, directories or snapshots reference them.
- Writing an object that already exists is a no-op.
- Reading an object re-hashes it and fails if the bytes do not match the name,
  so silent corruption is caught rather than propagated.

## Object index

The index is an **optimisation**, not a source of truth. It is a sharded map
from object hash to size:

```jsonc
// index/objects/5f.json
{ "objects": { "5f3a…": 4096, "5fb1…": 128 } }
```

It lets Persist answer "does this object already exist?" without listing the
entire object tree (which is expensive on Drive). If the index is missing or
stale it can always be rebuilt from the snapshots and objects — that is exactly
what `clone` does.

## What is authoritative

| Data | Authoritative? | If lost |
| --- | --- | --- |
| `repository.json`, snapshots, objects | **Yes** | Backup is gone |
| `refs/head.json` | Yes (but reconstructable) | Rebuilt as the newest snapshot |
| Object index | No | Rebuilt from snapshots + objects |
| Local SQLite cache | No | Rebuilt by `clone` |

## Format evolution

- `format_version` (repository) and `format_version` (snapshot) are independent.
- A reader accepts versions **≤** the one it knows.
- A reader refuses versions **>** the one it knows, with a clear message, instead
  of guessing.
- `extensions` on `repository.json` is a reserved map for additive fields.

## Deliberate deviation: one repository per folder

Some object stores (notably Google Drive) present a real folder tree. Persist
places **one repository directly inside the chosen folder** rather than nesting
many repositories under a `repositories/` subtree. This keeps Drive navigation
legible and avoids an extra layer of API calls. The trade-off is that one folder
holds one repository; use separate folders for separate repositories. The local
backend is unaffected by the constraint but follows the same layout for
consistency.
