# FAQ

## 支持哪些系统？

macOS（已验证）与 Windows（进行中，需要真机验证贴壁）。**v1 不做 Linux**：KDE（Plasma 壁纸插件）、GNOME Shell（无官方嵌入点）、X11（root window）与 Wayland 各需一套独立的桌面集成路径，投入产出比最低。架构上 `pin/` 是平台 trait，将来添加 Linux 后端不需要动引擎其余部分。

## 构建 macOS 版真的不需要 Xcode 吗？

不需要。Metal 着色器经 GPUI 快照的 `runtime_shaders` 特性在**运行时**由驱动编译，构建期不调用 `metal` 工具——Command Line Tools 即可（这在本仓库的 spike 中实测确认）。代价只是首启多 ~毫秒级的着色器编译。

首次启动 Xcode 的另一种情况：如果你看到 tray 初始化 panic（`Ivar platform not found on class NSApplication`），那不是缺 Xcode，而是壁纸窗口在 `gpui_kit::init` 之前创建了——属于 bug，请提 issue。

## 会上传数据吗 / 有遥测吗？

没有，也没有计划。天气联动（未来）需要你显式填数据源，网络请求只发生在你开启它之后。

## 和 Wallpaper Engine 的素材兼容吗？

支持（M6，见 [WALLPAPER-ENGINE.md](WALLPAPER-ENGINE.md)）：你在导入对话框主动选择 WE 的 `project.json`，Gesso 只解析和只读使用你指定的内容，不扫描磁盘、不枚举 Steam/工坊、不下载或再分发。`video`/`web` 已支持；`scene` 是 M7 目标，`application` 类型（原生 exe）永远不会支持。

## 为什么导入的文件被复制进库目录，而不是原地引用？

普通导入（视频/图片/Shader/HTML）采用自包含：素材入库目录，webview 按扩展名判型、相对路径引用、失效判定都自洽。WE 条目（M6 起）则是**零拷贝只读引用**你指定的原目录、不复制素材——两类共存。

`.mkv` 与 HEVC 会被拒绝并给出提示（转封装 / 系统扩展），因为 webview 的解码能力由系统决定。

## 全屏应用时壁纸会怎样？

已实现（M5，可在设置里选策略）：检测到前台全屏应用时自动暂停（0 GPU 开销）或降帧到 5 fps，退出后恢复；电池供电时同样适用。你也可以随时从托盘手动暂停（视频真的会停）。

## 托盘点「暂停全部」再点一次没恢复？

已修复（`fetch_xor` 那个坑）。如果你遇到的是别的现象，请按 bug 模板附上日志。

## 截图/录屏里看不到壁纸窗口？

检查 系统设置 → 隐私与安全性 → **屏幕录制** → 是否给终端（或录屏 App）授权。未授权时截图只能拍到桌面壁纸——这是 macOS 的权限模型，不是 Gesso 的问题。

## 怎么卸载？

1. 退出应用（托盘 → 退出）；
2. 删除 `~/Library/Application Support/Gesso/`（macOS）；
3. 如开启过开机自启：系统设置 → 通用 → 登录项 里移除 Gesso。

## 我想写一个 HTML 壁纸，契约是什么？

单文件自包含即可（相对路径的子资源可以带；`fetch`/`XHR` 不可用）。Gesso 把它存为条目里的 `wallpaper.html`，宿主页以**沙箱 iframe**（`sandbox="allow-scripts"`）装载：

- **无存储 / IPC**：opaque origin 下 `localStorage`、`sessionStorage`、`document.cookie`、`IndexedDB`、`window.top` 一律抛 `SecurityError`（实机 DoD 已验）；
- **无导航逃逸**：顶页导航、弹窗、表单被沙箱静默拦截（iframe 内部自身跳转暂无白名单，见 ROADMAP 已知缺口）；
- **暂停 / 恢复**：监听 `message` 事件，收 `{__gesso: "pause"}` / `{__gesso: "resume"}` 后自行停/启动动画（引擎不硬杀帧）；
- **fps_cap 是建议值**：html 壁纸自管 RAF。

内置样例 Clock（`builtin-html-clock`）就是这套契约的参考实现。背景知识见 [ARCHITECTURE](ARCHITECTURE.md) §3。

## 为什么选 GPUI 而不是 Electron/Tauri？

体积与内存（Rust 全栈、无 Node 常驻）、渲染一致性（GPU 加速 UI 与壁纸同进程生态）、以及对"桌面级窗口"这种系统集成的完全控制权。代价是绑定 GPUI 快照通道（`gpui-pre`）——升级纪律见工程笔记。
