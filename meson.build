project('kdmsg-tui-ng', version: '1.3')

# Install to /sbin for system administrators
install_data(
  'target/release/kdmsg-tui-ng',
  install_dir: '/sbin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-xr'
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system'
)
