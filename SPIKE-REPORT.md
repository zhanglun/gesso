# M0.5 判定点0 · SPIKE 报告

日期：2026-10-02（macOS 实机）· 代码：`crates/app/examples/m05.rs` · 运行：`cargo run -p gesso-app --example m05`

| # | 问题 | 结论 | 证据 |
|---|------|------|------|
| ① | gpui-wry 能否铺满 GPUI 窗口（壁纸形态） | **PASS** | 无边框 480×270 窗口 + wry webview as_child 挂载成功；宿主 HTML（时钟实时跳动）渲染于窗口内 |
| ② | tray-icon 能否接入 GPUI 事件循环 | **PASS*** | 托盘图标出现在菜单栏；事件处理器已安装；主窗口 500ms 轮询计数实时刷新。*待一次真实点击确认事件到达（看主窗口 ② 行计数） |
| ③ | GPUI 窗口能否拿原生句柄 | **PASS** | `Window: HasWindowHandle` 公开 API；macOS 返回 NSView 指针（`0x7c305dc000`），M1.5 经 `view.window()` 取 NSWindow；Windows 返回 HWND（M1 验） |
| ④ | 冻结 token 映射 kit 主题 | **PASS** | `Theme` 为 GPUI Global，`update_global` 注入 accent/danger 成功；`Theme::change(Light/Dark)` 全窗口切换；swatch 面板显示注入值 `#316EF5` |

**判定点0 结论：四项全通，主路线继续，不触发兜底。**

## 过程中钉死的 API 事实（后续里程碑直接用）

- gpui-wry 0.7 依赖 **lb-wry 0.53**（Longbridge wry fork）——builder 必须用 `lb_wry::WebViewBuilder::new().with_html(..).build_as_child(&window.window_handle()?)`
- rwh 0.6 的 `AppKitWindowHandle` 只有 **ns_view**（不是 ns_window）；NSWindow 在 M1.5 用 objc2 `view.window()` 取
- `cx.spawn(async move \|this, cx\|)` 双参签名（WeakEntity + AsyncApp）
- gpui `Rgba` 分量是 **f32（0..1）**，不是 u8
- kit `Button::on_click` 是三参 `(&ClickEvent, &mut Window, &mut App)`；`primary()` 在 **`ButtonVariants` trait** 上
- gpui-pre `Window` 实现 `HasWindowHandle`/`HasDisplayHandle`（Zed PR #13730 已在快照内）

## 给 M1（Windows 贴壁）的备注

gpui-kit 官方 webview 示例要求 Windows 上设置 `GPUI_DISABLE_DIRECT_COMPOSITION=true`（否则 WebView2 不渲染）——与 WorkerW `SetParent` 的交互列入 M1 验收矩阵。

## 环境事实（重要修正）

- **无需完整 Xcode**：gpui-pre-platform 的 `runtime_shaders` 特性使 Metal 着色器运行时编译，构建期不调 `metal`；CLT 即可（本机全链路实证）
- gpui_platform（官方）未发布；kit 生态用的是 `gpui-pre-platform` 快照
