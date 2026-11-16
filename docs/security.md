# Security

Persist handles your data and your Google credentials, so its safety properties
are deliberate and documented rather than incidental. This page describes the
threat model, the controls in place, and — equally important — what Persist does
**not** protect against.

## Goals

1. A backup must never be lost or corrupted by a Persist bug.
2. Persist must never silently destroy or overwrite local data.
3. Persist must never leak credentials.
4. A malicious or corrupted repository must not be able to write outside the
   restore destination.

## Credentials

### OAuth client

No client secret is compiled into the binary. You supply your own client
([google-oauth.md](google-oauth.md)), either through `PERSIST_CLIENT_ID` /
`PERSIST_CLIENT_SECRET` or through `~/.config/persist/oauth.json`. If you store
it in the file, protect it:

```bash
chmod 600 ~/.config/persist/oauth.json
```

### Tokens

Tokens (including the refresh token) are stored, in order of preference:

1. **System keyring** via `secret-tool` (freedesktop Secret Service), under
   service `persist`, account `google-oauth`.
2. **Fallback file** `~/.local/share/persist/credentials.json`, written with
   `0600` permissions.

Tokens are never written into the repository, never included in logs, and never
printed. `persist logout` clears both the keyring entry and the fallback file.
`persist account` prints only the account's email and name.

### Least privilege

Persist requests `drive.file`, which grants access **only to files Persist
creates**. It cannot read unrelated files in your Drive. Revoking Persist from
your Google account's third-party access page removes its access entirely.

## Path safety

Restoring is the most dangerous operation, because it writes files. Persist
applies several independent defences:

- **Sanitisation.** Every relative path in a snapshot is validated on read.
  Absolute paths, `..` components and other escapes make the manifest invalid,
  so a hostile manifest is rejected before anything is written.
- **Destination confinement.** Each path is joined to the destination through a
  guard that refuses to escape it.
- **Symlink-ancestor check.** Before writing, Persist checks that no ancestor
  directory in the destination is a symlink pointing elsewhere. This closes the
  classic "plant a symlink, then write through it" attack.
- **No silent overwrite.** The default policy is to refuse to overwrite an
  existing file. Replacing data requires `--overwrite` (or the equivalent in the
  desktop app).
- **Atomic writes.** Files are streamed to a temporary name in the destination
  directory and renamed into place, so an interrupted restore never leaves a
  half-written file that looks complete.

## Integrity

- Every object is named by the SHA-256 of its contents.
- Objects are re-hashed when read; a mismatch is an error, not a warning.
- A snapshot is written **only after** every object it references is durable.
- With `verify_uploads` enabled (the default), each object is re-downloaded and
  re-hashed before the snapshot is committed.
- `persist verify [--deep]` audits the whole repository on demand.
- A failure never produces a snapshot that looks successful, and re-running an
  interrupted backup reuses already-uploaded objects.

## Symlink handling

Symlinks are a common source of escapes and loops. Persist records them as
links by default (`store_symlinks`) and does not follow them
(`follow_symlinks` off). When following is enabled, loops are detected and
stopped, and the symlink-ancestor guard still applies at restore time.

## Encryption — read this

**Persist does not encrypt your data.** Objects are stored exactly as they are
in the repository. The format was designed so an encrypted-object mode could be
added later, but that mode is not implemented.

Consequences:

- Anyone with access to the repository (a shared Drive, a mounted disk, a cloud
  provider) can read your files.
- Google Drive encrypts data in transit and at rest as part of its service, but
  that is the provider's protection, not Persist's.
- Do **not** treat a Persist repository as confidential.

If you need end-to-end confidentiality today, encrypt the sensitive directories
before adding them (for example with a `gocryptfs` mount or `restic`-style
encrypted archives) and back up the encrypted result.

## What Persist does not defend against

- **A compromised local machine.** Malware with your user privileges can read
  files and the token store.
- **A malicious provider.** Google could, in principle, alter stored objects;
  Persist detects this on read/verify but cannot prevent it.
- **Local deletion of a backup before it is pushed.** Persist backs up on
  demand; it does not continuously watch the filesystem.
- **Confidentiality of repository contents.** See the encryption section above.
- **An attacker who controls your OAuth client configuration.** Supplying a
  hostile `oauth.json` would redirect the flow to that client.

## Reporting a vulnerability

Please open a private security advisory on the project's repository rather than a
public issue, and include a minimal reproduction.
