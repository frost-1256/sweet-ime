{
  description = "Sweet IME: Japanese input method (fcitx5 addon on Linux)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        sweet-ime = pkgs.callPackage ./nix/package.nix { };
        default = sweet-ime;
      });

      overlays.default = final: _prev: {
        sweet-ime = final.callPackage ./nix/package.nix { };
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.sweet-ime ];
          packages = [
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
            pkgs.dbus
            (pkgs.python3.withPackages (p: [
              p.dbus-python
              p.pygobject3
            ]))
          ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (import ./nix/gui-libs.nix pkgs);
        };
      });
    };
}
