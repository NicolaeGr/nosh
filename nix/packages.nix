{
  perSystem =
    { self', pkgs, ... }:
    {
      packages.default = pkgs.callPackage ./package.nix { };

      devShells.default = pkgs.mkShell {
        inputsFrom = [ self'.packages.default ];

        packages = with pkgs; [
          brightnessctl
          cargo
          clippy
          rust-analyzer
          rustc
          rustfmt
        ];

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.gtk4
          pkgs.gtk4-layer-shell
          pkgs.libpulseaudio
          pkgs.wayland
        ];
      };
    };
}
