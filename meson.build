project('kdmsg-tui-ng', 'rust', version: '1.3')

# Install the pre-compiled binary
install_data(
  'target/release/kdmsg-tui-ng',  # Path to your pre-built binary
  install_dir: '/sbin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-x'
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system'
)
