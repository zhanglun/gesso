# 0.1 Release Checklist

Gesso 首次公开发布（0.1.0）的操作清单。按顺序执行；每条都是可勾选项，不引入
仓库里没有的工具链（能用本机命令就不绑新依赖）。

> 发布由 **GitHub Actions 自动完成**：推送 `v*` tag → `release.yml` 在两平台
> matrix 跑打包脚本 → 汇总自动创建 GitHub Release，附产物 + `checksums.txt` + release notes。
>
> 目标产物：
> - **macOS**：`Gesso-<version>-arm64.dmg`（Windows 同构脚本本地也可跑）
> - **Windows**：`Gesso-<version>-x64-portable.zip`
> - 一个 GitHub Release（`.dmg` + `.zip` + `checksums.txt` + notes）

---

> **不做代码签名 / 公证**：Gesso 是无签名凭据的开源项目，0.1 两平台产物均为**未签名**。
> 发布时只在 release notes 里写清首次打开方式（macOS 右键打开 / Windows 过 SmartScreen），
> 不为此申请证书，也不阻塞发布。

## 阶段 0 — 发布决策（开工前先定）

- [x] **macOS 架构**：先只发 `aarch64`（Apple Silicon），还是同时做 Intel（`x86_64`）/ Universal binary。
- [x] **Windows 分发形态**：便携 zip（解压即用）还是安装器（`msi`/NSIS/Inno）。0.1 建议**便携 zip**，最省事。
- [x] **最低系统版本**：macOS（建议 12 Monterey+）/ Windows（建议 10 1809+，WebView2 随系统或引导安装）。
- [x] WebView2 Runtime 策略：Windows 上是否随包引导安装（Evergreen Bootstrapper）。

---

## 阶段 1 — 代码冻结与质量门

- [x] 合入所有要进 0.1 的 PR；main 处于稳定状态。
- [x] `cargo build -p gesso-app`：0 error / 0 warning。
- [x] `cargo build -p gesso-app --all-targets`：0 warning（含 examples）。
- [x] `cargo fmt --all --check` 通过。
- [x] `cargo clippy -p gesso-core --all-targets -- -D warnings` 通过。
- [x] `cargo test -p gesso-core -p gesso-app`：全绿。
- [x] CI 在 `macos-latest` + `windows-latest` 全绿。
- [x] 无遗留调试代码（grep `diag` / `println!` / 临时代码块）。
- [x] 无大文件 / 临时产物入库（`thumb*.png`、`/tmp` 夹具、`*.bundle`）。

## 阶段 2 — 实机冒烟（两平台都跑一遍）

按用户实际路径操作，不靠日志想当然：

- [x] 全新安装（清空配置目录后首次启动）→ 首启向导出现。
- [x] 内置样例自动上主屏并渲染。
- [x] 四种壁纸各应用一次：**视频 / 图片 / Shader / HTML**。
- [x] **WE 导入**：选一个真实 WE `project.json`（video 一个、web 一个）→ 渲染、零拷贝、源文件未被改。
- [x] 托盘菜单全项可用：暂停全部 / 切换 / 管理窗口 / 开机自启 / 退出。
- [x] 暂停恢复：视频真停解码（非仅黑屏）。
- [x] 全屏应用 → 按设置策略自动暂停/降帧；退出恢复。
- [x] 光标跟随壁纸（shader `iMouse` / html postMessage）正常。
- [x] 电池供电策略触发（笔记本）。
- [x] 重启系统后：配置恢复、壁纸自动上屏、开机自启生效。
- [x] 失效素材不留下空白桌面（回退系统壁纸/占位）。
- [x] **Windows 专项**：图标层之下渲染、点击穿透、explorer 重启自愈、125% DPI 对齐。
- [x] **macOS 专项**：菜单栏全屏不遮挡壁纸、多 Space 跟随。
- [x] **多显示器**（有条件时）：各屏独立贴壁。

## 阶段 3 — 版本号与变更记录

- [x] 确认 `Cargo.toml`（workspace）`version` 与各 crate `version.workspace = true` 一致。
- [x] 全仓搜旧版本号，确认无散落硬编码。
- [x] 整理 `CHANGELOG.md`：
  - [x] 把 `[Unreleased]` 下要发布的内容归到 `## [0.1.0] - YYYY-MM-DD`。
  - [x] 新建空的 `[Unreleased]`（`Added/Changed/Fixed` 子标题）。
  - [x] "Known limitations" 与 ROADMAP 现状一致。
