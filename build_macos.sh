#!/usr/bin/env bash
#
# hostswitch macOS 构建打包脚本
#
# 用法:
#   ./build_macos.sh           # 构建 .app（ad-hoc 签名，本机使用）
#   ./build_macos.sh dmg       # 构建 .app 并打包成 dmg
#   ./build_macos.sh clean     # 清理旧的打包产物
#
# 依赖: cargo、codesign（Xcode Command Line Tools）、hdiutil（系统自带）
# 统一产物目录（与 build_windows.bat 一致）: dist/
#   - dist/hostswitch.app
#   - dist/hostswitch.dmg（仅 dmg 模式）
#
# 说明: ad-hoc 签名（-s -）仅用于本机/内部分发。
# 公开分发需要 Developer ID 证书 + notarytool 公证。
set -euo pipefail

APP_NAME="hostswitch"
BUNDLE_ID="com.hostswitch.app"
VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/^version *= *"\([^"]*\)".*/\1/')"
ICON="assets/icon.icns"          # 可选：存在则打进 app，否则用默认图标
OS_MIN_VERSION="11.0"            # 最低 macOS 版本（与 rustc 默认部署目标一致）
DIST="dist"
APP_DIR="${DIST}/${APP_NAME}.app"
DMG_PATH="${DIST}/${APP_NAME}.dmg"

# 切到仓库根目录（脚本可被任意 cwd 调用）
cd "$(dirname "$0")"
ROOT="$(pwd -P)"

wait_to_close() {
    echo
    echo "Press Enter to close..."
    # 非交互环境（管道/CI）不阻塞
    if [[ -t 0 ]]; then
        read -r _
    fi
}

print_success() {
    echo
    echo "========================================"
    echo " Build succeeded."
    echo " Output directory:"
    echo "   ${ROOT}/${DIST}"
    echo " Artifacts:"
    for path in "$@"; do
        if [[ -e "$path" ]]; then
            echo "   ${ROOT}/${path}"
        fi
    done
    echo "========================================"
    echo
}

on_error() {
    local code=$?
    echo
    echo "Build failed (exit ${code}). See messages above."
    wait_to_close
    exit "$code"
}
trap on_error ERR

clean() {
    rm -rf "${DIST}/${APP_NAME}.app" \
           "${DIST}/${APP_NAME}.dmg" \
           "${DIST}/dmg-staging"
}

if [[ "${1:-}" == "clean" ]]; then
    clean
    echo
    echo "cleaned packaging artifacts under: ${ROOT}/${DIST}"
    echo
    wait_to_close
    exit 0
fi

echo "==> cargo build --release"
cargo build --release

CONTENTS="$APP_DIR/Contents"

clean
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"

echo "==> assembling .app bundle"
cp "target/release/${APP_NAME}" "$CONTENTS/MacOS/"

if [[ -f "$ICON" ]]; then
    cp "$ICON" "$CONTENTS/Resources/icon.icns"
    ICON_KEY="    <key>CFBundleIconFile</key>
    <string>icon.icns</string>"
else
    ICON_KEY=""
fi

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleExecutable</key>
    <string>${APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>${OS_MIN_VERSION}</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSUIElement</key>
    <false/>
${ICON_KEY}
</dict>
</plist>
EOF

echo "==> ad-hoc signing (local use)"
codesign --force --deep -s - "$APP_DIR"
codesign --verify --deep --strict "$APP_DIR"

ARTIFACTS=("$APP_DIR")

if [[ "${1:-}" == "dmg" ]]; then
    echo "==> building dmg"
    STAGING="${DIST}/dmg-staging"
    rm -rf "$STAGING"
    mkdir -p "$STAGING"
    # ditto 保留 .app 的代码签名和 xattr（cp 不保留）
    ditto "$APP_DIR" "$STAGING/${APP_NAME}.app"
    ln -s /Applications "$STAGING/Applications"

    hdiutil create \
        -volname "$APP_NAME" \
        -srcfolder "$STAGING" \
        -ov -format UDZO \
        "$DMG_PATH"

    rm -rf "$STAGING"
    ARTIFACTS+=("$DMG_PATH")
fi

print_success "${ARTIFACTS[@]}"
wait_to_close
