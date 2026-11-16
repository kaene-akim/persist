# Installing on Arch Linux

Persist is developed and tested on Arch Linux. This page covers building from
source. If you prefer, a `PKGBUILD` is provided in `packaging/arch/`.

## 1. Dependencies

### CLI only

```bash
sudo pacman -S --needed base-devel git rust
```

`rusqlite` is built with the bundled SQLite, so no separate SQLite package is
needed; `base-devel` provides the C compiler it uses.

### Desktop application (additional)

The Tauri backend needs the WebKitGTK stack and a few build tools:

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libsoup3 \
                       nodejs npm pkgconf
```

- `webkit2gtk-4.1` and `libsoup3` — the webview Tauri embeds.
- `gtk3` — windowing.
- `nodejs` / `npm` — to build the React frontend.

## 2. Build and install the CLI

From a checkout of the repository:

```bash
cargo build --release -p persist-cli
sudo install -Dm755 target/release/persist /usr/local/bin/persist
```

Verify:

```bash
persist --version
persist --help
```

## 3. Shell completions

```bash
persist completions bash | sudo tee /usr/share/bash-completion/completions/persist >/dev/null
persist completions zsh  | sudo tee /usr/local/share/zsh/site-functions/_persist >/dev/null
persist completions fish | sudo tee /usr/share/fish/vendor_completions.d/persist.fish >/dev/null
```

## 4. Build the desktop application (optional)

```bash
# 1. Build the frontend
cd crates/persist-app/ui
npm install
npm run build
cd -

# 2. Build the Tauri binary
cargo build --release -p persist-app
sudo install -Dm755 target/release/persist-app /usr/local/bin/persist-desktop
```

The window uses the icon set in `crates/persist-app/icons/`. Run it from a
terminal to see logs:

```bash
RUST_LOG=persist_app_lib=info,persist_core=info persist-desktop
```

Both the CLI and the desktop app read the same configuration and repository, so
you can switch between them freely.

## 5. Installing with the provided PKGBUILD

```bash
cd packaging/arch
makepkg -si
```

This builds `persist` (CLI). The PKGBUILD also defines a `persist-desktop`
split package; build both with:

```bash
makepkg -si        # installs every split package produced
```

The PKGBUILD assumes a tagged source release. If you are building from a local
checkout instead, see the comments at the top of the file.

## 6. First run

```bash
# Local repository — no account required
persist init "My backups" --local /mnt/backup/persist
persist add ~/projects
persist push --dry-run
persist push --yes
```

For Google Drive, configure an OAuth client first:
[docs/google-oauth.md](google-oauth.md).

## Notes

- Persist stores configuration under `$XDG_CONFIG_HOME/persist` and data under
  `$XDG_DATA_HOME/persist` (usually `~/.config/persist` and
  `~/.local/share/persist`). Override everything with `PERSIST_HOME`.
- Nothing essential lives outside the repository. Reinstalling the operating
  system loses only the local cache, which `persist clone` rebuilds.
- Uninstall with `sudo pacman -Rns persist` (or delete the binary you copied).
  Your repository and its backups are untouched.
