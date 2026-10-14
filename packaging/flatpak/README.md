# Flatpak

`io.github.fuleco.Farol.yml` builds one app with everything in it: the
tessela runner (from tessela's own module file, `../tessela` next to this
repository), the Phosphor icon font, the core (`farol`, built from `core/`
with tessela's SDK from the same sibling checkout) and the project under
`/app/share/farol` with adwaita-shell's `kit/` (from `../adwaita-shell`)
beside it, its `tessela.toml` pointing at `/app/bin/farol` and that kit.

```
/app/bin/tessela          the runner
/app/bin/farol            the core; `farol start` finds the runner beside it
                          and the project in ../share/farol
/app/bin/farol-flatpak    the app's command: no arguments → `farol start`,
                          anything else → the farol CLI
/app/share/farol/         tessela.toml, src/, kit/
```

**Prefer the Nix package or the RPM.** A launcher's job is to start any
program, so this sandbox is nominal: the app may run anything on the host.

## Building and running

Needs `org.freedesktop.Platform//26.08`, its SDK and
`org.freedesktop.Sdk.Extension.rust-stable//26.08` (from Flathub), and
`flatpak-builder`. Build outside the repository:

```sh
flatpak-builder --user --install-deps-from=flathub --install --force-clean \
    --state-dir ~/.cache/flatpak-builder ~/build/farol \
    packaging/flatpak/io.github.fuleco.Farol.yml
flatpak run io.github.fuleco.Farol                 # farol start
flatpak run io.github.fuleco.Farol toggle          # farol toggle
flatpak run io.github.fuleco.Farol doctor
```

The core's crates come from `core-cargo-sources.json`; regenerate it
whenever `core/Cargo.lock` changes (the generator is described in
`../tessela/packaging/flatpak/README.md`):

```sh
python3 flatpak-cargo-generator.py core/Cargo.lock \
    -o packaging/flatpak/core-cargo-sources.json
```

(The SDK, a path dependency, is not in it: the manifest copies the tessela
checkout beside this one.)

Keybindings, in hyprland.conf:

```
bind = SUPER ALT, space, exec, flatpak run io.github.fuleco.Farol toggle
exec-once = flatpak run io.github.fuleco.Farol
```

or a user unit with `ExecStart=/usr/bin/flatpak run io.github.fuleco.Farol`
bound to `graphical-session.target`.

## How the sandbox is opened, and why

| need | how |
|---|---|
| the launcher surface (layer-shell) and the clipboard (data-control) | **not** `--socket=wayland`: through Flatpak's `wp_security_context_v1` Hyprland and sway hide `zwlr_layer_shell_v1` (and other privileged protocols). The app gets the compositor's own socket, `--filesystem=xdg-run/wayland-0` and `wayland-1`, and `farol-flatpak` sets `WAYLAND_DISPLAY` again. Another name: `flatpak run --filesystem=xdg-run/wayland-2 --env=FAROL_WAYLAND_DISPLAY=wayland-2 io.github.fuleco.Farol`. |
| GPU | `--device=dri`. |
| starting apps, `xdg-open`, `notify-send`, `wl-copy` | `--talk-name=org.freedesktop.Flatpak`: with `FLATPAK_ID` set, the core's `spawn` and `pipe` run them through `flatpak-spawn --host` (core/src/platform/mod.rs). They get the environment of the host's session helper, so the compositor must export `WAYLAND_DISPLAY` to the systemd user manager (uwsm does). |
| notifications | `--talk-name=org.freedesktop.Notifications` (`notify-send` itself runs on the host). |
| windows, workspaces, shortcuts | `--filesystem=xdg-run/hypr` (Hyprland's IPC; `HYPRLAND_INSTANCE_SIGNATURE` passes through). |
| `farol toggle` from a keybinding | `--filesystem=xdg-run/tessela:create`: the runner's sockets live in the host's `$XDG_RUNTIME_DIR/tessela/farol`, shared by every instance of the app, so `flatpak run io.github.fuleco.Farol toggle` (a new instance) reaches the running one. A native farol and the Flatpak share that name: `farol start` in either sees the other running. |
| files, previews, desktop entries, icons, bookmarks | `--filesystem=home:ro`; `--filesystem=host-os:ro` (the host's `/usr` as `/run/host/usr`) and `/var/lib/flatpak/exports/share:ro`, added to `XDG_DATA_DIRS` by the wrapper. |

## What still differs from a native install

- **`farol hotkey`**: the keys farol binds in Hyprland run the binary that
  bound them, `/app/bin/farol`, which does not exist on the host; bind keys
  in hyprland.conf instead (above).
- **Desktop entries on NixOS** live in `/run/current-system/sw/share` and
  `/etc/profiles`, which the sandbox does not see; only Flatpak apps and
  `~/.local/share/applications` are listed there.
- **State**: the database is in
  `~/.var/app/io.github.fuleco.Farol/data/farol/farol.db`, not shared with a
  native install.
- **Fonts**: "CaskaydiaCove Nerd Font" is not bundled; install it in
  `~/.local/share/fonts` (on NixOS copy it to
  `~/.var/app/io.github.fuleco.Farol/data/fonts`).
- **Time zone on NixOS** (alarms): `flatpak override --user
  --env=TZ=America/Sao_Paulo io.github.fuleco.Farol`.
- **`farol dev`**, `--nested`: development only, not in the sandbox.
- **`farol autostart on`** writes a unit running `/app/bin/farol start`,
  which does not exist on the host; use the unit above.

## Verified

Built with flatpak-builder 1.4 against 26.08 and installed (`--user`) on
NixOS. Then, with `HYPRLAND_INSTANCE_SIGNATURE` and `WAYLAND_DISPLAY`
unset and a scratch database:

- `flatpak run --command=tessela io.github.fuleco.Farol run /app/share/farol
  --headless --seconds 8 --name farol-pkgtest` started the core; its
  `farol.sock` appeared on the host in
  `$XDG_RUNTIME_DIR/tessela/farol-pkgtest/services`;
- from a second instance, `flatpak run --env=FAROL_PROJECT=farol-pkgtest
  io.github.fuleco.Farol doctor` reached it (project `/app/share/farol`,
  runner `/app/bin/tessela`), `query` answered and `show` drew the launcher
  in the headless renderer;
- with the `browser` setting pointed at a shell that writes to a path only
  the host has, `farol run web:https://...` wrote it on the host: spawning
  goes through `flatpak-spawn --host`.

Not verified: the surface in a real session (Wayland socket, Hyprland,
the clipboard), and launching an app from a desktop entry.
