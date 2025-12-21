pkgdatadir = get_option('prefix') / get_option('datadir')
bindir = get_option('prefix') / get_option('bindir')
sass = find_program('sass', required: true)

dependencies = [
  dependency('gtk4-layer-shell-0'),
  dependency('astal-io-0.1'),
  dependency('gio-2.0'),
  dependency('glib-2.0'),
  dependency('astal-4-4.0'),
  dependency('astal-battery-0.1'),
  dependency('astal-wireplumber-0.1'),
  dependency('astal-network-0.1'),
  dependency('libnm'),
  dependency('astal-mpris-0.1'),
  dependency('astal-power-profiles-0.1'),
  dependency('astal-tray-0.1'),
  dependency('astal-bluetooth-0.1'),
  dependency('astal-hyprland-0.1'),
  dependency('wayland-client'),
]

vala_sources = files(
  'App.vala',
)

subdir('utils')
subdir('widgets')
subdir('bar')

# bundle scss files
css = custom_target(
  'scss',
  input: files('style.scss'),
  command: [sass, '@INPUT@', '@OUTPUT@'],
  output: ['style.css'],
)

# compiling data files into a binary
resource = import('gnome').compile_resources(
  'data',
  files('gresource.xml'),
  dependencies: [
    css,
  ],
  source_dir: meson.current_build_dir(),
)

executable(
  meson.project_name(),
  dependencies: dependencies,
  sources: [vala_sources, resource],
  link_args: ['-lm'], # Link math library
  install: true,
  install_dir: bindir,
)