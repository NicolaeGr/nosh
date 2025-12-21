{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    astal = {
      url = "github:aylur/astal";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      astal,
    }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      nativeBuildInputs = with pkgs; [
        meson
        ninja
        pkg-config
        gobject-introspection
        wrapGAppsHook4
        blueprint-compiler
        dart-sass
        vala
        glib
        gtk4
      ];

      astalPackages = with astal.packages.${system}; [
        astal4
        battery
        wireplumber
        network
        mpris
        powerprofiles
        tray
        bluetooth
        hyprland
      ];
    in
    {
      packages.${system}.default = pkgs.stdenv.mkDerivation {
        name = "hypr-shell";
        src = ./.;
        inherit nativeBuildInputs;
        buildInputs = astalPackages;
      };

      devShells.${system}.default = pkgs.mkShell {
        packages =
          nativeBuildInputs
          ++ astalPackages
          ++ [
            pkgs.vala-language-server
            pkgs.openjdk
            pkgs.uncrustify
          ];
      };
    };
}
