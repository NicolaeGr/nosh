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
        sqlite
        libgee
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
            pkgs.brightnessctl
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

            hjemModule = {
              enable = mkEnableOption "Enable NOSH systemd service for hjem";

              startAfter = mkOption {
                type = types.listOf types.str;
                default = [ "graphical-session.target" ];
              };

              wantedBy = mkOption {
                type = types.listOf types.str;
                default = [ "graphical-session.target" ];
              };

              package = mkOption {
                type = types.package;
                default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
                description = "The nosh package to use";
              };
            };
          };

          config = mkIf cfg.enable {
            services.upower.enable = true;

            environment.systemPackages = [
              pkgs.brightnessctl
            ];

            hjem.extraModules = mkIf cfg.hjemModule.enable [
              {
                systemd.services."nosh" = {
                  description = "NOSH - Hyprland Shell";
                  after = cfg.hjemModule.startAfter;
                  wantedBy = cfg.hjemModule.wantedBy;
                  partOf = [ "graphical-session.target" ];

                  unitConfig = {
                    ConditionEnvironment = "HYPRLAND_INSTANCE_SIGNATURE";
                  };

                  serviceConfig = {
                    Type = "simple";
                    ExecStart = "${cfg.hjemModule.package}/bin/nosh";
                    Restart = "on-failure";
                    RestartSec = 5;

                    Environment = [
                      "QT_QPA_PLATFORM=wayland"
                      "WAYLAND_DISPLAY=wayland-1"
                    ];
                  };
                };
              }
            ];
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
          nosh = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          options.programs.nosh = {
            enable = mkEnableOption "NOSH - Hyprland shell";

            startAfter = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets to start after";
            };

            wantedBy = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets to be wanted by";
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
                PartOf = [ "graphical-session.target" ];
                ConditionEnvironment = "HYPRLAND_INSTANCE_SIGNATURE";
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
                WantedBy = cfg.wantedBy;
              };
            };
          };
        };
    };
}
