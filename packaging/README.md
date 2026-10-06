# 打包脚本

把 Gesso 构建成可分发产物的脚本。当前仅 macOS；Windows 待补。
完整发布流程见 [../docs/RELEASE-CHECKLIST.md](../docs/RELEASE-CHECKLIST.md)。

## macOS

纯 bash + 系统自带 `hdiutil`，**无需第三方工具、不签名不公证**。

```bash
# 1. release 构建 + 组装 .app → target/release-bundle/Gesso.app
packaging/macos/build_app.sh

# 2. 打成 .dmg → target/release-bundle/Gesso-<version>-<arch>.dmg（并打印 SHA-256）
packaging/macos/build_dmg.sh
```

文件说明：

| 文件 | 作用 |
|------|------|
| `macos/Info.plist` | `.app` 元信息模板，`__VERSION__` 由脚本注入 |
| `macos/build_app.sh` | release 构建，把二进制 / 图标 / `host/` / `samples/` 组装成标准 `Gesso.app` |
| `macos/build_dmg.sh` | 把 `.app` + Applications 快捷方式压成可分发 `.dmg` |

### 资源定位（重要）

`crates/app/src/protocol.rs` 的 `assets_dir()`：

- **debug 构建**：用源码目录 `crates/app/assets`（改宿主页/样例立即生效）。
- **release 构建**：定位 `.app` 内的 `Contents/Resources/assets`。
  绝不能用 `option_env!("CARGO_MANIFEST_DIR")`——那是编译期绝对路径，
  会固化成开发机路径，导致别的机器上宿主页加载失败。

### 验证打包产物（不污染真实配置）

```bash
SKIP_BUILD=1 packaging/macos/build_app.sh
TMPHOME=$(mktemp -d)
HOME="$TMPHOME" GESSO_LOCK=dev \
  target/release-bundle/Gesso.app/Contents/MacOS/gesso > /tmp/g.log 2>&1 &
sleep 9
grep -c "内置样例已入库" /tmp/g.log    # 应为 6
grep -iE "panic|error|404" /tmp/g.log  # 应无输出
# 结束后清理
pkill -f release-bundle/Gesso.app; rm -rf "$TMPHOME" /tmp/g.log
```

> 未签名产物首次打开：访达里**右键 → 打开**；被隔离时
> `xattr -dr com.apple.quarantine /Applications/Gesso.app`。

## Windows

待补：`cargo build --release -p gesso-app` → exe 压便携 zip。
当前需手工完成，见发布清单阶段 5。
