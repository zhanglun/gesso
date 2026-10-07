# 本地开发与调试指南

> 面向：要在本机构建、运行、调试 Gesso 的开发者。
> 配套阅读：[ARCHITECTURE.md](ARCHITECTURE.md)（架构）· [ENGINEERING-NOTES.md](ENGINEERING-NOTES.md)（架构规则与踩坑）· [../design/技术方案.md](design/技术方案.md)（完整工程方案）· [../crates/app/API.md](../crates/app/API.md)（引擎接口）

---

## 1. 环境准备

| 项 | 要求 |
|----|------|
| Rust | 较新的 stable（当前开发用 **1.99**，与 CI 同步； edition 2021） |
| macOS | Command Line Tools 即可。**不需要完整 Xcode**——Metal 着色器经 GPUI 快照的 `runtime_shaders` 特性在运行时编译（ spike 实测确认） |
| Windows | MSVC 工具链（WebView2 SDK 随之提供；已在真机验证） |
| 可选 | `ffmpeg`（生成测试素材）、终端的**屏幕录制**权限（授权后 `screencapture` 才能拍到窗口内容，用于验证壁纸） |

> 曾有结论"macOS 构建需要完整 Xcode"——那是 `gpui 0.2.2` 直依赖时的旧行为；换用 gpui-kit（`gpui-pre` 快照 + `runtime_shaders`）后已不需要。若未来升级依赖后构建报 `unable to find utility "metal"`，才需要装 Xcode。

## 2. 常用命令

> **本地 rustfmt 要与 CI 对齐**：CI 用最新 stable 的 rustfmt，排版规则随版本变。
> 若 `cargo fmt --all --check` 本地过、CI 挂（或反过来），先 `rustup update stable`
> 再 `cargo fmt --all`。升级 Rust 后建议跑一次这条，避免提交一堆意外重排版。

```bash
cargo build -p gesso-app                  # 构建
cargo run   -p gesso-app                  # 运行（托盘 + 管理窗口 + 壁纸）
cargo test  -p gesso-core -p gesso-app    # 单测（core 20 项 + app 31 项）
cargo clippy -p gesso-core --all-targets -- -D warnings
cargo fmt -p gesso-core
pkill -f "target/debug/gesso"             # 退出（有单实例锁）
```

日志走 stdout/stderr，建议重定向后查看：

```bash
cargo run -p gesso-app 2>&1 | tee /tmp/gesso.log
```

日志关键行前缀：`[boot]`（启动/首启）、`[session]`（会话创建/恢复/URL）、`[tray]`（托盘动作）、`[diag]`（窗口与 webview 几何真值）、`[M1.5]`（压层参考实现）。

**开发期多实例**：默认单实例锁会拒绝第二个实例。需要并排对比时：

```bash
GESSO_LOCK=dev ./target/debug/gesso       # 用独立锁名起一个开发实例
```

> ⚠️ 多实例会叠多个壁纸窗口与托盘，验证完记得 `pkill -f "target/debug/gesso"`。

## 3. 调试手册

### 3.1 壁纸窗口"看不见/不渲染"

按顺序排除（每一步都有实测先例，详见 ENGINEERING-NOTES §2）：

1. **截图权限**：系统设置 → 隐私与安全性 → 屏幕录制 → 给终端授权。未授权时 `screencapture` 只拍到桌面壁纸，像素判读全部失真（本项目踩过的最大的坑）。
2. **验证窗口存在**：`osascript -e 'tell application "System Events" to tell process "gesso" to get {position, size, name} of every window'`
3. **别用裁剪截图测像素**——窗口可能不在你以为的位置（曾因此误判三轮）。要么用 `screencapture -R x,y,w,h` 精确到窗口区域，要么直接看屏幕。
4. **区分"加载失败"与"播放失败"**：宿主页内置诊断色——红=媒体错误、绿=播放中、蓝=已加载但暂停、品红=调用过 pause、橙=resume。颜色即状态。
5. **运动判据**：间隔 1.5s 截两帧比 sha256——相同=静止（视频没播），不同=在播。注意测试图源（testsrc2）本身大部分区域是暗色，均值接近系统壁纸，**别用均值下结论**。

