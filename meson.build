project('kdmsg-tui-ng', 'rust',
  version: '1.3',
  default_options: ['buildtype=release']
)

# Rust-specific configuration
rust = import('rust')
executable(
  'kdmsg-tui-ng',
  sources: ['src/main.rs'],
  dependencies: [
    # Add your Rust dependencies here
    # rust.find_crate('some_crate', version: '>=1.0.0'),
  ],
  install: true
)

# Install to /sbin for system administrators
install_data(
  get_option('bindir') / 'kdmsg-tui-ng',
  install_dir: '/sbin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-x'
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system'
)

# Optional: Install man page if you have one
# install_man('man/kdmsg-tui-ng.1')
