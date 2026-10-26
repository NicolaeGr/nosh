{
  lib,
  rustPlatform,
  makeWrapper,
  pkg-config,
  brightnessctl,
  ddcutil,
  gtk4,
  gtk4-layer-shell,
  libpulseaudio,
  pulseaudio,
  wayland,
  wireplumber,
}:

let
  version = (lib.importTOML ../crates/nosh-ui/Cargo.toml).package.version;

  runtimeDeps = [
    brightnessctl
    ddcutil
    pulseaudio
    wireplumber
  ];
in
rustPlatform.buildRustPackage {
  pname = "nosh";
  inherit version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      lib.cleanSourceFilter path type
      && !(builtins.elem (baseNameOf path) [
        ".direnv"
        "result"
        "target"
      ]);
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    makeWrapper
    pkg-config
  ];

  buildInputs = [
    gtk4
    gtk4-layer-shell
    libpulseaudio
    wayland
  ];

  cargoBuildFlags = [
    "-p"
    "nosh-ui"
    "--no-default-features"
    "--features"
    "stable"
  ];

  cargoTestFlags = [
    "-p"
    "nosh-core"
  ];

  postFixup = ''
    wrapProgram $out/bin/nosh \
      --prefix PATH : ${lib.makeBinPath runtimeDeps}
  '';

  meta = {
    description = "A Hyprland shell";
    homepage = "https://github.com/nicolaegr/nosh";
    license = lib.licenses.mit;
    mainProgram = "nosh";
    platforms = lib.platforms.linux;
  };
}
