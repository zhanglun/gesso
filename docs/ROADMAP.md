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

### M5-W — Windows 系统数据桥（✅ 2026-10-06 实机验证）

- ✅ 光标 feed：GetCursorPos（顶左物理像素）+ GetAsyncKeyState 按键 + GetLastInputInfo 空闲（含键盘活动）；左下契约翻转在 poll_mouse 契约边界一次完成。实机：光标 glow 随 SetCursorPos 移动/离开（条带亮度 366 ↔ 114）。
- ✅ 全屏检测：EnumWindows 找 rect 完整覆盖显示器 frame（±3px）的可见顶层窗口，排除自身类/shell（Progman/WorkerW 恒全屏）/工具窗/DWM cloak 幽灵。实机：真全屏触发自动暂停、退出解除、与空闲降帧状态互斥正确。
- ✅ 电池：GetSystemPowerStatus（BatteryFlag 128 = 无电池 → None；台式机正确返回 None）。
- ✅ 空闲降帧：5 分钟无输入 → 5 fps，一动即恢复（实机触发复验）。

### M1 — Windows pinning（✅ 主体完成，2026-10-05 实机验证）
- ✅ 自有 Win32 壁纸窗口（WS_POPUP + TOOLWINDOW/NOACTIVATE，非 GPUI 窗口）+ wry/WebView2 子窗口直挂。
- ✅ WorkerW 挂载阶梯：`Progman 0x052C` → SHELLDLL_DefView 宿主之后的 WorkerW → `SetParent`；兜底 Progman（桌面图标关闭）/ 顶层 HWND_BOTTOM（explorer 未就绪）。实机：shader / html / **视频（协议 Range 206 流式）**均在图标层之下全屏渲染。
- ✅ **创建顺序生死线（实测踩坑）**：必须先 `SetParent` 挂载、后创建 WebView2——反之 DComp 视觉树绑定失效，窗口树全绿但整窗不可见（见工程笔记 §2）。
- ✅ DPI：进程顶部 `SetProcessDpiAwarenessContext(PMv2)`（GPUI/wry 均不设置），125% 缩放下物理像素与 WorkerW 精确对齐；`GPUI_DISABLE_DIRECT_COMPOSITION=1`。
- ✅ explorer 重启自愈：隐藏监听窗口收 `TaskbarCreated` → 引擎轮询整窗重建（explorer 死亡会连带销毁跨进程子窗口，重挂旧句柄无效）。实机验证：重启后自动落位 WorkerW 并恢复渲染。
- ✅ gesso:// 协议在 WebView2 下的 workaround 全链路：导航 `gesso://X/…` ↔ `http://gesso.X/…` 由 wry 翻译/还原，页面子资源 URL 由 `protocol::entry_url` 直接产出 workaround 形态，CSP 按 workaround 宿主枚举。
- ⬜ 多显示器实机验证（实现已就位：EnumDisplayMonitors 物理像素 + 各屏独立窗口；验证机单屏）。
- ⬜ 点击穿透语义对齐：WM_NCHITTEST HTTRANSPARENT 只作用于本窗口，空白桌面点击会落入 WebView2 子窗口（技术方案 §6 视作可选交互增强，v1 接受）。
- ⬜ M5 数据桥 Windows 侧（全屏检测 WinEventHook / 电池 / 光标 feed GetCursorPos，技术方案 §6）。

### M4-W — 缩略图管线 Windows 接入（✅ 2026-10-06 实机验证）

背景：M4 的缩略图生成（视频 AVFoundation 抽帧 / shader·html WKWebView 快照）是 Apple 专属，
Windows 库页无帧图、无 hover 预览（image 类型 Direct 直引不受影响）。方案：**以 Windows 为
契机把抽帧收敛到 webview 一条管线**，业务逻辑（调度/抽取/落盘）单份，平台面只剩 html 截图
一个薄接缝。macOS 现有路径本次零改动（已验证、已投产），待新管线在两侧对齐后再择机退役
AVFoundation（含其 objc2 崩溃面）。

- ✅ 宿主页：WebGL 上下文加 `preserveDrawingBuffer: true`（工程笔记 #41 认可方案；
      否则 toDataURL 跨任务读到清空后的黑帧）
- ✅ Windows 采集窗口：常驻隐形窗口（TOPMOST + 1.2% alpha + TOOLWINDOW + NOACTIVATE，
      物理像素 800×450），顶层窗口恒不被遮挡、不受 explorer 重启影响——比 macOS
      的「壁纸层之上一档」更稳（那里靠层级逼近同一效果）
