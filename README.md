# kdmsg-tui-ng
**A tui in rust for kdmsg infos**


## Quick Start

1. Clone the repository:
   ```sh
   git clone https://github.com/NAZY-OS/kdmsg-tui-ng.git
   cd kdmsg-tui-ng
   ```

2. Change Permission

   ```sh
   chmod +x build.sh
   ```

3. Run Script and install
   ```sh
   ./build.sh
   rm -rf build
   meson setup build
   sudo meson install -C build
   ```

4. Install Systeme Service
   ```sh
   sudo systemctl daemon-reload
   sudo systemctl enable --now kdmsg-tui.service
   ```
 
---
 \
 \
![kdmsg-tui-ng](https://github.com/NAZY-OS/kdmsg-tui-ng/blob/main/screenshot/Screenshot_RC.png)

## F1 help infos
![kdmsg-tui-ng-help](https://github.com/NAZY-OS/kdmsg-tui-ng/blob/main/screenshot/Screenshot_RC-II.png)
