{
  self,
  ...
}:

let
  defaultPackage = pkgs: self.packages.${pkgs.stdenv.hostPlatform.system}.default;

  mkUnit = cfg: {
    description = "nosh - Hyprland shell";
    after = cfg.startAfter;
    partOf = [ "graphical-session.target" ];
    wantedBy = cfg.wantedBy;

    unitConfig.ConditionEnvironment = "HYPRLAND_INSTANCE_SIGNATURE";

    serviceConfig = {
      Type = "simple";
      ExecStart = "${cfg.package}/bin/nosh";
      Restart = "on-failure";
      RestartSec = 5;
    };
  };
in
{
  flake =
    let
      nixos =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.nosh;
          inherit (lib)
            mkDefault
            mkEnableOption
            mkIf
            mkOption
            types
            ;
        in
        {
          options.services.nosh = {
            enable = mkEnableOption "nosh, a Hyprland shell";

            package = mkOption {
              type = types.package;
              default = defaultPackage pkgs;
              description = "The nosh package to use.";
            };

            systemd.enable = mkOption {
              type = types.bool;
              default = true;
              description = ''
                Install a systemd user unit for every user. The unit starts only
                when the user is in a Hyprland session.
              '';
            };

            startAfter = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets the shell starts after.";
            };

            wantedBy = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets that want the shell.";
            };
          };

          config = mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];

            services.upower.enable = mkDefault true;

            # DDC/CI backlights need i2c device nodes. brightnessctl and ddcutil
            # both ship udev rules that grant the active session access.
            hardware.i2c.enable = mkDefault true;
            services.udev.packages = [
              pkgs.brightnessctl
              pkgs.ddcutil
            ];

            systemd.user.services.nosh = mkIf cfg.systemd.enable (mkUnit cfg);
          };
        };

      homeManager =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.programs.nosh;
          inherit (lib)
            mkEnableOption
            mkIf
            mkOption
            types
            ;
        in
        {
          options.programs.nosh = {
            enable = mkEnableOption "nosh, a Hyprland shell";

            package = mkOption {
              type = types.package;
              default = defaultPackage pkgs;
              description = "The nosh package to use.";
            };

            startAfter = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets the shell starts after.";
            };

            wantedBy = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets that want the shell.";
            };
          };

          config = mkIf cfg.enable {
            home.packages = [ cfg.package ];
            systemd.user.services.nosh = mkUnit cfg;
          };
        };

      hjem =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.programs.nosh;
          inherit (lib)
            mkEnableOption
            mkIf
            mkOption
            types
            ;
        in
        {
          options.programs.nosh = {
            enable = mkEnableOption "nosh, a Hyprland shell";

            package = mkOption {
              type = types.package;
              default = defaultPackage pkgs;
              description = "The nosh package to use.";
            };

            startAfter = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets the shell starts after.";
            };

            wantedBy = mkOption {
              type = types.listOf types.str;
              default = [ "graphical-session.target" ];
              description = "Systemd targets that want the shell.";
            };
          };

          config = mkIf cfg.enable {
            packages = [ cfg.package ];
            systemd.services.nosh = mkUnit cfg;
          };
        };
    in
    {
      nixosModules = {
        default = nixos;
        nosh = nixos;
      };

      homeManagerModules = {
        default = homeManager;
        nosh = homeManager;
      };

      hjemModules = {
        default = hjem;
        nosh = hjem;
      };
    };
}
