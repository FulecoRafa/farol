# Nix

`flake.nix` exports:

| output | what |
|---|---|
| `packages.<system>.farol` (= `default`) | `bin/farol`, the project in `share/farol` with the kit, a systemd user unit |
| `packages.<system>.phosphor-icons-fill` | the icon font the kit draws with (adwaita-shell's) |
| `apps.<system>.default` | `nix run` is `farol start` |
| `overlays.default` | adds `pkgs.farol` |
| `homeManagerModules.default` | `services.farol`: `enable`, `package`, `autostart`, `target`, `hotkeys`, `settings`, `fonts` |
| `nixosModules.default` | `programs.farol`: `enable`, `package`, `autostart`, `fonts` |
| `checks.<system>` | the package (its build runs the core's tests and checks the installed manifest) and `tsc` over the TSX |

Inputs: `tessela` (the runner; the core builds on its SDK, `sdk/rust`, from
the same source) and `adwaita-shell` (its `kit/`, and the Phosphor font),
both `git+file:` sibling checkouts locked to commits, with
`adwaita-shell.inputs.tessela.follows = "tessela"`. Taking newer commits:
`nix flake update tessela adwaita-shell`; trying uncommitted work:
`--override-input tessela path:/home/fuleco/Dev/tessela`.

## The package

```
bin/farol                        wrapper: TESSELA (default, the input's runner) and,
                                 after the host's PATH, wl-copy, xdg-open, notify-send
bin/.farol-wrapped               the core: the CLI, and with no arguments the service
share/farol/tessela.toml         command, kit and theme rewritten to store paths
share/farol/{src,kit}/           the surface; adwaita-shell's kit
share/farol/tsconfig.json        `@kit/*` → kit/*
share/systemd/user/farol.service (lib/systemd/user links to it) ExecStart=farol start
share/doc/farol, share/licenses/farol
```

`farol start` finds the project as `<bin>/../share/farol` and the runner
from `$TESSELA` (core/src/launch.rs), so `bin/farol` is all a user or a unit
needs. `postUnpack` copies `tessela.src` to `../tessela`, where
`core/Cargo.toml` looks for the SDK.

Shortcuts farol binds itself in Hyprland run the store path of the binary
that bound them; after an upgrade, restart farol (the service does on
`home-manager switch` when its unit changes) and it binds them again.

## Home Manager

```nix
# flake inputs
farol.url = "git+file:///home/fuleco/Dev/farol";

# a home-manager module
imports = [ inputs.farol.homeManagerModules.default ];
services.farol = {
  enable = true;
  # autostart = true;  # a user service on graphical-session.target, `farol start`
  hotkeys = { "super+alt+space" = ""; "super+v" = ">"; };
  settings = { terminal = [ "foot" ]; "web.search" = "https://duckduckgo.com/?q={query}"; };
};
```

`hotkeys` and `settings` are written to farol's database (`farol hotkey`,
`farol set`) on activation and in the unit's `ExecStartPre`; removing one
from the configuration does not delete it (`farol hotkey -d KEYS`). The
compositor has to export `WAYLAND_DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE`
to the systemd user manager (uwsm does) for the unit to work.

The module is not evaluated by `nix flake check` (that would need
home-manager as an input); it was evaluated with
`home-manager.lib.homeManagerConfiguration`, the unit and the
`farol-apply-config` script checked, and the script run against a scratch
database.

## NixOS

```nix
imports = [ inputs.farol.nixosModules.default ];
programs.farol.enable = true;
```

Installs farol and the fonts for every user and enables the packaged user
unit for `graphical-session.target` (`autostart = false` to skip).
