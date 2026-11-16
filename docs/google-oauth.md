# Google Drive setup (OAuth)

Persist talks to Google Drive with **your own** OAuth client. No client secret
is compiled into the binary, and Persist never asks for your Google password.

The flow is the standard **installed application** flow:

1. Persist starts a temporary web server on `127.0.0.1` (a random port).
2. It opens your browser to Google's consent page, using **PKCE**.
3. Google redirects back to the loopback server with an authorization code.
4. Persist exchanges the code for an access token and a refresh token, and
   stores them securely.

## Scopes

Persist requests the minimum it needs:

| Scope | Why |
| --- | --- |
| `openid` | Identify the account. |
| `https://www.googleapis.com/auth/userinfo.email` | Show which account is connected. |
| `https://www.googleapis.com/auth/drive.file` | **Only** the files Persist itself creates. |

`drive.file` is important: Persist can see and manage the files it created, but
it cannot read the rest of your Drive. Revoking Persist's access removes its
ability to touch anything.

## 1. Create an OAuth client

1. Open the [Google Cloud Console](https://console.cloud.google.com/).
2. Create (or choose) a project.
3. **APIs & Services → Library**, enable **Google Drive API**.
4. **APIs & Services → OAuth consent screen**:
   - Choose **External** (or **Internal** for a Workspace org).
   - Fill in the app name and support email.
   - Add the scopes above (`.../auth/drive.file`, `.../auth/userinfo.email`,
     `openid`).
   - While the app is in **Testing**, add your own Google account under **Test
     users**.
5. **APIs & Services → Credentials → Create credentials → OAuth client ID**:
   - Application type: **Desktop app**.
   - Name it, for example, `Persist`.
6. Copy the **Client ID** and **Client secret**.

> The loopback redirect (`http://127.0.0.1:<random port>`) is registered
> automatically for the **Desktop app** type. You do **not** need to add a
> redirect URI by hand.

## 2. Give Persist the client

Choose one method.

### Environment variables (quickest)

```bash
export PERSIST_CLIENT_ID="1234567890-abcdef.apps.googleusercontent.com"
export PERSIST_CLIENT_SECRET="GOCSPX-…"
```

The secret may be omitted for client types that do not use one; Persist sends it
only when it is set and non-empty.

### Configuration file (persistent)

Create `~/.config/persist/oauth.json` (on Linux; the directory follows
`$XDG_CONFIG_HOME`):

```json
{
  "client_id": "1234567890-abcdef.apps.googleusercontent.com",
  "client_secret": "GOCSPX-…",
  "scopes": [
    "openid",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/drive.file"
  ],
  "auth_uri": "https://accounts.google.com/o/oauth2/v2/auth",
  "token_uri": "https://oauth2.googleapis.com/token"
}
```

`scopes`, `auth_uri` and `token_uri` are optional and default to the values
above. Environment variables take precedence over the file.

The file is read with normal permissions, but it contains a secret — protect it:

```bash
chmod 600 ~/.config/persist/oauth.json
```

## 3. Authorise

```bash
persist login
```

A browser opens. Approve the request; the terminal reports the connected
account. Verify with:

```bash
persist account
```

If the browser cannot be opened, use `persist login --no-browser` and paste the
printed URL into any browser on the same machine.

## Where tokens are stored

Persist prefers the **system keyring**:

- On Linux it uses `secret-tool` (libsecret / GNOME Keyring / KWallet).
- If no keyring is available, it falls back to
  `~/.local/share/persist/credentials.json`, written with `0600` permissions.

Tokens are never logged and never written into the repository.

To revoke access completely:

1. `persist logout` (removes local tokens), and/or
2. Remove Persist from
   [your Google account's third-party access](https://myaccount.google.com/permissions).

## Token refresh

Access tokens expire after about an hour. Persist refreshes them automatically
using the stored refresh token and the client credentials. A refresh failure
surfaces as an authentication error (exit code `3`) with a message telling you
to run `persist login` again.

## Troubleshooting

| Symptom | Likely cause / fix |
| --- | --- |
| `no Google OAuth client configured` | Set `PERSIST_CLIENT_ID` or create `oauth.json`. |
| `redirect_uri_mismatch` | The client is not of type **Desktop app**. Recreate it with that type. |
| `access_denied` | Your account is not a **test user** on a Testing consent screen, or you declined. |
| `403 … insufficient … scopes` | Enable the **Google Drive API** for the project and re-run `persist login`. |
| `TokenExpired` / auth error later | `persist login` again to obtain a fresh refresh token. |
| Browser does not open | Use `persist login --no-browser`. |
| Works, then fails on another machine | Run `persist login` there too; tokens are per-machine. |

See also [troubleshooting.md](troubleshooting.md).
