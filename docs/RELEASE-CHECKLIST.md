# 0.1 Release Checklist

Gesso 首次公开发布（0.1.0）的操作清单。按顺序执行；每条都是可勾选项，不引入
仓库里没有的工具链（能用本机命令就不绑新依赖）。

> 目标产物：
> - **macOS**：`Gesso.app`（Universal 可选，见下）→ `.dmg`
> - **Windows**：便携 `.exe` / 或安装器（见下决策）
> - 一个 GitHub Release，附产物 + SHA-256 + release notes
>
> 发布是**人工执行**的一次性流程；0.1 不做全自动 release pipeline（YAGNI，
> 手动跑顺了再考虑 GitHub Actions 自动化）。

---

> **不做代码签名 / 公证**：Gesso 是无签名凭据的开源项目，0.1 两平台产物均为**未签名**。
> 发布时只在 release notes 里写清首次打开方式（macOS 右键打开 / Windows 过 SmartScreen），
> 不为此申请证书，也不阻塞发布。

## 阶段 0 — 发布决策（开工前先定）

- [ ] **macOS 架构**：先只发 `aarch64`（Apple Silicon），还是同时做 Intel（`x86_64`）/ Universal binary。
- [ ] **Windows 分发形态**：便携 zip（解压即用）还是安装器（`msi`/NSIS/Inno）。0.1 建议**便携 zip**，最省事。
- [ ] **最低系统版本**：macOS（建议 12 Monterey+）/ Windows（建议 10 1809+，WebView2 随系统或引导安装）。
- [ ] WebView2 Runtime 策略：Windows 上是否随包引导安装（Evergreen Bootstrapper）。

---

## 阶段 1 — 代码冻结与质量门

- [ ] 合入所有要进 0.1 的 PR；main 处于稳定状态。
- [ ] `cargo build -p gesso-app`：0 error / 0 warning。
- [ ] `cargo build -p gesso-app --all-targets`：0 warning（含 examples）。
- [ ] `cargo fmt --all --check` 通过。
- [ ] `cargo clippy -p gesso-core --all-targets -- -D warnings` 通过。
- [ ] `cargo test -p gesso-core -p gesso-app`：全绿。
- [ ] CI 在 `macos-latest` + `windows-latest` 全绿。
- [ ] 无遗留调试代码（grep `diag` / `println!` / 临时代码块）。
- [ ] 无大文件 / 临时产物入库（`thumb*.png`、`/tmp` 夹具、`*.bundle`）。

## 阶段 2 — 实机冒烟（两平台都跑一遍）

按用户实际路径操作，不靠日志想当然：

- [ ] 全新安装（清空配置目录后首次启动）→ 首启向导出现。
- [ ] 内置样例自动上主屏并渲染。
- [ ] 四种壁纸各应用一次：**视频 / 图片 / Shader / HTML**。
- [ ] **WE 导入**：选一个真实 WE `project.json`（video 一个、web 一个）→ 渲染、零拷贝、源文件未被改。
- [ ] 托盘菜单全项可用：暂停全部 / 切换 / 管理窗口 / 开机自启 / 退出。
- [ ] 暂停恢复：视频真停解码（非仅黑屏）。
- [ ] 全屏应用 → 按设置策略自动暂停/降帧；退出恢复。
- [ ] 光标跟随壁纸（shader `iMouse` / html postMessage）正常。
- [ ] 电池供电策略触发（笔记本）。
- [ ] 重启系统后：配置恢复、壁纸自动上屏、开机自启生效。
- [ ] 失效素材不留下空白桌面（回退系统壁纸/占位）。
- [ ] **Windows 专项**：图标层之下渲染、点击穿透、explorer 重启自愈、125% DPI 对齐。
- [ ] **macOS 专项**：菜单栏全屏不遮挡壁纸、多 Space 跟随。
- [ ] **多显示器**（有条件时）：各屏独立贴壁。

## 阶段 3 — 版本号与变更记录

- [ ] 确认 `Cargo.toml`（workspace）`version` 与各 crate `version.workspace = true` 一致。
- [ ] 全仓搜旧版本号，确认无散落硬编码。
- [ ] 整理 `CHANGELOG.md`：
  - [ ] 把 `[Unreleased]` 下要发布的内容归到 `## [0.1.0] - YYYY-MM-DD`。
  - [ ] 新建空的 `[Unreleased]`（`Added/Changed/Fixed` 子标题）。
  - [ ] "Known limitations" 与 ROADMAP 现状一致。
- [ ] 更新 README 状态行（如"early development"→"0.1 stable/beta"）。
- [ ] 提交：`chore(release): 0.1.0`（**先不打 tag**，产物验证后再打）。

## 阶段 4 — macOS 打包

打包脚本在 `packaging/macos/`（纯 bash + `hdiutil`，无第三方依赖）：

