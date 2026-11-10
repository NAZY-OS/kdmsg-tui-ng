project('kdmsg-tui-ng', 'rust',
  version: '1.3',
  default_options: ['buildtype=release']
)

# This meson.build only handles installation, not building
# (since your build.sh handles all Cargo operations)

# Install the binary (which will be built by build.sh)
install_data(
  'target/release/kdmsg-tui-ng',  # Path to your pre-built binary
  install_dir: '/usr/local/bin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-x'
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/etc/systemd/system',
  install_mode: 'rw-r--r--'
)

