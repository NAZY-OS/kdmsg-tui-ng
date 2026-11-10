project('kdmsg-tui-ng', 'rust',
  version: '1.3',
  default_options: ['buildtype=release']
)

# Build the Rust executable (managed by Cargo)
executable(
  'kdmsg-tui-ng',
  sources: ['src/main.rs'],
  install: true
)

# Install binary to /usr/local/bin
install_data(
  get_option('bindir') / 'kdmsg-tui-ng',
  install_dir: '/usr/local/bin',
  install_mode: 'rwxr-xr-x'
)

# Install systemd service
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/etc/systemd/system',
  install_mode: 'rw-r--r--'
)
