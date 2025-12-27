{
  description = "NOSH - A Hyprland shell written in Vala";

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
        wayland
        wayland-protocols
        wayland-scanner
      ];

      astalPackages = with astal.packages.${system}; [
        astal4
        battery
        wireplumber
        network
        mpris
        notifd
        powerprofiles
        tray
        bluetooth
        hyprland
      ];

      buildNosh =
        { pkgs }:
        pkgs.stdenv.mkDerivation {
          name = "nosh";
          src = ./.;
          inherit nativeBuildInputs;
          buildInputs = astalPackages;
          mesonFlags = [ "-Dstable=true" ];
        };

      noshPackage = buildNosh { inherit pkgs; };
    in
    {
      packages.${system}.default = noshPackage;

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

      nixosModules.nosh =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        with lib;
        let
          cfg = config.services.nosh;
        in
        {
          options.services.nosh = {
            enable = mkEnableOption "NOSH - Hyprland shell";
          };

          config = mkIf cfg.enable {
            services.upower.enable = true;

            environment.systemPackages = [ pkgs.light ];
          };
        };

      homeManagerModules.nosh =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        with lib;
        let
          cfg = config.programs.nosh;
          nosh = self.packages.${pkgs.system}.default;
        in
        {
          options.programs.nosh = {
            enable = mkEnableOption "NOSH - Hyprland shell";

            startAfter = mkOption {
              type = types.listOf types.str;
              default = [ "hyprland-session.target" ];
              description = "Systemd targets to start after";
            };

            package = mkOption {
              type = types.package;
              default = nosh;
              description = "The nosh package to use";
            };
          };

          config = mkIf cfg.enable {
            systemd.user.services.nosh = {
              Unit = {
                Description = "NOSH - Hyprland Shell";
                After = cfg.startAfter;
              };

              Service = {
                Type = "simple";
                ExecStart = "${cfg.package}/bin/nosh";
                Restart = "on-failure";
                RestartSec = 5;

                Environment = [
                  "QT_QPA_PLATFORM=wayland"
                  "WAYLAND_DISPLAY=wayland-1"
                ];
              };

              Install = {
                WantedBy = [ "graphical-session.target" ];
              };
            };
          };
        };
    };
}
