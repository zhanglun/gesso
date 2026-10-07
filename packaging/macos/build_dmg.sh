#!/usr/bin/env bash
# 把 Gesso 打成 .dmg（不签名公证；.app 每次重新组装，幂等）。
#
# 用法：
#   packaging/macos/build_dmg.sh                  # 构建/复用 release 二进制 + 重组装 .app + 打 dmg
#   SKIP_BUILD=1 packaging/macos/build_dmg.sh     # 复用已有 release 二进制，跳过 cargo build
#
# 产物：target/release-bundle/Gesso-<version>-<arch>.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

VERSION="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
APP="target/release-bundle/Gesso.app"

DMG="target/release-bundle/Gesso-${VERSION}-${ARCH}.dmg"
STAGE="target/release-bundle/dmg-staging"
echo ">> 打包 ${DMG}"

# 每次都重新组装 .app（幂等，含 ad-hoc 重签；几秒的事）。
# 绝不能"已存在就跳过"：CI 的 rust-cache 会恢复 target/，里面的旧/残缺 .app
# 会被当现成产物打进 dmg（0.1.0 首发事故：17KB 空 dmg）。
SKIP_BUILD="${SKIP_BUILD:-0}" packaging/macos/build_app.sh

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
