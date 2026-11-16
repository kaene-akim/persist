# Troubleshooting

Find the message or symptom, then apply the fix. If a command fails, its
**exit code** (see [cli.md](cli.md#exit-codes)) narrows down the cause.

## Quick diagnosis

```bash
persist status            # is a repository present and reachable?
persist config            # backend, paths and settings
persist verify            # is the repository intact?
persist --json … 2>&1 | jq   # structured error with a stable code
```

## Authentication

| Symptom | Cause | Fix |
| --- | --- | --- |
| `no Google OAuth client configured` | No client id | Set `PERSIST_CLIENT_ID` or create `~/.config/persist/oauth.json` ([google-oauth.md](google-oauth.md)). |
| `redirect_uri_mismatch` | Client is not type **Desktop app** | Recreate the OAuth client with the Desktop app type. |
| `access_denied` | Consent screen in Testing and you are not a test user, or you declined | Add your account as a test user, then `persist login` again. |
| `403 … insufficient … scopes` | Drive API not enabled | Enable **Google Drive API** in the Cloud project, then `persist login`. |
| Auth error after weeks of working | Refresh token revoked or expired | `persist login` again. |
| `secret-tool: command not found` | No Secret Service | Harmless: Persist falls back to a `0600` file. Install `libsecret` for keyring storage. |
| Tokens seem to vanish | Different `PERSIST_HOME` / user | Tokens are per user and per `PERSIST_HOME`. Re-run `persist login`. |

Exit code `3` always means authentication.

## Google Drive

| Symptom | Cause | Fix |
| --- | --- | --- |
| Very slow uploads | High concurrency or small chunks on a slow link | `persist config set upload_concurrency 2`. |
| `storage is full` (exit 4) | Drive quota exhausted | Free space or use another account. |
| `429` / `rate limited` | Too many requests | Wait and retry; lower `upload_concurrency`. |
| Repository "not found" after connecting | Folder name differs | `persist config set drive_folder "<name>"` then `persist clone`. |
| Two machines disagree on history | They point at different folders | Compare `persist config` → `drive_folder` and `active repository`. |

> Reminder: the live Google Drive path has **not** been exercised against a real
> account in this build. If something behaves unexpectedly on the first run, use
> `--dry-run` and `--json` to inspect each step, and check
> `RUST_LOG=persist_core=debug persist …` for detail.

## Local backend

| Symptom | Cause | Fix |
| --- | --- | --- |
| `directory does not exist` (exit 5) | Repository path missing | Recreate the folder, or `persist config set local_path <path>`. |
| `permission denied` | Wrong ownership/permissions | `chmod`/`chown` the repository directory. |
| `local disk is full` | Destination disk full | Free space; nothing was committed. |

## Backups

| Symptom | Cause | Fix |
| --- | --- | --- |
| `Everything is already backed up` | No changes since the last snapshot | Expected. |
| Expected files missing | Excluded by a pattern | Check `persist config get default_exclusions` and the root's `--exclude` list. |
| Large files skipped | `max_file_size` set | `persist config set max_file_size 0`. |
| Backup seems to upload everything | First snapshot, or files change during scan | Expected on the first run; otherwise check for a tool rewriting files constantly. |
| `Ctrl-C` mid-upload | Interrupted (exit 131) | Nothing is lost; re-run `persist push` and it resumes from uploaded objects. |

## Restores

| Symptom | Cause | Fix |
| --- | --- | --- |
| `refusing to overwrite existing file` (exit 7) | Default conflict policy | Re-run with `--overwrite`, `--skip` or `--rename`; or restore to a fresh `--destination`. |
| `path escapes the destination` (exit 7) | Malformed/hostile manifest, or an odd `--path` | Verify the repository; use a clean `--destination`. |
| Nothing written but exit 0 | You used `--dry-run` | Drop `--dry-run` to actually write. |
| Single file landed in the wrong place | Destination ambiguity | End the destination with `/` to force directory semantics, or give an exact file path. |
| Permissions wrong after restore | Filesystem differences | Persist preserves recorded modes where the OS allows. |

## Verification

| Symptom | Cause | Fix |
| --- | --- | --- |
| `missing object` (exit 6) | Object deleted or never uploaded | Restore from a snapshot that predates the loss; re-push from the source if available. |
| `corrupt object` (exit 6) | Stored bytes do not match their hash | The object must be re-uploaded: remove it locally, then `persist push` again. |
| `orphan objects` reported | Objects present but unreferenced | Harmless; they waste a little space. |
| `snapshot uses format N which is newer` | Repository written by a newer Persist | Update Persist. |

## Building and running

| Symptom | Cause | Fix |
| --- | --- | --- |
| `webkit2gtk-4.1 not found` when building the app | Missing WebKitGTK | `sudo pacman -S webkit2gtk-4.1 gtk3 libsoup3`. |
| Frontend build fails on `esbuild` postinstall | Install scripts blocked by npm | Run `npm install` again, or approve the script (`npm install-scripts approve esbuild`). |
| `frontendDist … doesn't exist` | `ui/dist` not built | `cd crates/persist-app/ui && npm run build`. |
| App window is blank | Frontend not rebuilt after changes | Rebuild the frontend, then rebuild the app. |
| CLI prints no progress bar | Not a TTY, or `--json` | Expected: bars are TTY-only. |

## Getting more detail

```bash
# Verbose core logging
RUST_LOG=persist_core=debug persist status

# Structured errors
persist push --json 2>&1 | jq

# A safe sandbox with an isolated home
PERSIST_HOME=$(mktemp -d) persist init "Test" --local /tmp/test-repo
```

## Still stuck?

Collect the following before asking for help:

```bash
persist --version
persist config
persist status --json
```

Do **not** paste token files or `oauth.json`. Redact client ids if you share
`config` output.
