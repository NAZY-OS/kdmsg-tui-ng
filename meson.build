project('kdmsg-tui-ng', 'c',
  version: '1.3.0',
  license: 'GPLv3'
)

cargo = find_program('cargo', required: true)

buildtype = get_option('buildtype')
cargo_args = ['build']
if buildtype == 'release' or buildtype == 'minsize'
  cargo_args += ['--release']
endif

binary_name = 'kdmsg-tui-ng'

cargo_build = custom_target('cargo-build',
  output: binary_name,
  input: ['Cargo.toml', 'src/main.rs'],
  command: [cargo, cargo_args],
  build_by_default: true,
  install: true,
  install_dir: get_option('sbindir'),
)

install_data(
  'service/kdmsg-tui.service',
  install_dir: get_option('prefix') / 'lib' / 'systemd' / 'system'
)