- [x] 更新 README 状态行（如"early development"→"0.1 stable/beta"）。
- [x] 提交：`chore(release): 0.1.0`（**先不打 tag**，产物验证后再打）。

## 阶段 4 & 5 — 两平台打包脚本（CI 调用，本地也能跑）

脚本是 `release.yml` matrix 的执行体；脚本本身已本地验证（macOS）：

```bash
# macOS（bash + hdiutil）
packaging/macos/build_app.sh   # release 构建 + 组装 target/release-bundle/Gesso.app
packaging/macos/build_dmg.sh   # 打成 Gesso-<version>-arm64.dmg

# Windows（PowerShell 5.1+/7）
packaging\windows\build_portable.ps1   # 打成 Gesso-<version>-x64-portable.zip
```

打包脚本约定（两平台一致）：

- [x] **macOS `.app`**：`Contents/MacOS/gesso` + `Info.plist`（`com.gesso.dev`、版本注入、
      `LSMinimumSystemVersion=12.0`、`LSUIElement=true`）+ `Gesso.icns` + `Resources/assets/{host,samples}`。
- [x] **Windows zip**：顶层 `Gesso/` 含 `gesso.exe`（图标内嵌 winresource）+ `assets/{host,samples}`；
      WebView2 用系统 Evergreen，不打包。
- [x] release 的 `assets_dir()` 定位 bundle/exe 内资源，**不硬编码开发机路径**（见 protocol.rs）。
- [x] release notes 写明未签名首次打开：macOS **右键 → 打开** / `xattr` 兜底；
      Windows **SmartScreen → 更多信息 → 仍要运行**。
- [x] （可选，非必需）Universal/Intel、Windows 安装器均不在 0.1 范围。

## 阶段 6 — 触发 CI 发布

- [x] **（推荐先干跑）** Actions 页手动触发 **Release** workflow（`workflow_dispatch`）：
      只跑两平台打包并上传为构建 artifact，**不创建 Release**。确认两平台都能打包成功。
- [x] 确认 release notes 文件就位：`packaging/release-notes/v0.1.0.md`
      （workflow 按 tag 名 `body_path: packaging/release-notes/<tag>.md` 读取）。
- [x] 打 tag 并推送（**唯一需要的发布动作**）:
  ```bash
  git tag -a v0.1.0 -m "Release 0.1.0"
  git push origin v0.1.0
  ```
- [x] CI 自动：matrix 打包 → 汇总生成 `checksums.txt` → 创建 Release（标题=tag、notes=对应 md）
      并上传 `.dmg` + `.zip` + `checksums.txt`。
- [x] 在 Actions 页确认 `release.yml` 三个 job（两平台 build + release）全绿。
- [x] 预发布/正式：若想标 pre-release，打完后在 Release 设置里勾选（0.1 首个版本可考虑）。
- [x] 发布后从 **Release 页面下载产物**（非本地构建）在干净机器上各装一遍。

## 阶段 7 — 发布后

- [x] 验证下载安装的产物真实可用（关键，防"本机构建能跑、发布产物缺资源"）。
- [x] README 双语已更新状态行 + Releases 下载链接（版本徽章暂未加）。
- [ ] 公告渠道（release post / 社区）。
- [ ] 开启 0.2 规划：把发布后反馈整理进 ROADMAP。
- [x] 确认本地无遗留临时产物（历史改写备份已删；/tmp 测试残留已清）。

---

## 就绪情况（0.1.0 已发布，以下为发布后沉淀）

发布自动化就绪情况：

1. ✅ **Release workflow**：`.github/workflows/release.yml`（tag 触发，matrix 两平台，自动发 Release）。
2. ✅ **打包脚本**：macOS `build_app.sh`/`build_dmg.sh`、Windows `build_portable.ps1`。
3. ✅ **release notes**：`packaging/release-notes/v0.1.0.md`；校验值由 CI 生成 `checksums.txt`。
4. ✅ workflow 已过 `actionlint`；macOS 脚本本地验证，Windows 脚本靠 `workflow_dispatch` 干跑首次验证。

> 原则：0.1 先保证"两平台能下载、安装、跑起来"；不做签名/公证/自动更新，
> 通用二进制/安装器非必需。未签名产物在 release notes 写清打开方式即可。