```bash
packaging/macos/build_app.sh   # release 构建 + 组装 target/release-bundle/Gesso.app
packaging/macos/build_dmg.sh   # 打成 Gesso-<version>-<arch>.dmg + 打印 SHA-256
```

- [ ] 跑 `build_app.sh` 产出标准 `.app`：`Contents/MacOS`、`Contents/Resources`、`Info.plist`。
- [ ] `Info.plist`（模板 `packaging/macos/Info.plist`，版本由脚本注入）：
      `CFBundleIdentifier=com.zhanglun.gesso`、`CFBundleShortVersionString`、`LSMinimumSystemVersion=12.0`、
      `LSUIElement=true`（托盘应用，无 Dock 常驻）、`CFBundleIconFile`。
- [ ] 图标 `Gesso.icns` 打进 `Contents/Resources`。
- [ ] 资源：宿主页 `host/` + 内置样例 `samples/` 打进 `Contents/Resources/assets`。
      release 二进制的 `assets_dir()` 定位此处（**不再硬编码开发机路径**，见 protocol.rs）。
- [ ] **关键验证**：`SKIP_BUILD=1 build_app.sh` 后，用临时 `HOME` 跑打包后的二进制，
      确认 6 个内置样例从 bundle 复制入库、`gesso://host/index.html` 200、无 panic。
- [ ] release notes 写明未签名产物首次打开：**右键 → 打开**；被隔离时
      `xattr -dr com.apple.quarantine /Applications/Gesso.app` 兜底。
- [ ] 跑 `build_dmg.sh`，挂载验证含 `Gesso.app` + `Applications` 拖拽快捷方式。
- [ ] （可选，非必需）Universal：`rustup target add aarch64-apple-darwin x86_64-apple-darwin`，
      `lipo` 合并两个 release 二进制后再组装。

## 阶段 5 — Windows 打包

```powershell
cargo build --release -p gesso-app
```

- [ ] 在 release 目录确认 `.exe` + 内嵌图标（winresource）+ WebView2 依赖。
- [ ] 运行**打包后的 exe** 冒烟（贴壁 WorkerW、托盘、DPI、explorer 自愈）。
- [ ] **便携方案（0.1 建议）**：exe 打成 zip；release notes 说明解压即用。
- [ ] **安装器（可选）**：若要开始菜单/卸载项，用 Inno Setup 或 `cargo-wix`（msi）。
- [ ] release notes 写明未签名 + SmartScreen 首次拦截：**"更多信息" → "仍要运行"**。
- [ ] WebView2 Evergreen：确认目标机有 Runtime，或 release notes 附下载链接。
- [ ] 计算产物 SHA-256。

## 阶段 6 — 发布

- [ ] 给两个产物算校验值：`shasum -a 256`（macOS）/ `Get-FileHash`（Windows）。
- [ ] 打 tag 并推送：
  ```bash
  git tag -a v0.1.0 -m "Release 0.1.0"
  git push origin v0.1.0
  ```
- [ ] GitHub 新建 Release（基于 tag）：
  - [ ] 标题 `v0.1.0`
  - [ ] release notes = CHANGELOG 0.1.0 段落 + 两平台未签名首次打开说明 + SHA-256。
  - [ ] 上传 `.dmg`、Windows zip（及校验文件）。
  - [ ] 勾选预发布？0.1 可标 **pre-release**（首个版本，收集反馈）或正式发布——发布决策里定。
- [ ] 发布后从 **Release 页面下载产物**（非本地构建）在干净机器上各装一遍。

## 阶段 7 — 发布后

- [ ] 验证下载安装的产物真实可用（关键，防"本机构建能跑、发布产物缺资源"）。
- [ ] README / 官网（若有）更新下载链接与版本徽章。
- [ ] 公告渠道（release post / 社区）。
- [ ] 开启 0.2 规划：把发布后反馈整理进 ROADMAP。
- [ ] 确认本地无遗留临时产物（历史改写备份已删除）。

---

## 当前缺口（要在阶段 0/4/5 补齐的东西）

发布所需、仓库里当前的就绪情况：

1. ✅ **macOS `.app` 组装**：`packaging/macos/build_app.sh`（Info.plist 模板 + 资源定位修复）。
2. ✅ **macOS `.dmg`**：`packaging/macos/build_dmg.sh`（含 SHA-256）。
3. ⬜ **Windows 打包脚本**：release exe → 便携 zip（当前需手工压缩，待补脚本）。
4. ⬜ **release notes 模板**：可复用，含两平台校验值与未签名首次打开说明。

> 原则：0.1 先保证"两平台能下载、安装、跑起来"；不做签名/公证/自动更新，
> 通用二进制非必需。未签名产物在 release notes 写清打开方式即可，不阻塞发布。
