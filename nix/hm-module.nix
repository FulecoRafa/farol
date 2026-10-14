# Home Manager: services.farol.enable installs farol; `autostart` starts it
# with the graphical session (a systemd user service running `farol start`).
# `hotkeys` and `settings` are written to farol's database with `farol
# hotkey` and `farol set` on activation and before each start.
self:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.farol;
  packages = self.packages.${pkgs.stdenv.hostPlatform.system};
  farol = lib.getExe cfg.package;

  # Values go to `farol set KEY JSON`; strings as JSON strings too.
  apply = pkgs.writeShellScript "farol-apply-config" (
    lib.concatStringsSep "\n" (
      [ "set -e" ]
      ++ lib.mapAttrsToList (
        keys: text: "${farol} hotkey ${lib.escapeShellArg keys} ${lib.escapeShellArg text}"
      ) cfg.hotkeys
      ++ lib.mapAttrsToList (
        key: value: "${farol} set ${lib.escapeShellArg key} ${lib.escapeShellArg (builtins.toJSON value)}"
      ) cfg.settings
    )
  );
  configured = cfg.hotkeys != { } || cfg.settings != { };
in
{
  options.services.farol = {
    enable = lib.mkEnableOption "farol, a launcher built with tessela";

    package = lib.mkOption {
      type = lib.types.package;
      default = packages.farol;
      defaultText = lib.literalExpression "farol.packages.\${system}.farol";
      description = "The farol package (it brings its own tessela and kit).";
    };

    autostart = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Start farol with the graphical session: a systemd user service,
        bound to {option}`services.farol.target`, running `farol start`.
        Without it, run `farol start` yourself (e.g. `exec-once` in
        hyprland.conf).
      '';
    };

    target = lib.mkOption {
      type = lib.types.str;
      default = "graphical-session.target";
      description = "The systemd user target that starts and stops farol.";
    };

    hotkeys = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = {
        "super+alt+space" = "";
        "super+v" = ">";
        "super+period" = ":";
      };
      description = ''
        Shortcuts farol binds in Hyprland when it starts: keys → the text the
        launcher opens with (`""` opens it empty, `"@<id>"` runs a row).
        Applied with `farol hotkey KEYS TEXT`. Only adds or replaces: a key
        removed from here stays until `farol hotkey -d KEYS`.
      '';
    };

    settings = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = { };
      example = {
        terminal = [ "foot" ];
        "web.search" = "https://duckduckgo.com/?q={query}";
        "files.hidden" = false;
      };
      description = ''
        farol's settings (`farol options` lists them), each applied with
        `farol set KEY JSON`. Only adds or replaces, like
        {option}`services.farol.hotkeys`.
      '';
    };

    fonts = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Install the fonts the kit's theme asks for: CaskaydiaCove Nerd Font
        (text) and Phosphor-Fill (icons). Without them text falls back to
        another font and icons do not show.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [
      cfg.package
    ]
    ++ lib.optionals cfg.fonts [
      pkgs.nerd-fonts.caskaydia-cove
      packages.phosphor-icons-fill
    ];

    fonts.fontconfig.enable = lib.mkIf cfg.fonts (lib.mkDefault true);

    # The database lives in $XDG_DATA_HOME/farol; `farol hotkey` and `farol
    # set` work without a running farol.
    home.activation.farol = lib.mkIf configured (
      lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        run ${apply}
      ''
    );

    systemd.user.services.farol = lib.mkIf cfg.autostart {
      Unit = {
        Description = "farol launcher (tessela)";
        PartOf = [ cfg.target ];
        After = [ cfg.target ];
      };
      Service = {
        ExecStartPre = lib.mkIf configured "${apply}";
        ExecStart = "${farol} start";
        Restart = "on-failure";
        RestartSec = 2;
      };
      Install.WantedBy = [ cfg.target ];
    };
  };
}
