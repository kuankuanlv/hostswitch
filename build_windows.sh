#!/usr/bin/env bash
#
# hostswitch Windows 构建打包脚本（macOS 上交叉编译）
#
# 用法:
#   ./build_windows.sh              # 交叉编译出单 exe（cargo-xwin + MSVC）
#   ./build_windows.sh sign         # 编译后用 osslsigncode 签名（需要 cert/hostswitch.pfx）
#   ./build_windows.sh clean        # 清理旧的打包产物
#
# 依赖: cargo、rustup、Homebrew llvm（提供 clang-cl）、cargo-xwin
#   一次性安装:
#     brew install llvm
#     rustup target add x86_64-pc-windows-msvc
#     cargo install --locked cargo-xwin
#   首次运行会下载 Windows SDK（约 1-2GB，自动接受微软许可）
#
# 产物: target/x86_64-pc-windows-msvc/release/hostswitch.exe
#       dist/hostswitch.exe（sign 模式额外产出 dist/hostswitch-signed.exe）
#
# 说明: 未签名 exe 提权时 UAC 显示"未知发布者"；自签名只能显示证书 CN 名 + 黄色警告，
# 公开分发需购买 Authenticode 证书。自签证书在 Windows 上生成后导出 pfx:
#   $cert = New-SelfSignedCertificate -Type CodeSigning -Subject "CN=hostswitch Dev" `
#     -KeyExportPolicy Exportable -KeyAlgorithm RSA -KeyLength 2048 `
#     -CertStoreLocation Cert:\CurrentUser\My
#   Export-PfxCertificate -Cert "Cert:\CurrentUser\My\$($cert.Thumbprint)" `
#     -FilePath cert/hostswitch.pfx -Password $pwd
set -euo pipefail

APP_NAME="hostswitch"
TARGET="x86_64-pc-windows-msvc"
EXE="target/${TARGET}/release/${APP_NAME}.exe"
DIST="dist"
PFX="cert/hostswitch.pfx"   # 可选：sign 模式要求存在

# 切到仓库根目录（脚本可被任意 cwd 调用）
cd "$(dirname "$0")"

# Homebrew llvm 是 keg-only 安装，clang-cl 不在默认 PATH，需手动加入
if [[ -d /opt/homebrew/opt/llvm/bin ]]; then
    export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
elif [[ -d /usr/local/opt/llvm/bin ]]; then
    export PATH="/usr/local/opt/llvm/bin:$PATH"
fi

clean() {
    rm -rf "$DIST"
    rm -f "$EXE"
}

if [[ "${1:-}" == "clean" ]]; then
    clean
    echo "cleaned packaging artifacts"
    exit 0
fi

# 前置依赖检查（幂等）
command -v clang-cl >/dev/null || {
    echo "error: clang-cl not found, run: brew install llvm" >&2
    exit 1
}
command -v cargo-xwin >/dev/null || {
    echo "error: cargo-xwin not found, run: cargo install --locked cargo-xwin" >&2
    exit 1
}
if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "==> adding target $TARGET"
    rustup target add "$TARGET"
fi

echo "==> cargo xwin build --release --target $TARGET"
# XWIN_ARCH=x86_64：只下载 x86_64 SDK，避免多余的 aarch64 库（省一半下载量）
XWIN_ARCH=x86_64 cargo xwin build --release --target "$TARGET"

echo "==> assembling dist/"
mkdir -p "$DIST"
cp "$EXE" "$DIST/${APP_NAME}.exe"

# 校验产物是 64 位 Windows GUI 程序
file "$DIST/${APP_NAME}.exe"
echo "==> done: $DIST/${APP_NAME}.exe"

# 签名（可选）：PFX_PASS 从环境变量传入密码，避免脚本内硬编码
if [[ "${1:-}" == "sign" ]]; then
    if [[ ! -f "$PFX" ]]; then
        echo "error: $PFX not found, generate it on Windows first (see header)" >&2
        exit 1
    fi
    if [[ -z "${PFX_PASS:-}" ]]; then
        echo "error: set PFX_PASS to the pfx password" >&2
        exit 1
    fi
    command -v osslsigncode >/dev/null || {
        echo "error: osslsigncode not found, run: brew install osslsigncode" >&2
        exit 1
    }
    osslsigncode sign -pkcs12 "$PFX" -pass "$PFX_PASS" \
        -n "$APP_NAME" \
        -in "$DIST/${APP_NAME}.exe" -out "$DIST/${APP_NAME}-signed.exe" \
        -t http://timestamp.digicert.com
    osslsigncode verify "$DIST/${APP_NAME}-signed.exe"
    echo "==> done: $DIST/${APP_NAME}-signed.exe"
fi
