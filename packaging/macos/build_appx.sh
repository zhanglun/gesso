#!/usr/bin/env bash
# 编译并组装 GessoWallpaper.appex（路线 A M1，技术方案 §10.6），嵌入指定 .app。
#
# 用法：build_appx.sh <目标 Gesso.app 路径>
#
# 依赖：swiftc（CommandLineTools 即可，CI runner 自带完整 Xcode）。
# 注意：嵌入后宿主 .app 需重新 codesign；每次重组装宿主 bundle 都要
# lsregister -f（WallpaperAgent 缓存旧扩展路径的坑，见技术方案 §10.6）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP="${1:?用法: build_appx.sh <Gesso.app 路径>}"
SRC="$ROOT/crates/app/swift/wallpaper-extension"

VERSION="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' "$ROOT/Cargo.toml" | head -1)"
APPEX="$APP/Contents/PlugIns/GessoWallpaper.appex"

echo ">> 编译壁纸扩展（swiftc）"
mkdir -p "$APPEX/Contents/MacOS"
sed "s/__VERSION__/${VERSION}/g" "$SRC/Info.plist" > "$APPEX/Contents/Info.plist"
swiftc -parse-as-library -swift-version 5 -O \
    -framework Foundation -framework AppKit \
    "$SRC"/Sources/*.swift \
    -o "$APPEX/Contents/MacOS/GessoWallpaper"

echo ">> 签名 appex（ad-hoc + App Sandbox）"
codesign --force --sign - \
    --entitlements "$SRC/GessoWallpaper.entitlements" \
    "$APPEX"

echo ">> appex 就绪：$APPEX"
