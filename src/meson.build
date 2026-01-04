pkgdatadir = get_option('prefix') / get_option('datadir')
bindir = get_option('prefix') / get_option('bindir')
sass = find_program('sass', required: true)
blp = find_program('blueprint-compiler', required: true)
gnome = import('gnome')

# Get the stable option
stable_build = get_option('stable')

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
  language: 'vala',
)

dependencies = [
  dependency('gtk4-layer-shell-0'),
  dependency('astal-io-0.1'),
  dependency('gio-2.0'),
  dependency('gio-unix-2.0'),
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
  dependency('astal-notifd-0.1'),
  dependency('wayland-client'),
  dependency('sqlite3'),
  dependency('gee-0.8'),
]

vala_sources = files(
  'App.vala',
  'AppState.vala',
)

blueprint_sources = []

subdir('utils')
subdir('bar')
subdir('notifications')
subdir('quick_settings')
subdir('indicators')
subdir('app_launcher')

ui_target = custom_target(
  'ui-files',
  input: blueprint_sources,
  output: '.',
  command: [
    blp,
    'batch-compile',
    '@OUTPUT@',
    '@CURRENT_SOURCE_DIR@',
    '@INPUT@',
  ],
  build_by_default: true,
)

all_entries = []
current_src_dir = meson.current_source_dir() + '/'

foreach src : blueprint_sources
  full_path = src.full_path()
  if not full_path.startswith(current_src_dir)
    error('Blueprint file @0@ is not under current source dir!'.format(full_path))
  endif
  rel_path = full_path.replace(current_src_dir, '')
  ui_path = rel_path.replace('.blp', '.ui')
  all_entries += '<file preprocess="xml-stripblanks">' + ui_path + '</file>'
endforeach

css = custom_target(
  'scss',
  input: files('style.scss'),
  command: [sass, '@INPUT@', '@OUTPUT@'],
  output: ['style.css'],
)

gresource_content = run_command(
  'cat', files('gresource.xml'),
  capture: true,
  check: true,
).stdout()

final_gresource_content = gresource_content.replace(
  '<!-- UI_FILES_PLACEHOLDER -->',
  '\n'.join(all_entries)
)

generated_gresource = custom_target(
  'generated-gresource',
  output: 'gresource.xml',
  command: ['printf', '%s', final_gresource_content],
  capture: true,
  build_by_default: true,
)

resource = import('gnome').compile_resources(
  'data',
  generated_gresource,
  dependencies: [
    ui_target,
    css,
  ],
  source_dir: meson.current_build_dir(),
)

executable(
  meson.project_name(),
  dependencies: dependencies,
  sources: [vala_sources, resource, wl_inhibit_src, wl_inhibit_inc] ,
  c_args: [
    '-I' + meson.current_build_dir(),
    '-fno-semantic-interposition', 
  ],
  vala_args: stable_build ? ['-D', 'STABLE_BUILD'] : [],
  link_args: [
    '-lm', 
    '-Wl,-O2',
  ],
  install: true,
  install_dir: bindir,
)