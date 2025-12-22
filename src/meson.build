pkgdatadir = get_option('prefix') / get_option('datadir')
bindir = get_option('prefix') / get_option('bindir')
sass = find_program('sass', required: true)

# Generate Wayland idle-inhibit protocol bindings
wl_protocols_dep = dependency('wayland-protocols')
wl_protocols_dir = wl_protocols_dep.get_variable('pkgdatadir')
wl_scanner_prog = find_program('wayland-scanner', native: true, required: true)

wl_inhibit_xml = wl_protocols_dir / 'unstable/idle-inhibit/idle-inhibit-unstable-v1.xml'

wl_inhibit_inc = custom_target(
  'idle-inhibit-client-protocol.h',
  output: 'idle-inhibit-client-protocol.h',
  input: wl_inhibit_xml,
  command: [wl_scanner_prog, 'client-header', '@INPUT@', '@OUTPUT@'],
)

wl_inhibit_src = custom_target(
  'idle-inhibit-client-protocol.c',
  output: 'idle-inhibit-client-protocol.c',
  input: wl_inhibit_xml,
  command: [wl_scanner_prog, 'private-code', '@INPUT@', '@OUTPUT@'],
)

# Add Vala arguments for idle-inhibit protocol
add_project_arguments(
  '--vapidir=' + meson.current_source_dir() / 'utils' / 'Zwp',
  '--pkg=idle-inhibit',
  language: 'vala'
)

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
  'AppState.vala',
)

subdir('utils')
subdir('widgets')
subdir('bar')
subdir('quick_settings')

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
  sources: [vala_sources, resource, wl_inhibit_src, wl_inhibit_inc],
  c_args: [
    '-I' + meson.current_build_dir(),
  ],
  link_args: ['-lm'], # Link math library
  install: true,
  install_dir: bindir,
)