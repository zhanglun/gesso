# Roadmap

Status reflects what actually runs on hardware: ✅ means it was verified on a real machine. Time estimates assume spare-time pace.

## Where we are

```
✅ M0    workspace skeleton + CI
✅ M0.5  GPUI integration spike (tray × GPUI loop, native handles, theme tokens)
✅ M1.5  macOS desktop layering spike (pin below icons, click-through, all-spaces)
✅ M2    video wallpaper end-to-end (pin + pause/resume + persistence)
✅ M3    sessions/config/UI engine + UI↔engine actions + import + settings + first-run
✅ M4    renderer completeness — video / image / shader (WebGL2 + Shadertoy subset) / HTML (sandboxed iframe); all four kinds have real webview-captured thumbnails (static + 15-frame hover sequence)
✅ M5    system data bridge — fullscreen/battery auto-pause + time feed + cursor feed (iMouse) + idle downscale
✅ M6    Wallpaper Engine import I (video/web) + static images + content-type registry refactor
✅ M1    Windows pinning（实机验证：图标层下渲染 / TaskbarCreated 自愈 / PMv2 DPI）
⬜ M7    Wallpaper Engine import II (scene, long-term)
```

## Next up, in priority order

### M5 — system data bridge（✅ 已完成）
- ✅ fullscreen detection: `CGWindowList` layer-0 window covering a display frame（±3pt 容差，y 轴按主显示器高度翻转）→ 按全屏策略执行：暂停 / 降帧到 5 fps / 忽略。
- ✅ battery: IOKit power sources（`IOPSCopyPowerSourcesList`）。电池供电作用于全部显示器，本屏全屏优先。
- ✅ time feed: 引擎 ~1 Hz `evaluate` 驱动 `__gesso.tick(Date.now())`；html 壁纸收 `postMessage {__gesso:"time"}`。
- ✅ cursor feed: 全局光标位置路由到所在显示器的会话，归一化量化（u16）后经 `__gesso.mouse(present,x,y,buttons)` 喂入，shader 写 `iMouse`、html 收 `{__gesso:"mouse"}`；推送 30 Hz、宿主页每帧快速平滑，变化才推。鼠标静止 5 分钟自动降到 5 fps，一动即恢复。
- 权限事实（实测）：位置走 `NSEvent::mouseLocation`、按键/空闲走 CoreGraphics HID 源状态表——纯轮询、不做事件 tap，**无需 Input Monitoring / 辅助功能授权**（只有 `CGEventTapCreate` 才要）。

### M1 — Windows pinning（✅ 主体完成，2026-10-05 实机验证）
- ✅ 自有 Win32 壁纸窗口（WS_POPUP + TOOLWINDOW/NOACTIVATE，非 GPUI 窗口）+ wry/WebView2 子窗口直挂。
- ✅ WorkerW 挂载阶梯：`Progman 0x052C` → SHELLDLL_DefView 宿主之后的 WorkerW → `SetParent`；兜底 Progman（桌面图标关闭）/ 顶层 HWND_BOTTOM（explorer 未就绪）。实机：plasma shader 在图标层之下全屏渲染。
- ✅ **创建顺序生死线（实测踩坑）**：必须先 `SetParent` 挂载、后创建 WebView2——反之 DComp 视觉树绑定失效，窗口树全绿但整窗不可见（见工程笔记 §2）。
- ✅ DPI：进程顶部 `SetProcessDpiAwarenessContext(PMv2)`（GPUI/wry 均不设置），125% 缩放下物理像素与 WorkerW 精确对齐；`GPUI_DISABLE_DIRECT_COMPOSITION=1`。
- ✅ explorer 重启自愈：隐藏监听窗口收 `TaskbarCreated` → 引擎轮询整窗重建（explorer 死亡会连带销毁跨进程子窗口，重挂旧句柄无效）。实机验证：重启后自动落位 WorkerW 并恢复渲染。
- ✅ gesso:// 协议在 WebView2 下的 workaround 全链路：导航 `gesso://X/…` ↔ `http://gesso.X/…` 由 wry 翻译/还原，页面子资源 URL 由 `protocol::entry_url` 直接产出 workaround 形态，CSP 按 workaround 宿主枚举。
- ⬜ 多显示器实机验证（实现已就位：EnumDisplayMonitors 物理像素 + 各屏独立窗口；验证机单屏）。
- ⬜ 点击穿透语义对齐：WM_NCHITTEST HTTRANSPARENT 只作用于本窗口，空白桌面点击会落入 WebView2 子窗口（技术方案 §6 视作可选交互增强，v1 接受）。
- ⬜ M5 数据桥 Windows 侧（全屏检测 WinEventHook / 电池 / 光标 feed GetCursorPos，技术方案 §6）。

