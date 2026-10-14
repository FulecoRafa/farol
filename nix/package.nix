# farol: a launcher on tessela. Builds the core (core/, Rust, on tessela's
# SDK), installs the project to share/farol with adwaita-shell's kit beside
# it, and bin/farol: the CLI (`farol start`, `farol toggle`, ...) and, with
# no arguments, the service the runner starts.
#
# Layout (what core/src/launch.rs looks for):
#   bin/farol                  wrapper: TESSELA (the runner) and the tools
#                              the core calls; `farol start` runs
#                              `tessela run <out>/share/farol`
#   share/farol/tessela.toml   its plugin command is bin/farol, its kit and
#                              theme the copy below
#   share/farol/{src,kit}      the surface and adwaita-shell's kit
#   lib/systemd/user/farol.service
{
  lib,
  rustPlatform,
  makeWrapper,
  tessela,
  # adwaita-shell's kit/ directory (components, theme.toml).
  kit,
  # What the core runs, after the host's own (PATH suffix).
  wl-clipboard, # wl-copy: copying when the native clipboard is not running
  xdg-utils, # xdg-open: files, folders and URLs
  libnotify, # notify-send: timers and alarms
}:

let
  tools = [
    wl-clipboard
    xdg-utils
    libnotify
  ];
in
rustPlatform.buildRustPackage {
  pname = "farol";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../tessela.toml
      ../tsconfig.json
      ../README.md
      ../LICENSE-MIT
      ../LICENSE-APACHE
      ../src
      ../core/Cargo.toml
      ../core/Cargo.lock
      ../core/src
    ];
  };

  # core/Cargo.toml takes the SDK from a sibling checkout,
  # ../../tessela/sdk/rust: put the source tessela was built from there.
  postUnpack = ''
    cp -r ${tessela.src} tessela
    chmod -R u+w tessela
  '';

  cargoRoot = "core";
  buildAndTestSubdir = "core";
  cargoLock.lockFile = ../core/Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  postInstall = ''
    share=$out/share/farol
    mkdir -p $share
    cp -r tessela.toml tsconfig.json src $share
    cp -r ${kit} $share/kit
    chmod -R u+w $share/kit

    # The service is this package's farol, the kit the copy beside it.
    substituteInPlace $share/tessela.toml \
      --replace-fail 'command = ["core/target/release/farol"]' "command = [\"$out/bin/farol\"]" \
      --replace-fail 'themes = ["../adwaita-shell/kit/theme.toml"]' "themes = [\"$share/kit/theme.toml\"]" \
      --replace-fail 'kit = "../adwaita-shell/kit"' "kit = \"$share/kit\""
    substituteInPlace $share/tsconfig.json \
      --replace-fail '"@kit/*": ["../adwaita-shell/kit/*"]' '"@kit/*": ["kit/*"]'

    install -Dm644 -t $out/share/doc/farol README.md
    install -Dm644 -t $out/share/licenses/farol LICENSE-MIT LICENSE-APACHE

    # `farol start` finds the project from the binary (../share/farol) and
    # the runner from $TESSELA. The host's tools before ours.
    wrapProgram $out/bin/farol \
      --set-default TESSELA ${lib.getExe tessela} \
      --suffix PATH : ${lib.makeBinPath tools}

    install -Dm644 ${./farol.service} $out/lib/systemd/user/farol.service
    substituteInPlace $out/lib/systemd/user/farol.service \
      --replace-fail @bindir@ $out/bin
  '';

  # The installed manifest points into the store only, and the CLI runs.
  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    grep -qF "command = [\"$out/bin/farol\"]" $out/share/farol/tessela.toml
    if grep -q '\.\./' $out/share/farol/tessela.toml; then
      echo "tessela.toml still points outside the package" >&2
      exit 1
    fi
    test -f $out/share/farol/kit/theme.toml
    export FAROL_DB=$TMPDIR/check.db
    $out/bin/farol hotkey super+alt+space '>'
    $out/bin/farol hotkeys | grep -qF 'super+alt+space'
    runHook postInstallCheck
  '';

  passthru = { inherit tessela; };

  meta = {
    description = "Launcher for Wayland (apps, calculator, files, clipboard, windows, timers), a tessela project";
    license = with lib.licenses; [ mit asl20 ];
    mainProgram = "farol";
    platforms = lib.platforms.linux;
  };
}