### 3.2 壁纸窗口挂载诊断（DIAG）

`pin/macos.rs::create` 与会话轮询内置了真值打印：窗口 level / visible / occluded / frame、contentView 与每个 subview 的 frame、webview 当前 URL。排查"窗口在不在、webview 挂没挂、几何对不对"直接看 `[diag]` 行。

已钉死的事实：level 必须是 `-2147483604`（图标层下缘之下）；窗口 `origin.y` 会被 GPUI 坐标换算压到 `-30`——当前由 `session` 轮询里的 webview 偏移补偿（见 `force/补偿` 逻辑与 ENGINEERING-NOTES §2 的 swap 教训）。

### 3.3 探针页技巧

怀疑"页面没加载"还是"加载了没渲染"时，写一个 `/tmp/probe.html`（纯色 + 大字 + 秒级变色脚本），`window.load("file:///tmp/probe.html")` 后截图：
- 看到颜色 → 渲染管线正常，问题在内容/URL；
- 看到桌面壁纸 → webview 没挂上或没绘制（按 §2 排查表走）。

### 3.4 托盘与菜单

- 托盘左键（无菜单时）与右键菜单项都会进 `EngineAction` 队列；菜单文案**不能**在轮询里动态修改（muda handler 要求 `Send`）——需要动态文案时改用「动作 → 快照回灌」模式。
- `PredefinedMenuItem::quit` 在 GPUI 循环下不可靠，退出请走自定义菜单项 + `cx.quit()`。

### 3.5 宿主页调试

宿主页没有 devtools 入口（v1），调试手段：
- 页面状态写进**背景色**（§3.1 的诊断色即此思路）；
- `evaluate_script` 从 Rust 侧注入探针：`wv.evaluate_script("document.title = JSON.stringify(window.__gesso_state)")` 后读窗口标题；
- 视频/媒体问题先查 MIME（`protocol.rs::mime_of`）与相对路径——webview 按扩展名判定类型。

## 4. 排查速查表

| 症状 | 根因 | 处置 |
|------|------|------|
| 桌面纯白 | 宿主页加载失败（协议/URL 非法） | 看 `[protocol]` 日志；检查路径编码（空格！） |
| 桌面显示系统壁纸（静止） | webview 未挂载 / 页面透明 | `[diag]` 看 subview 数量与 frame；查创建顺序（§4.1 顺序陷阱） |
| 壁纸不动（有画面） | 自动播放被策略拦截 / 视频解码失败 | 确认 `with_autoplay(true)`；看宿主页诊断色 |
| 图标点不动 | 穿透标志丢失 | 查 `setIgnoresMouseEvents(true)` 是否生效 |
| 顶部菜单栏带露馅 | 窗口 `origin.y` 被 GPUI 压到 -30 | 确认几何补偿逻辑在轮询中生效（M2 记录） |
| explorer/ Finder 重启后壁纸消失 | 重钉逻辑未触发 | 查 `TaskbarCreated`（Win）/ 空间通知（mac）监听 |
| 双实例叠影 | 单实例锁被 `GESSO_LOCK` 绕过 | `pkill -f gesso` 清场后重跑 |

## 5. 发布构建

```bash
cargo build --release -p gesso-app
```

- macOS：`target/release/gesso` 单文件即可运行；.app 打包与 DMG 是 M3 后续（见技术方案 §11）。
- Windows：`GPUI_DISABLE_DIRECT_COMPOSITION=1` 环境变量是 WebView2 渲染的前提（gpui-kit 官方示例同款要求），打包脚本需写入。

## 6. 相关文档

| 文档 | 内容 |
|------|------|
| [design/技术方案.md](design/技术方案.md) | **完整工程方案**（14 章：选型/架构/贴壁/渲染/数据桥/安全/WE 兼容/节奏/风险） |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构提炼（英文） |
| [ENGINEERING-NOTES.md](ENGINEERING-NOTES.md) | 架构规则 + 踩坑实录（**改代码前必读**） |
| [../crates/app/API.md](../crates/app/API.md) | UI 层唯一允许调用的引擎接口 |
| [design/界面与交互设计.md](design/界面与交互设计.md) | 冻结的界面交互规格 v1.0 |
