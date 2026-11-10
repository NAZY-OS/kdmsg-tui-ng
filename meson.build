project('kdmsg-tui-ng', 'rust',
  version: '1.3',
  default_options: ['buildtype=release']
)

# Build the Rust executable
executable(
  'kdmsg-tui-ng',
  sources: ['src/main.rs'],
  install: true
)

# Install the binary to /sbin
install_data(
  get_option('bindir') / 'kdmsg-tui-ng',
  install_dir: '/sbin',
  install_mode: 'rwxr-xr-x'
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system',
  install_mode: 'rw-r--r--'
)
