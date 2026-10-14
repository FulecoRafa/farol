# RPM (Fedora)

`farol.spec` builds the core (`core/`, Rust) offline from vendored crates
and installs:

| path | what |
|---|---|
| `/usr/bin/farol` | the core: the CLI (`farol start`, `toggle`, `doctor`, `hotkey`, `set`, ...) and, with no arguments, the service the runner starts |
| `/usr/share/farol/` | the project: `tessela.toml` (its `[plugins.core]` command patched to `/usr/bin/farol`, `[imports] kit` and `themes` to the kit below), `src/`, and `kit/` (adwaita-shell's) |
| `/usr/lib/systemd/user/farol.service` | `farol start` with `graphical-session.target` |
| `/usr/share/fonts/farol/Phosphor-Fill.ttf` | the icon font the kit draws glyphs with |

`farol start` finds the project as `/usr/bin/../share/farol` and the runner
as `/usr/bin/tessela`, beside itself: it needs the `tessela` package (built
from `../tessela/packaging/rpm`).

## Building

`core/Cargo.toml` takes tessela's SDK from `../../tessela/sdk/rust`, and
`tessela.toml` imports adwaita-shell's kit, so `make-sources.sh` also
archives a tessela and an adwaita-shell checkout (by default the siblings of
the main checkout, `../tessela` and `../adwaita-shell`; `TESSELA=<dir>`,
`ADWAITA=<dir>`, `TESSELA_REF`, `ADWAITA_REF`, default `HEAD`) and vendors
`core/Cargo.lock` against tessela. It needs git, curl, cargo and xz:

```sh
nix develop ../tessela -c packaging/rpm/make-sources.sh ~/rpmbuild/SOURCES
#   farol-0.1.0.tar.gz, Phosphor-Fill.ttf,
#   tessela-0.1.0.tar.xz (the same file tessela.spec builds from),
#   adwaita-shell-0.1.0.tar.gz (the same file adwaita-shell.spec builds from),
#   farol-0.1.0-vendor.tar.xz (core/cargo-vendor)
sudo dnf builddep packaging/rpm/farol.spec
rpmbuild -ba packaging/rpm/farol.spec
sudo dnf install ~/rpmbuild/RPMS/x86_64/{farol,tessela}-0.1.0-*.rpm
systemctl --user enable --now farol.service   # or run `farol start`
```

`%prep` unpacks the tessela tarball to `../tessela` beside the source tree
and takes `kit/` from the adwaita-shell tarball; `%check` greps the
installed `tessela.toml` and runs the core's tests (`--without check` skips
the tests).

In a container (as verified, see below):

```sh
docker run --rm --cpus=4 -v pkg-dnf-cache:/var/cache/dnf \
    -v ~/rpmbuild/SOURCES:/root/rpmbuild/SOURCES -v "$PWD/packaging/rpm:/specs:ro" \
    fedora:latest sh -c 'dnf -y install rpm-build && dnf -y builddep /specs/farol.spec &&
                         rpmbuild -ba /specs/farol.spec'
```

With Hyprland under uwsm the unit starts with the session; otherwise put
`exec-once = farol start` in hyprland.conf instead of enabling the unit.
`farol autostart on` writes a unit of the same name to
`~/.config/systemd/user`, which takes precedence over the packaged one.

## Runtime dependencies

| command | for | Fedora package | |
|---|---|---|---|
| `tessela` | the runner | tessela | Requires |
| `xdg-open` | opening files, folders, URLs | xdg-utils | Requires (the file) |
| `notify-send` | timers and alarms | libnotify | Requires (the file) |
| `wl-copy` | copying when farol's own clipboard (data-control) is not running | wl-clipboard | Recommends |
| Hyprland | windows, workspaces, shortcuts | hyprland | Recommends |

The theme asks for "CaskaydiaCove Nerd Font", which Fedora does not ship;
`cascadia-mono-fonts` is recommended and fontconfig falls back without it.
The default terminal and editor (`ghostty`, `hx`) are settings, not
dependencies (`farol set terminal '["foot"]'`).

## License, and when core/Cargo.lock changes

`License:` is the project's `(MIT OR Apache-2.0) AND MIT` (farol and the
kit, the Phosphor font) and the licenses of the crates linked into `farol`,
from `%cargo_license_summary`, which the build prints. When
`core/Cargo.lock` changes:

1. rerun `make-sources.sh` (the vendor tarball follows the lock);
2. rebuild, and copy the summary from the build log into `License:`;
3. the package ships `LICENSE.dependencies` and `cargo-vendor.txt`, from
   which rpm generates the `bundled(crate(...))` provides; the SDK's path
   entry is dropped from `cargo-vendor.txt` (the generator cannot parse it)
   and provided by hand as `bundled(crate(tessela-service))`.

The `URL:` is a provisional placeholder (`https://example.invalid/farol`).

## Verified

In `fedora:latest` (Fedora 44): `tessela.spec` built (`--without check`)
and `farol.spec` built with its `%check` (manifest greps, 33 core tests);
both RPMs installed with dnf, then as a normal user without a display:
`tessela run /usr/share/farol --headless --seconds 6 --name farol-pkgtest`
started the core (`farol.sock` in `$XDG_RUNTIME_DIR/tessela/farol-pkgtest/services`),
`farol doctor` reported project `/usr/share/farol` and runner
`/usr/bin/tessela`, `farol query '5 km to mi'` answered and `farol show`
drew the launcher in the headless renderer; `farol start` resolved the same
project and runner by itself. Not verified: a real session (Wayland
surface, Hyprland, the user unit under systemd).
