{
  description = "Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem = { config, pkgs, ... }: {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
            rust-analyzer
            pkg-config
            gtk4
            gtk4-layer-shell
            libpulseaudio
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.gtk4
            pkgs.gtk4-layer-shell
            pkgs.libpulseaudio
          ];

          shellHook = "";
        };
      };
    };
}
