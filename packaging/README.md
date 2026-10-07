# 打包脚本

把 Gesso 构建成可分发产物的脚本，覆盖两平台。

**发布主路径是 CI**：推送 `v*` tag 后，`.github/workflows/release.yml` 的 matrix
会分别调用下面的脚本打包，并自动创建 GitHub Release（产物 + `checksums.txt` + notes）。
脚本本地也能跑，用于调试或手工打包。完整流程见 [../docs/RELEASE-CHECKLIST.md](../docs/RELEASE-CHECKLIST.md)。

所有脚本**不签名、不公证**，只用系统内置工具，无第三方依赖。

## 目录结构

```
packaging/
├─ README.md                 # 本文件
├─ macos/                    # macOS：.app / .dmg
│  ├─ Info.plist             # .app 元信息模板，__VERSION__ 由脚本注入
│  ├─ build_app.sh           # release 构建 + 组装 target/release-bundle/Gesso.app
│  └─ build_dmg.sh           # 打成 Gesso-<version>-<arch>.dmg（打印 SHA-256）
├─ windows/                  # Windows：便携 zip
│  └─ build_portable.ps1     # 打成 Gesso-<version>-x64-portable.zip（打印 SHA-256）
└─ release-notes/
   └─ v0.1.0.md              # GitHub Release 文案（下载/校验值/未签名首次打开）
```

## macOS

纯 bash + 系统自带 `hdiutil`。

```bash
# 1. release 构建 + 组装 .app → target/release-bundle/Gesso.app
packaging/macos/build_app.sh

# 2. 打成 .dmg → target/release-bundle/Gesso-<version>-<arch>.dmg（并打印 SHA-256）
packaging/macos/build_dmg.sh
```

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

PowerShell 5.1 / 7+，仅内置 cmdlet。产出解压即用的便携 zip
（顶层含 `Gesso/gesso.exe` + `assets/`；图标已内嵌 exe，WebView2 用系统 Evergreen）。

```powershell
packaging\windows\build_portable.ps1              # release 构建 + 打包
packaging\windows\build_portable.ps1 -SkipBuild   # 复用已有 release，只重新打包
```

产物：`target/release-bundle/Gesso-<version>-x64-portable.zip`（脚本打印 SHA-256）。

## Release notes

`release-notes/v0.1.0.md` 是 GitHub Release 的正式文案，已包含：
下载清单、两平台未签名首次打开说明、已知限制。CI 按 tag 名读取对应文件
（`packaging/release-notes/<tag>.md`）；**SHA-256 不用手填**——CI 自动生成并附上
`checksums.txt`。发新版本时复制一份改成对应 tag 名即可。
