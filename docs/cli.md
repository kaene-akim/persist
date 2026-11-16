# CLI reference

The `persist` binary is the complete interface. Every command works without the
desktop application.

```
persist [GLOBAL OPTIONS] <COMMAND> [OPTIONS]
```

## Global options

| Option | Meaning |
| --- | --- |
| `--json` | Emit machine-readable JSON instead of text. Errors also become JSON on stderr. |
| `-y`, `--yes` | Assume "yes" for every confirmation (non-interactive). |
| `-q`, `--quiet` | Reduce output to a minimum (suppresses `push`'s success summary). |
| `--home <PATH>` | Override the Persist home directory (config + state). Also read from `$PERSIST_HOME`. |
| `-h`, `--help` | Help for the command. |
| `-V`, `--version` | Version. |

`--json` changes the shape of both success and error output. In `--json` mode
no progress bars or colour are emitted, so the output is safe to pipe.

## Commands

### Account

| Command | Description |
| --- | --- |
| `persist login [--no-browser]` | Run the Google OAuth flow and store tokens. `--no-browser` prints the URL without trying to open a browser. |
| `persist logout` | Delete stored tokens for this machine. The repository is untouched. |
| `persist account` | Show the connected Google account (or report that none is connected). |

### Repository lifecycle

| Command | Description |
| --- | --- |
| `persist init [NAME] [--local <PATH>] [--drive-folder <NAME>]` | Create a repository. Defaults to Google Drive; `--local` creates it in a directory instead. |
| `persist clone [REPOSITORY_ID] [DESTINATION]` | Discover an existing repository on the configured backend, adopt it on this machine, and rebuild the local history cache. `REPOSITORY_ID` is optional and verified when given. `DESTINATION` sets where recovered directories should be written. |

### Managed directories

| Command | Description |
| --- | --- |
| `persist add <PATH> [--remote <DEST>] [--name <NAME>] [--exclude <GLOB>]…` | Add a local directory to backup management. `--remote` sets its logical path inside the repository; `--exclude` adds per-directory globs (repeatable). |
| `persist remove <PATH\|NAME>` | Stop managing a directory. **Its cloud history is preserved.** |
| `persist list` | List managed directories and repository information. |

### Backing up

| Command | Description |
| --- | --- |
| `persist push [PATH] [--dry-run] [-m <MSG>]` | Create a snapshot. Optionally limit to one managed directory (by path or name). `--dry-run` analyses and prints the plan without uploading anything. `-m/--message` attaches a note. |
| `persist status` | Show repository status: backend, head snapshot, counts, logical vs physical storage, and the managed directories. |
| `persist diff [A] [B] [--text]` | Compare the working tree with the head snapshot (no arguments), one snapshot against the head (one argument), or two snapshots (two arguments). `--text` prints text diffs for changed text files. |

### Restoring

`pull` restores the latest snapshot by default; `restore` restores a specific
one. They share the same options.

| Command | Description |
| --- | --- |
| `persist pull [--snapshot <ID>] [options]` | Restore the latest snapshot, or a specific one. |
| `persist restore <SNAPSHOT_ID> [options]` | Restore a specific snapshot. |

Shared options:

| Option | Meaning |
| --- | --- |
| `--destination <PATH>` | Write to this location instead of the recorded paths. |
| `--root <ROOT>` | Restrict the restore to one managed directory, by id, name, or local path. |
| `--path <REL>` | Restore only these root-relative paths (repeatable). |
| `--overwrite` | Replace files that already exist. |
| `--skip` | Leave existing files untouched. |
| `--rename` | Write alongside existing files with a `(restored)` suffix. |
| `--dry-run` | Analyse and print the plan; write nothing. |

With no conflict option, restoration **refuses to overwrite** and reports the
conflicts instead of writing anything.

**Single-file destination rule.** When exactly one `--path` selects a file and
`--destination` is neither an existing directory nor ends with `/`, the file is
written to that exact path. Otherwise `--destination` is treated as a directory
and the file keeps its relative name inside it.

### Inspection and maintenance

| Command | Description |
| --- | --- |
| `persist log [--limit <N>]` | Show snapshot history (default 20). |
| `persist storage` | Show provider quota (if any) and Persist's logical/physical usage. |
| `persist verify [--deep]` | Check repository integrity. `--deep` downloads and re-hashes every object. Exits non-zero when problems are found. |

### Configuration

| Command | Description |
| --- | --- |
| `persist config` | Print the full configuration and settings. |
| `persist config get <KEY>` | Print one value. |
| `persist config set <KEY> <VALUE>` | Set one value and save. |

Configurable keys:

| Key | Type | Notes |
| --- | --- | --- |
| `backend` | `local` \| `google-drive` | Also accepts `drive` / `gdrive`. |
| `repo_name` (or `repository_name`) | string | Human-readable repository name. |
| `drive_folder` | string | Folder name created in Google Drive. |
| `local_path` | path | Used when `backend = local`. |
| `verify_uploads` | bool | Re-download and hash each object after upload. |
| `upload_concurrency` | integer | Concurrent uploads. |
| `follow_symlinks` | bool | Follow links to their targets. |
| `store_symlinks` | bool | Record links as links. |
| `max_file_size` | bytes | Skip files larger than this (`0` = no limit). |
| `upload_chunk_size` | bytes | Resumable upload chunk size. |
| `default_exclusions` | comma-separated globs | Applied to every directory. |

Booleans accept `true/1/yes/on` and their negatives.

### Shell completions

```bash
persist completions bash > /usr/share/bash-completion/completions/persist
persist completions zsh  > ~/.zfunc/_persist
persist completions fish > ~/.config/fish/completions/persist.fish
```

Supported shells: `bash`, `zsh`, `fish`.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | Generic error (I/O, configuration, provider error, …). |
| `3` | Authentication problem (not connected, expired, failed). |
| `4` | Cloud storage is full. |
| `5` | Not found: repository, snapshot, managed directory, or source directory. |
| `6` | Repository is missing data, corrupt, or an unsupported format. |
| `7` | Refused for safety: path traversal, or refusing to overwrite. |
| `130` | Cancelled by the user (declined a confirmation). |
| `131` | Interrupted (e.g. `Ctrl-C`). |

## JSON output

Success (example, `push --json`):

```json
{
  "snapshot_id": "2026-11-16T09-31-12Z-a1b2c3",
  "parent": "2026-11-16T09-10-00Z-9f8e7d",
  "created_at": "2026-11-16T09:31:12Z",
  "files_backed_up": 128,
  "logical_size": 918273,
  "new_objects": 12,
  "new_bytes": 40960,
  "added": 3,
  "modified": 9,
  "deleted": 1,
  "unchanged": 103,
  "duration_secs": 1.9
}
```

Failure (on stderr, any command with `--json`):

```json
{
  "ok": false,
  "error": {
    "code": "root_not_found",
    "message": "backup root not found: projects"
  }
}
```

The `code` values are stable and match the desktop app's error codes.

## Examples

```bash
# Preview what a backup would upload
persist push --dry-run

# Back up one directory, non-interactively, and capture the snapshot id
id=$(persist push projects --yes --json | jq -r .snapshot_id)

# Compare the working tree with the head snapshot
persist diff

# Restore a single file from a specific snapshot to a chosen path
persist restore "$id" --path src/main.rs --destination /tmp/main.rs --yes

# Restore everything into a fresh directory, overwriting nothing
persist pull --snapshot "$id" --destination /tmp/recovered/ --yes

# Prove the repository is intact
persist verify --deep
```
