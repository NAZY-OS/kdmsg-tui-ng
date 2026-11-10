project('kdmsg-tui-ng', 'rust',
  version: '1.3',
  default_options: ['buildtype=release']
)

# Rust executable (if you want Meson to build it)
executable(
  'kdmsg-tui-ng',
  sources: ['src/main.rs'],
  dependencies: [
    # Add your Rust dependencies here (e.g., rust.find_crate('serde', version: '>=1.0.0'))
  ],
  install: true
)

# Install pre-built binary (if you prefer to use the existing binary)
install_data(
  'target/release/kdmsg-tui-ng',  # Relative path to the pre-built binary
  install_dir: '/sbin',
  rename: 'kdmsg-tui-ng',
  install_mode: 'rwxr-xr-x'
)

# Install systemd service file
install_data(
  'service/kdmsg-tui.service',
  install_dir: '/lib/systemd/system'
)
