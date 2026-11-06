{
  description = "nosh - a Wayland desktop shell written in Rust";

  nixConfig = {
    extra-substituters = [ "https://nicolaegr.cachix.org" ];
    extra-trusted-public-keys = [
      "nicolaegr.cachix.org-1:wHSo9o0JcmUWG7+/tfFjFErV+3ZcToIYmtcDyTv1SDc="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        ./packaging/nix/packages.nix
        ./packaging/nix/modules.nix
      ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
    };
}
