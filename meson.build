project('kdmsg-tui-ng', version: '1.0.0')

# Install the pre-compiled Cargo binary directly to /sbin
install_data(
  'target/release/kdmsg-tui-ng',
  install_dir: '/sbin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-x'
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system'
)