### M6 — Wallpaper Engine import I（✅ 已完成）
- ✅ 工坊扫描：`steamapps/workshop/content/431960/<id>/project.json`，支持 `video`/`web`；检测不到 Steam 时自动隐藏「工坊」入口（可用 `STEAM_DIR` 夹具验证）。
- ✅ WE web 垫片：`wallpaperRegisterAudioListener`/媒体等空实现 + 暂停/fps/鼠标/时间桥，注入点紧贴 `<head>`。
- ✅ 普通导入可选 `project.json` 触发整目录 WE 导入；法律边界：只读本机已订阅内容，不下载/不爬取/不再分发。
- ✅ 静态图：支持 jpg/jpeg/png/avif，UI 新增独立「图片」分类（底层复用 Image）；图片缩略图 Direct 直引源文件，零生成零拷贝。
- ✅ 架构还债（本次两个 bug 的结构性根因）：① `gesso-core::content` 内容类型描述表——类型↔扩展名↔MIME↔缩略图策略的单一事实源；② `HostCommand` 枚举类型化 Rust↔宿主页命令；③ main.rs 上帝循环拆解（`apply_engine_action`/`merge_snapshot`/命名定时）；④ session.rs 纯工具抽到 `encoding.rs`。
- ✅ **多 Steam 内容库**：解析主库 `steamapps/libraryfolders.vdf`，工坊扫描/`gesso://steam` 路由覆盖主库 + 全部已登记库（条目可能分散在不同盘/库）。
- scene 类型（M7）暂拒绝；`depkg` 为 GPL，倾向子进程隔离，见 [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md)。
- ✅ **`gesso://` 协议回归**（`bf3c294`）：lb-wry ≥0.53 自定义协议在 macOS 已正常（M2 “零回调”结论失效）。宿主页改为 `gesso://host/index.html` 统一副本，不再拷入条目目录；`ContentSpec.source` 为绝对 `gesso://library/<entry>/…`；实现视频 Range(206)。四类实机验证通过。

## Known gaps (no milestone yet)

- 零拷贝直引：WE video/web 已改为**只读直引 Steam 源**（不拷入库、不修改原文件），经 `gesso://steam` 路由流式 Range 读取、shim 内存注入。
- Real-machine monitor outline (signature #1) and tray panel positioning on retina displays.
- ~~HTML thumbnails use the gradient placeholder~~ → Done (`372b4c4`): html captures live frames via the same persistent-window capture pipeline as shader/video.
- HTML wallpapers: iframe-internal navigation is not allow-listed yet (top-navigation / popups / forms are sandbox-blocked; `fps_cap` is advisory for the html kind). Hover previews use a two-phase cycle — preload all 16 frames behind a spinner, then play at a fixed 125 ms/frame (`c6bd3ce`).
- i18n: UI strings are centralized in `ui/strings.rs` (Chinese-first); English translation pass pending.
- Tray polish: dynamic menu copy (muda handlers are `Send`-only — menu handle can't be mutated from the poller) and precise quick-panel positioning relative to the tray icon (retina coordinate conversion).
