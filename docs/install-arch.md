# Installing Persist on Arch Linux

Persist is developed and tested on Arch Linux. A **full installation** gives you
both front ends around the same engine:

| Split package | What it installs | Why you want it |
| --- | --- | --- |
| `persist` | `/usr/bin/persist`, bash/zsh/fish completions, documentation | Command-line backups and recovery |
| `persist-desktop` | `/usr/bin/persist-desktop`, a `Persist` launcher entry, icons in `hicolor` | Desktop app you can open from your application launcher |

Both packages are defined in `packaging/arch/PKGBUILD`. Install both unless you
only need the terminal.

## 1. Dependencies

`makepkg -s` (below) installs these automatically. They are listed here for a
manual build.

### CLI only

```bash
sudo pacman -S --needed base-devel git rust
```

`rusqlite` is built with the bundled SQLite, so no separate SQLite package is
needed; `base-devel` provides the C compiler it uses.

### Desktop application (additional)

The Tauri backend needs the WebKitGTK stack and a few build tools:

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libsoup3 nodejs npm pkgconf
```

- `webkit2gtk-4.1` and `libsoup3` — the webview Tauri embeds.
- `gtk3` — windowing.
- `nodejs` / `npm` — to build the React frontend.

## 2. Full install with the PKGBUILD (recommended)

From any directory:

```bash
git clone https://github.com/kaene-akim/persist.git
cd persist/packaging/arch
makepkg -si
```

- `makepkg` builds the React frontend and the Rust workspace, then produces two
  packages: `persist-0.1.0-1-*.pkg.tar.zst` and
  `persist-desktop-0.1.0-1-*.pkg.tar.zst`.
- `-s` installs any missing build dependencies; `-i` installs **both** split
  packages, including the launcher entry and icons.

To install only one front end, add `--pkg`:

```bash
makepkg -si --pkg persist          # CLI only
makepkg -si --pkg persist-desktop  # desktop app only
```

The PKGBUILD tracks the `main` branch. For a pinned release, change
`#branch=main` to `#tag=v0.1.0` in its `source` line.

## 3. Verify the installation

```bash
persist --version                  # persist 0.1.0
persist --help
command -v persist-desktop         # /usr/bin/persist-desktop (or ~/.local/bin)
ls /usr/share/applications/persist.desktop
```

## 4. Launch from your app launcher

1. Open your application launcher and search for **Persist**. For example:
   - **GNOME:** press Super, type `Persist`, press Enter.
   - **KDE Plasma:** open the application menu → *Utilities* or *Archiving*.
   - **niri / wlroots:** open your launcher (Rofi, Walker, etc.) and type `Persist`.
2. Select the entry named **Persist** (comment: *Incremental, versioned
   backups*). It uses the `persist` icon and starts `persist-desktop`.
3. On first launch the window shows **Set up Persist**: choose to create a new
   repository or recover an existing one, and pick a local folder or Google
   Drive. After setup, use **My Backups** to add the directories to protect and
   start a snapshot.

You can also start it from a terminal:

```bash
persist-desktop
```

With verbose logging (useful when diagnosing a launcher problem):

```bash
RUST_LOG=persist_app_lib=info,persist_core=info persist-desktop
```

If the entry does not show up right away, log out and back in, or refresh the
desktop database:

```bash
sudo update-desktop-database /usr/share/applications
# for a user-level install instead:
update-desktop-database ~/.local/share/applications
```

## 5. Manual install from source

Use this if you do not want `makepkg`, or if you are on another distribution.

### 5.1 Build the CLI

```bash
git clone https://github.com/kaene-akim/persist.git
cd persist
cargo build --release -p persist-cli
sudo install -Dm755 target/release/persist /usr/local/bin/persist
```

### 5.2 Build the desktop app

The React frontend is embedded at compile time, so build it first:

```bash
cd crates/persist-app/ui
npm ci
npm run build
cd ../..
cargo build --release -p persist-app
```

### 5.3 Register the app with your launcher

Create the desktop entry:

