# farol

A launcher on [tessela](../tessela), light before anything else: one
resident process that sleeps while closed, a search core in Rust, and a
TSX surface that only draws the rows in view. Components come from
adwaita-shell's `@kit`.

```
tessela.toml        the surface, its capabilities and the core plugin
src/surfaces/       launcher.tsx: field, grouped rows, preview, actions
src/components/     row icons, the preview pane
src/data/farol.ts   the farol service, typed
core/               the core (Rust, tessela-service): search, providers, SQLite
examples/           an external plugin in plain Python
docs/PLANO.md       plan, principles and budget (Portuguese)
flake.nix, nix/     Nix package, Home Manager and NixOS modules
packaging/          RPM, Flatpak, Homebrew
```

## Running

```sh
cargo build --release --manifest-path core/Cargo.toml
cd ../tessela && ./target/release/tessela run ../farol      # or --nested to try it headless
farol toggle                                               # open/close (core/target/release/farol)
farol hotkey super+alt+space && farol hotkey super+v '>'   # shortcuts, bound when farol starts
```

## Installing

Every package installs the same layout: `farol` (the CLI, and with no
arguments the service) and the project in `share/farol` with adwaita-shell's
`kit/` beside it, its `tessela.toml` pointing at both; `farol start` finds
the project and the tessela runner from where it is installed. Then open it
with `farol toggle` from a compositor keybinding, or let farol bind keys in
Hyprland (`farol hotkey super+alt+space`). Details per format in
[nix/README.md](nix/README.md) and `packaging/*/README.md`.

**Nix** (flake; tessela and adwaita-shell are inputs):

```sh
nix run .                      # farol start
nix build                      # result/bin/farol
```

```nix
# Home Manager
imports = [ inputs.farol.homeManagerModules.default ];
services.farol = {
  enable = true;                                # autostart: a user service on graphical-session.target
  hotkeys = { "super+alt+space" = ""; "super+v" = ">"; };
  settings = { terminal = [ "foot" ]; };        # farol set KEY JSON
};
# NixOS: imports = [ inputs.farol.nixosModules.default ]; programs.farol.enable = true;
```

**RPM** (Fedora; needs the `tessela` RPM from tessela's `packaging/rpm`):

```sh
nix develop ../tessela -c packaging/rpm/make-sources.sh ~/rpmbuild/SOURCES
rpmbuild -ba packaging/rpm/farol.spec
sudo dnf install ~/rpmbuild/RPMS/x86_64/{farol,tessela}-0.1.0-*.rpm
systemctl --user enable --now farol.service          # or: farol start
```

**Flatpak** (runner, core, kit and icon font in one app; the sandbox is
nominal, programs start on the host through `flatpak-spawn --host`):

```sh
flatpak-builder --user --install-deps-from=flathub --install --force-clean \
    ~/build/farol packaging/flatpak/io.github.fuleco.Farol.yml
flatpak run io.github.fuleco.Farol                   # farol start
flatpak run io.github.fuleco.Farol toggle            # any other argument: the farol CLI
```

**Homebrew** (Linux only, a tap next to tessela's formula):

```sh
brew install FulecoRafa/tessela/farol
farol autostart on && farol start
```

When `core/Cargo.lock` changes, the RPM's vendored crates and `License:`
and the Flatpak's `core-cargo-sources.json` must be regenerated (see their
READMEs); the Nix package and the formula read the lock directly.

## What it finds

| type | does |
|---|---|
| apps | installed apps; Enter opens, Ctrl+C copies the command |
| `=` or bare math | fend: `2^10`, `5 km em mi`, `0xff to decimal` |
| `:` | emoji by name or shortcode; Enter copies |
| `/` | files in the home as you type (`/ nix/hosts` matches paths), `/` alone the week's; preview of text, images, folders; drag a row onto any window |
| `>` | clipboard history (Wayland data-control, no polling): text and images, Ctrl+D deletes, Alt+P pins |
| `!` | open windows to close or kill; windows also join the normal search (Enter goes to them) |
| window commands | close, kill, float, fullscreen, pin, center the window you were in; `ws 3`, `ir 3` |
| `timer 10m chá`, `alarme 7:30`, `pomodoro` | timers kept in the database, notified even with farol closed; running ones show with the field empty |
| `?` and any text | web search (`web.search`), addresses, Helium bookmarks, quicklinks: `link gh https://github.com/search?q={query}` then `gh tessela` |
| shortcuts | the desktop's own (Hyprland), searchable by what they do; `atalho super+alt+space`, Ctrl+S on any row to bind it |
| `alias zap = whatsapp` | aliases: `zap` brings WhatsApp first; Ctrl+A on any row |
| plugins | any process serving a tessela service socket with `query`/`activate`, listed in the `plugins` setting |

## Settings

Ctrl+, in the launcher, `farol settings` or the "configurações do farol" row
open the settings window: aliases and shortcuts (created by name, listed as
what they open, next to the desktop's own shortcuts), links, plugins,
starting at login, the programs farol uses and a diagnostics page.
`farol autostart on` does the same from a terminal: a systemd user unit on
the graphical session (XDG autostart without systemd), a LaunchAgent on
macOS, a Startup script on Windows.

Options live in the same SQLite file (`farol set KEY JSON`, `farol options`):
`terminal`, `terminal.here`, `editor`, `browser`, `web.search`,
`web.fallback`, `web.bookmarks`, `files.roots`, `files.hidden`,
`files.mixed`, `clipboard.limit`, `clipboard.primary`, `prefixes`, `plugins`.

## For developers

```sh
farol dev            # a second copy, farol-dev (own sockets and database), with
                     # tessela's devtools served and their window opened
farol dev --nested   # the same inside a headless sway: nothing on your screen
farol doctor         # paths, what works on this desktop, timings per provider
farol query TEXT     # one search answered in the terminal, with its time
```

The devtools show the live view tree, every call to the farol service,
renders and frames; `tessela devtools --project farol-dev shot launcher`
captures the window. Plugins: see `core/src/providers/external.rs` and
`examples/hello-plugin.py`.

## Platforms

Linux (Wayland, Hyprland for windows and shortcuts) is complete. On macOS
farol runs from a checkout (tessela's `macos` branch, with adwaita-shell
beside it, and the Phosphor-Fill font installed): surfaces are floating
panels, apps come with their icons, and opening, copying, files and
notifications work. Windows, global shortcuts (bind `farol toggle` with
Shortcuts or skhd meanwhile) and the clipboard watcher are not wired there
yet; Windows has the core's code paths only (plan, phase 5).
