# NixOS: programs.farol.enable installs farol for every user and, with
# `autostart`, starts it with their graphical session (the packaged systemd
# user unit, `farol start`). Per-user hotkeys and settings: the Home Manager
# module, or `farol hotkey` / `farol set`.
self:
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.farol;
  packages = self.packages.${pkgs.stdenv.hostPlatform.system};
in
{
  options.programs.farol = {
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
      description = "Start farol with each user's graphical session (graphical-session.target).";
    };

    fonts = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install the theme's fonts: CaskaydiaCove Nerd Font and Phosphor-Fill.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    systemd.packages = [ cfg.package ];
    systemd.user.services.farol.wantedBy = lib.mkIf cfg.autostart [ "graphical-session.target" ];

    fonts.packages = lib.optionals cfg.fonts [
      pkgs.nerd-fonts.caskaydia-cove
      packages.phosphor-icons-fill
    ];
  };
}
