# Project definition
project(
  'kdmsg-tui-ng',
  'rust',
  version: '1.3',
  default_options: [
    'buildtype=release',  # Default to release builds
    'warning_level=2'     # Enable higher warning level
  ]
)

# Rust dependencies (example for serde_json)
# rust_dep = rust.find_crate('serde_json', version: '>=1.0.0')

# Main executable (built by Meson)
executable(
  'kdmsg-tui-ng',
  sources: ['src/main.rs'],  # Main source file
  # dependencies: [rust_dep],  # Uncomment if you have dependencies
  install: true              # Enable installation
)

# Install the compiled binary to /sbin
install_data(
  get_option('bindir') / 'kdmsg-tui-ng',  # Path to built binary
  install_dir: '/sbin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-x'  # Permissions: rwx for owner, rx for others
)

# Install the systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system',
  install_mode: 'rw-r--r--'  # Permissions for service file
)

# Optional: Manpage installation (if available)
# install_man('man/kdmsg-tui-ng.1')
