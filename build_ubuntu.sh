#!/usr/bin/env bash
#
# hostswitch Ubuntu 构建打包脚本
#
# 用法:
#   ./build_ubuntu.sh            # 构建 release 并生成 .deb（默认）
#   ./build_ubuntu.sh install    # 构建 .deb 并用 apt 安装（需要 sudo）
#   ./build_ubuntu.sh uninstall  # 卸载已安装的 hostswitch
#   ./build_ubuntu.sh clean      # 清理打包产物
#
# 依赖: cargo、dpkg-deb（Ubuntu 自带）
# 产物: dist/hostswitch_<version>_<arch>.deb
set -euo pipefail
# 统一产物权限，避免受调用方 umask 影响（deb 里目录 755、文件 644）
umask 022

APP_NAME="hostswitch"
VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/^version *= *"\([^"]*\)".*/\1/')"
ICON="assets/icon.png"
DIST="dist"
ARCH="$(dpkg --print-architecture)"
DEB="${DIST}/${APP_NAME}_${VERSION}_${ARCH}.deb"

cd "$(dirname "$0")"
ROOT="$(pwd -P)"

usage() {
    cat <<'EOF'
usage: ./build_ubuntu.sh [deb|install|uninstall|clean]

  deb        构建 release 并生成 .deb（默认）
  install    构建 .deb 并用 apt 安装（需要 sudo）
  uninstall  卸载已安装的 hostswitch
  clean      清理打包产物
EOF
}

clean() {
    rm -rf "${DIST}/deb" "${DIST}/linux" "${DIST}/ubuntu" "$DEB"
}

build() {
    echo "==> cargo build --release"
    cargo build --release
}

write_desktop() {
    local exec_path="$1"
    local icon="$2"
    local path="$3"

    cat > "$path" <<EOF
[Desktop Entry]
Type=Application
Name=${APP_NAME}
Comment=Manage and switch hosts file entries
Exec=${exec_path}
Icon=${icon}
Terminal=false
Categories=Utility;System;
StartupWMClass=${APP_NAME}
EOF
}

build_deb() {
    command -v dpkg-deb >/dev/null 2>&1 || {
        echo "dpkg-deb not found; install dpkg-dev first" >&2
        exit 1
    }
    [ -f "$ICON" ] || {
        echo "icon not found: $ICON" >&2
        exit 1
    }

    local root="${DIST}/deb"
    local size
    echo "==> assembling .deb"
    rm -rf "$root"
    mkdir -p \
        "$root/DEBIAN" \
        "$root/usr/bin" \
        "$root/usr/share/applications" \
        "$root/usr/share/icons/hicolor/256x256/apps"

    install -m 755 "target/release/${APP_NAME}" "$root/usr/bin/${APP_NAME}"
    install -m 644 "$ICON" "$root/usr/share/icons/hicolor/256x256/apps/${APP_NAME}.png"
    write_desktop \
        "/usr/bin/${APP_NAME}" \
        "${APP_NAME}" \
        "$root/usr/share/applications/${APP_NAME}.desktop"

    # 安装/卸载后刷新桌面数据库和图标缓存，失败不影响 dpkg 结果
    cat > "$root/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
update-desktop-database -q /usr/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || true
exit 0
EOF
    cat > "$root/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
update-desktop-database -q /usr/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t /usr/share/icons/hicolor 2>/dev/null || true
exit 0
EOF
    chmod 755 "$root/DEBIAN/postinst" "$root/DEBIAN/postrm"

    size="$(du -sk "$root/usr" | cut -f1)"
    cat > "$root/DEBIAN/control" <<EOF
Package: ${APP_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: zhangzhichao <zhangzhichao@localhost>
Depends: libc6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libx11-6, libxcursor1, libxi6, libxrandr2, libxinerama1, libegl1, libgl1, libvulkan1, pkexec | policykit-1
Recommends: mesa-vulkan-drivers
Installed-Size: ${size}
Description: hosts file manager and switcher
 Manage hosts file environments and apply them to the system hosts file.
EOF

    dpkg-deb --root-owner-group --build "$root" "$DEB"
    rm -rf "$root"
}

print_result() {
    echo
    echo "Build succeeded."
    echo "  ${ROOT}/${DEB}"
    echo
    echo "安装:   sudo apt install ${ROOT}/${DEB}"
    echo "卸载:   sudo apt remove ${APP_NAME}"
    echo "配置文件 ~/.hostswitch 在卸载后保留。"
}

case "${1:-deb}" in
    deb)
        clean
        build
        build_deb
        print_result
        ;;
    install)
        clean
        build
        build_deb
        sudo apt-get install -y "${ROOT}/${DEB}"
        ;;
    uninstall)
        sudo apt-get remove -y "${APP_NAME}"
        ;;
    clean)
        clean
        echo "cleaned packaging artifacts under: ${ROOT}/${DIST}"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        echo "unknown argument: $1" >&2
        usage
        exit 1
        ;;
esac
