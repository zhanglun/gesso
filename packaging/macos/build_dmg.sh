#!/usr/bin/env bash
# 把已组装的 Gesso.app 打成 .dmg（不签名）。
#
# 用法：
#   packaging/macos/build_dmg.sh            # 先确保 .app 存在（不存在则构建），再打 dmg
#
# 产物：target/release-bundle/Gesso-<version>-<arch>.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

VERSION="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
APP="target/release-bundle/Gesso.app"

# .app 不在则先构建
if [[ ! -d "$APP" ]]; then
    echo ">> 未找到 ${APP}，先执行 build_app.sh"
    packaging/macos/build_app.sh
fi

DMG="target/release-bundle/Gesso-${VERSION}-${ARCH}.dmg"
STAGE="target/release-bundle/dmg-staging"
echo ">> 打包 ${DMG}"

rm -f "$DMG"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/Gesso.app"
# 拖拽到 Applications 的快捷方式
ln -s /Applications "$STAGE/Applications"

# UDZO = zlib 压缩只读；现代 macOS 可直接打开
hdiutil create \
    -volname "Gesso" \
    -srcfolder "$STAGE" \
    -ov \
    -format UDZO \
    "$DMG" >/dev/null

rm -rf "$STAGE"

# SHA-256 校验值（release 时一并公布）
SHASUM="$(shasum -a 256 "$DMG" | awk '{print $1}')"
echo ">> 完成：${DMG}"
echo "   SHA-256: ${SHASUM}"