```bash
cat > /tmp/persist.desktop <<'EOF'
[Desktop Entry]
Type=Application
Version=1.0
Name=Persist
GenericName=Backup Utility
Comment=Incremental, versioned backups
Exec=persist-desktop
Icon=persist
Terminal=false
Categories=Utility;Archiving;FileTools;
Keywords=backup;restore;snapshots;drive;
StartupWMClass=persist-desktop
EOF
```

Install the binaries, icons and entry system-wide:

```bash
sudo install -Dm755 target/release/persist-app /usr/local/bin/persist-desktop
for size in 32x32 128x128 256x256 512x512; do
  case "$size" in
    32x32)   src=32x32.png ;;
    128x128) src=128x128.png ;;
    256x256) src=128x128@2x.png ;;
    512x512) src=icon.png ;;
  esac
  sudo install -Dm644 "crates/persist-app/icons/$src" \
    "/usr/local/share/icons/hicolor/$size/apps/persist.png"
done
sudo install -Dm644 /tmp/persist.desktop \
  /usr/local/share/applications/persist.desktop
sudo update-desktop-database /usr/local/share/applications
```

**Without root** — install under `$HOME` instead. Make sure
`$HOME/.local/bin` is in your `PATH` (it already is on most Arch setups):

```bash
install -Dm755 target/release/persist "$HOME/.local/bin/persist"
install -Dm755 target/release/persist-app "$HOME/.local/bin/persist-desktop"
install -Dm644 crates/persist-app/icons/32x32.png \
  "$HOME/.local/share/icons/hicolor/32x32/apps/persist.png"
install -Dm644 crates/persist-app/icons/128x128.png \
  "$HOME/.local/share/icons/hicolor/128x128/apps/persist.png"
install -Dm644 crates/persist-app/icons/128x128@2x.png \
  "$HOME/.local/share/icons/hicolor/256x256/apps/persist.png"
install -Dm644 crates/persist-app/icons/icon.png \
  "$HOME/.local/share/icons/hicolor/512x512/apps/persist.png"
install -Dm644 /tmp/persist.desktop \
  "$HOME/.local/share/applications/persist.desktop"
update-desktop-database "$HOME/.local/share/applications"
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
```

Then follow [section 4](#4-launch-from-your-app-launcher) to launch it.

### 5.4 Shell completions (CLI)

```bash
persist completions bash | sudo tee /usr/share/bash-completion/completions/persist >/dev/null
persist completions zsh  | sudo tee /usr/local/share/zsh/site-functions/_persist >/dev/null
persist completions fish | sudo tee /usr/share/fish/vendor_completions.d/persist.fish >/dev/null
```

For a user-level install, write to the matching directories under
`$HOME/.local/share` instead; no `sudo` needed.

## 6. First run

**Desktop:** open **Persist** from your launcher and follow the setup screen.
For Google Drive, connect an account in **Settings** first; see
[google-oauth.md](google-oauth.md).

**Terminal:** a local repository needs no account:

```bash
persist init "My backups" --local /mnt/backup/persist
persist add ~/projects
persist push --dry-run
persist push --yes
```

Both front ends read the same configuration and repository, so you can switch
between them freely.

## 7. Update and uninstall

```bash
# PKGBUILD install: pull the new source, then rebuild/reinstall
cd persist && git pull && cd packaging/arch
makepkg -si

# System packages
sudo pacman -Rns persist persist-desktop
```

For a manual install, delete the binaries, desktop entry and icons you copied.
Your repository and backups are never removed by uninstalling Persist.

## Notes

- Persist stores configuration under `$XDG_CONFIG_HOME/persist` and data under
  `$XDG_DATA_HOME/persist` (usually `~/.config/persist` and
  `~/.local/share/persist`). Override everything with `PERSIST_HOME`.
- Nothing essential lives outside the repository. Reinstalling the operating
  system loses only the local cache, which `persist clone` rebuilds.
- Launcher lookup uses `XDG_DATA_DIRS`. If you installed manually to
  `/usr/local/share`, make sure that directory is present (it is by default on
  Arch).
