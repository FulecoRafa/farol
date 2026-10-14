# Homebrew

`farol.rb` is a formula for the same tap as tessela's `tessela.rb` (it
`depends_on "tessela"`). It is **Linux only**, for the same reasons the
runner is (see `../tessela/packaging/homebrew/README.md`): layer-shell
surfaces, Hyprland, freedesktop desktop entries.

## What it installs

```
bin/farol            env script: TESSELA (tessela's opt_bin, unless set) and
                     FAROL_PROJECT_DIR (opt_pkgshare, unless set)
libexec/bin/farol    the core (cargo install of core/)
share/farol/         tessela.toml (command, kit and theme rewritten to opt paths), src/, kit/
```

The binary lives in `libexec/bin`, so `farol start` could not find
`../share/farol` or a `tessela` beside it on its own; the env script tells
it. `farol autostart on` writes the systemd user unit (bound to
`graphical-session.target`; `brew services` would bind it to
`default.target`, which starts before the compositor exports
`WAYLAND_DISPLAY`). `xdg-open`, `notify-send` and `wl-copy` come from the
system (caveats).

## Sources

The formula has three tarballs, one per repository: farol's (`url`), and
the resources `tessela` (core/Cargo.toml takes the SDK, `sdk/rust`, by path;
the same tarball `tessela.rb` builds from) and `adwaita-shell` (its `kit/`).
`install` stages tessela at `buildpath/tessela` and rewrites the path in
`core/Cargo.toml` from `../../tessela/sdk/rust` to `../tessela/sdk/rust`.
`core/Cargo.lock` does not record path dependencies' locations, so
`cargo install --locked` still holds.

## Releasing

1. Tag farol, tessela and adwaita-shell and make the tarballs:
   `git archive --prefix=farol-0.1.0/ -o farol-0.1.0.tar.gz v0.1.0` (likewise
   the other two).
2. Set `homepage`, each `url` and each `sha256` (`sha256sum *.tar.gz`) in
   `farol.rb`; all are placeholders (`https://example.invalid/...`).
3. Put `farol.rb` next to `tessela.rb` in `Formula/` of the tap.

Nothing in the formula depends on `core/Cargo.lock`'s contents: when it
changes, only the release tarball changes.

## Verified

Homebrew was not available here, so the formula was not installed. Checked:
`ruby -c farol.rb` (syntax), and its build by hand: the git archives of farol
(HEAD) and tessela laid out as `install` does (tessela at `buildpath/tessela`,
the path rewritten), then `cargo metadata --locked` and `cargo build --locked
--offline --release` in `core/` succeed. To test for real, follow the Docker
recipe in tessela's Homebrew README with both formulae in the local tap
(`farol.rb` with its three `url`s pointing at `file:///src/...` tarballs);
`brew test` then runs the CLI against a scratch database and the project
headless with its service.
