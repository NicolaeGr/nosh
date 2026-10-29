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

    # Keep the session PATH that UWSM imported. The unit generator otherwise
    # writes Environment="PATH=coreutils:...".
    enableDefaultPath = false;

    unitConfig.ConditionEnvironment = "HYPRLAND_INSTANCE_SIGNATURE";

    serviceConfig = {
      Type = "simple";
      ExecStart = "${cfg.package}/bin/nosh";
      Restart = "on-failure";
      RestartSec = 5;
    };
  };

  mkHomeManagerUnit = cfg: {
    Unit = {
      Description = "nosh - Hyprland shell";
      After = cfg.startAfter;
      PartOf = [ "graphical-session.target" ];
      ConditionEnvironment = "HYPRLAND_INSTANCE_SIGNATURE";
    };
    Service = {
      Type = "simple";
      ExecStart = "${cfg.package}/bin/nosh";
      Restart = "on-failure";
      RestartSec = 5;
    };
    Install.WantedBy = cfg.wantedBy;
  };
in
{
  flake =
    let
      nixos =
        {
          config,
          lib,
          options,
          pkgs,
          ...
        }:
        let
          cfg = config.services.nosh;
          inherit (lib)
            mkDefault
            mkEnableOption
            mkIf
            mkMerge
            mkOption
            optionalAttrs
            types
            ;

          enabled = cfg.withHM || cfg.withHJEM;
        in
        {
          options.services.nosh = {
            withHM = mkEnableOption "the nosh Home Manager module";
            withHJEM = mkEnableOption "the nosh hjem module";

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

          config = mkMerge [
            (mkIf enabled {
              services.upower.enable = mkDefault true;

              security.pam.services.nosh-auth = { };

              hardware.i2c.enable = mkDefault true;
              services.udev.packages = [
                pkgs.brightnessctl
                pkgs.ddcutil
              ];

              assertions = [
                {
                  assertion = !(cfg.withHM && cfg.withHJEM);
                  message = "services.nosh: enable either withHM or withHJEM, not both";
                }
                {
                  assertion = !cfg.withHM || options ? home-manager;
                  message = "services.nosh.withHM needs the Home Manager NixOS module";
                }
                {
                  assertion = !cfg.withHJEM || options ? hjem;
                  message = "services.nosh.withHJEM needs the hjem NixOS module";
                }
              ];
            })
            (optionalAttrs (options ? home-manager) {
              home-manager.sharedModules = mkIf cfg.withHM [
                {
                  home.packages = [ cfg.package ];
                  systemd.user.services.nosh = mkHomeManagerUnit cfg;
                }
              ];
            })
            (optionalAttrs (options ? hjem) {
              hjem.extraModules = mkIf cfg.withHJEM [
                {
                  packages = [ cfg.package ];
                  systemd.services.nosh = mkUnit cfg;
                }
              ];
            })
          ];
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
            systemd.user.services.nosh = mkHomeManagerUnit cfg;
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