- ✅ 视频抽帧：采集 JS 用影子 `<video crossorigin=anonymous>`（协议响应已带
      `Access-Control-Allow-Origin: *`，规避 canvas 跨源污染），seek → cover-fit 绘制
      → toDataURL；采样时刻表复用 `frame_times`（8fps × 2s = 16 帧契约不变）
- ✅ shader 定格：`__gessoSeek(t)` + settle 后 `#gl.toDataURL`（依赖 preserveDrawingBuffer）
- ✅ html 实时帧：PrintWindow(PW_RENDERFULLCONTENT) 截采集窗口（兜底 BitBlt 屏幕区），
      唯一的平台接缝，独立成 capture_win.rs；实机 800×450 16 帧全出
- ✅ 落盘：dataURL → base64 解码（encoding.rs 新增解码器）→ `.tmp` → rename 原子写，
      命名沿用 `thumb.png + thumb-N.png` 契约，UI 零改动
- ✅ 调度：`dispatch_thumb` 统一分派——Windows 视频走主线程串行采集队列
      （CAPTURE_QUEUE / ThumbScheduler 原样复用）；macOS 分派不变
- ✅ 协议 Range 改惰性切片 + 开放范围 512KB 部分响应（大视频逐 seek 全量过盘的
      顺带修复：394MB 条目就绪超时即此根因，壁纸播放同步受益）
- ⚠️ 实测坑：ExecuteScript 字符串结果 JSON 编码带引号、布尔裸值——统一去引号
      （见工程笔记 §2）
- ✅ 实机验收：五条目全 16 帧（testsrc / plasma / clock + 两条 212/394MB 用户视频），
      帧内容视觉核验非黑帧；库页卡片/hover 读同一磁盘契约即生效

### M6 — Wallpaper Engine import I（✅ 已完成）
- ✅ 工坊内容解析：用户**自己通过导入对话框选中** WE 条目的 `project.json`（或整目录），Gesso 解析并使用——**绝不扫描磁盘、不枚举 Steam/工坊、不依赖 Steam 安装**。法律边界：只读用户主动指定的内容，不下载/不爬取/不再分发。
- ✅ WE web 垫片：`wallpaperRegisterAudioListener`/媒体等空实现 + 暂停/fps/鼠标/时间桥，注入点紧贴 `<head>`。- ✅ 普通导入可选 `project.json` 触发整目录 WE 导入；法律边界：只读本机已订阅内容，不下载/不爬取/不再分发。
- ✅ 静态图：支持 jpg/jpeg/png/avif，UI 新增独立「图片」分类（底层复用 Image）；图片缩略图 Direct 直引源文件，零生成零拷贝。
- ✅ 架构还债（本次两个 bug 的结构性根因）：① `gesso-core::content` 内容类型描述表——类型↔扩展名↔MIME↔缩略图策略的单一事实源；② `HostCommand` 枚举类型化 Rust↔宿主页命令；③ main.rs 上帝循环拆解（`apply_engine_action`/`merge_snapshot`/命名定时）；④ session.rs 纯工具抽到 `encoding.rs`。
- scene 类型（M7）暂拒绝；`depkg` 为 GPL，倾向子进程隔离，见 [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md)。
- ✅ **`gesso://` 协议回归**（`bf3c294`）：lb-wry ≥0.53 自定义协议在 macOS 已正常（M2 “零回调”结论失效）。宿主页改为 `gesso://host/index.html` 统一副本，不再拷入条目目录；`ContentSpec.source` 为绝对 `gesso://library/<entry>/…`；实现视频 Range(206)。四类实机验证通过。

## Known gaps (no milestone yet)

- WE 零拷贝直引：video/web 不拷入库、不修改原文件。用户主动导入 `project.json` 后，经 `gesso://steam/<entry-id>/<rel>` 只读访问该条目的 source_dir（以库清单里的不可猜 entry id 为凭据，不扫描磁盘）。
- Real-machine monitor outline (signature #1) and tray panel positioning on retina displays.
- ~~HTML thumbnails use the gradient placeholder~~ → Done (`372b4c4`): html captures live frames via the same persistent-window capture pipeline as shader/video.
- HTML wallpapers: iframe-internal navigation is not allow-listed yet (top-navigation / popups / forms are sandbox-blocked; `fps_cap` is advisory for the html kind). Hover previews use a two-phase cycle — preload all 16 frames behind a spinner, then play at a fixed 125 ms/frame (`c6bd3ce`).
- i18n: UI strings are centralized in `ui/strings.rs` (Chinese-first); English translation pass pending.
- Tray polish: dynamic menu copy (muda handlers are `Send`-only — menu handle can't be mutated from the poller) and precise quick-panel positioning relative to the tray icon (retina coordinate conversion).
