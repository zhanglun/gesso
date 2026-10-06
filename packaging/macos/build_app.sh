#!/usr/bin/env bash
# 组装 macOS Gesso.app（不签名、不公证）。
#
# 用法：
#   packaging/macos/build_app.sh            # release 构建 + 组装 .app
#   SKIP_BUILD=1 packaging/macos/build_app.sh   # 复用已有 release 二进制，只重新组装
#
# 产物：target/release-bundle/Gesso.app
set -euo pipefail

# 仓库根（脚本在 packaging/macos/ 下，上两级）
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

# 版本号取自 workspace Cargo.toml 的 `version = "x.y.z"`
VERSION="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' Cargo.toml | head -1)"
if [[ -z "$VERSION" ]]; then
    echo "无法从 Cargo.toml 解析版本号" >&2
    exit 1
fi

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    echo ">> cargo build --release -p gesso-app"
    cargo build --release -p gesso-app
fi

BIN="target/release/gesso"
if [[ ! -f "$BIN" ]]; then
    echo "找不到 ${BIN}（先去掉 SKIP_BUILD 让脚本构建）" >&2
    exit 1
fi

APP="target/release-bundle/Gesso.app"
echo ">> 组装 ${APP}（版本 ${VERSION}）"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# 二进制
cp "$BIN" "$APP/Contents/MacOS/gesso"

# Info.plist（替换版本占位）
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# App 图标
cp crates/app/assets/icons/mac/Gesso.icns "$APP/Contents/Resources/Gesso.icns"

# 运行时资源：宿主页 + 内置样例（assets_dir() 在 release 下定位 Resources/assets）
mkdir -p "$APP/Contents/Resources/assets"
cp -R crates/app/assets/host "$APP/Contents/Resources/assets/host"
cp -R crates/app/assets/samples "$APP/Contents/Resources/assets/samples"

echo ">> 完成：${APP}"
echo "   本地试运行：open \"${APP}\""
