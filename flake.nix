{
  description = "farol — a launcher on tessela (package, app, overlay, Home Manager and NixOS modules)";

  inputs = {
    # The runner, and the SDK core/ builds against (its source is the
    # package's src). Local checkouts: --override-input tessela git+file:///...
    tessela.url = "github:FulecoRafa/tessela";
    nixpkgs.follows = "tessela/nixpkgs";
    # The component kit (kit/, its theme) and the Phosphor icon font.
    adwaita-shell = {
      url = "github:FulecoRafa/adwaita-shell";
      inputs.tessela.follows = "tessela";
    };
  };

  outputs =
    { self, nixpkgs, tessela, adwaita-shell }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});
      farolFor =
        pkgs: system:
        pkgs.callPackage ./nix/package.nix {
          inherit (tessela.packages.${system}) tessela;
          kit = "${adwaita-shell}/kit";
        };
    in
    {
      packages = forAll (
        system: pkgs: rec {
          farol = farolFor pkgs system;
          inherit (adwaita-shell.packages.${system}) phosphor-icons-fill;
          default = farol;
        }
      );

      apps = forAll (
        system: pkgs: {
          default = {
            type = "app";
            # `farol` alone is the service; the app is `farol start`.
            program = "${pkgs.writeShellScript "farol-start" ''
              exec ${self.packages.${system}.farol}/bin/farol start
            ''}";
            meta.description = "Run farol (farol start)";
          };
        }
      );

      overlays.default = final: _prev: {
        farol = farolFor final final.stdenv.hostPlatform.system;
      };

      homeManagerModules.default = import ./nix/hm-module.nix self;
      nixosModules.default = import ./nix/nixos-module.nix self;

      # The package (whose build compiles and tests the core) and a
      # type-check of the TSX against adwaita-shell's kit and types. The
      # Home Manager module is evaluated against home-manager itself, outside
      # the checks (see nix/README.md).
      checks = forAll (
        system: pkgs: {
          package = self.packages.${system}.farol;
          typescript =
            pkgs.runCommand "farol-tsc" { nativeBuildInputs = [ pkgs.typescript ]; } ''
              # tsconfig.json looks for the kit and the types in ../adwaita-shell.
              mkdir work && cd work
              cp -r ${self} farol
              cp -r ${adwaita-shell} adwaita-shell
              chmod -R u+w .
              cd farol && tsc -p . && touch $out
            '';
        }
      );
    };
}
