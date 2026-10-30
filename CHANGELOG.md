# Changelog

All notable changes to Gesso are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: [SemVer](https://semver.org/spec/v2.0.0.html) once 0.1 ships.

## [Unreleased]

### Added

- 更新器根据 App 当前语言显示双语发布说明，并兼容旧版静态清单格式。

### Changed

- README（中英）改版为产品门面：顶部应用图标、新增「下载安装」一节与内置壁纸清单；图片统一放在 `docs/assets/`，发布清单同步新增截图维护步骤。
- 发布清单保留旧客户端使用的 `notes`，同时增加 `notes_localized.zh/en` 供新版本按语言选择。

### Fixed

- **Windows 系统代理下更新检查恒失败（Proxy failed to connect）**：从注册表读取系统代理时，`RegGetValueW` 返回的字节计数含结尾 NUL 终止符，截出的代理 URI 带着它传给了 HTTP 库——带 NUL 的 CONNECT 隧道建不起来，更新检查在配置了系统代理的 Windows 上**从未成功过**（0.1.2 起一直如此），且重试无效（代理地址被进程内缓存）。现解析前剥掉 NUL 并加回归测试；经系统代理实测检查成功（远端 v0.1.3 → 「已是最新」）。

## [0.1.3] - 2026-10-15

### Added

- 更新检查只读取 GitHub Release 里的 `latest.json` 静态清单，不再访问未认证 GitHub API；清单不可用时显示明确失败原因并提供发布页入口。

### Changed

- **桌面宠物 macOS P0 基础链路（`feat/desktop-pet`）**：完成 v1.0 产品/领养规格与原型；新增 `gesso-core::pet` 纯数据确定性生成器（root seed、命名空间子 seed、三候选去重、版本输入、临时候选身份）、`gesso-core::adoption` 领养状态机和 `gesso-core::pet_save` 正式个体/存档纯数据模型；macOS app 已接入 `pet_store` 安全加载、损坏备份、版本保护和领养确认写盘骨架；正式 ID 使用 128 位随机值；已接入托盘“开始领养”和最小领养小窗（物种选择、三候选程序化占位外观预览、reroll、900ms 单任务冷却、确认保存状态、存档错误提示和“重试保存”按钮）；已接入版本化缓存元数据事务（缓存目录当前只写入 `pet.json`）、独立 macOS AppKit 占位宠物窗口（主屏右下角显示）、窗口拖动后的位置识别与存档、托盘显示/隐藏和全屏临时隐藏；正式视觉资源、互动动画、Windows 宠物窗口和跨平台回归暂缓；macOS 占位窗口已支持身体矩形命中、透明区域穿透、6pt 拖动阈值、300ms 连点摸和单击戳判定；互动统计经引擎动作更新并约 1s 节流写盘；宠物与壁纸平级、产品上独立但由 Gesso 承载。详见 `docs/design/桌面宠物设计.md` v1.0。

### Fixed

- **Windows 视频壁纸全黑（0.1.2 quad 管线三处叠加缺陷）**：0.1.2 引入的全屏 quad 管线在本机实测一个像素都画不出来——Draw 前未设图元拓扑（D3D11 默认 UNDEFINED，Draw 静默不画）、着色器用不绑顶点缓冲的 SV_VertexID 生成顶点（部分 AMD 驱动同样静默吞掉该 Draw）、fit 常量缓冲从未 `PSSetConstantBuffers` 绑到像素着色器（读到全零常量，缩放除零全屏落「越界黑」）。三处失败都不报错、Present 照常成功、解码正常循环，日志全绿。现改为顶点缓冲全屏三角（uv 随顶点走、画面方向随之转正）、显式设拓扑与 CullNone、绑定常量缓冲，实机验证视频正常播放、填充三态数学不变。
- **Windows 桌面壳层被挂载流程搞乱：孤儿 WorkerW 堆积 + 退出后壁纸不回来**。两处根因一起修：其一，`find_wallpaper_parent` 每次挂载都无条件向 Progman 发一次 0x052C，Explorer 每次都新生成一个壁纸 WorkerW——一晚上实测堆积 14+ 个隐藏孤儿，还会触发桌面结构重组、把已挂载的壁纸窗孤儿化。现改为先找现成的壁纸 WorkerW，找不到才生成一次（三轮启动退出实测 WorkerW 数量零新增）。其二，部分驱动 + DWM 组合下（WindowDev/RX580 实测），跨进程子窗从壁纸 WorkerW 移除后 DWM 不重新合成该子树，屏幕残留已销毁窗口的最后一帧或黑底，失效重绘唤不醒——连普通 GDI 子窗都如此，非 Gesso 特有。退出清理在失效重绘之后追加「同文件重设一次系统壁纸」，让 Explorer 重载重绘；实测优雅退出（含 panic 清场路径）桌面完整恢复。
- 更新失败态改为浅红提示面板，复制错误详情改为低存在感的行内操作并用图标反馈；macOS 改为菜单栏常驻应用，不出现在 Cmd+Tab，托盘首次点击管理窗口或更新弹窗即可激活。

## [0.1.2] - 2026-10-13

### Added

- **更新检查与下载遵循系统代理**：macOS 读 `scutil --proxy`（HTTPS > HTTP > SOCKS），Windows 读 Internet Settings 注册表；环境变量代理优先级更高；PAC 不解析（回落直连）。此前 GUI 进程不继承 shell 代理变量、HTTP 客户端又不读系统代理——国内网络下浏览器能下载而应用内超时（os error 60）。另：下载连接加 30 秒读超时（停滞不再无限挂起），失败文案补「前往发布页手动下载」出路。

- **填充模式（§4.4 填充下拉）**：显示器页详情条新增填充下拉——填充（cover，默认）/ 适应（contain）/ 拉伸（fill），按屏持久化、切换即时生效。此前壁纸本体硬编码 contain（比例不合留黑边），且 Windows 原生视频 swapchain 按帧尺寸建、整缓冲交 DWM 拉伸（比例不合变形）——双平台语义不一致。现默认与 WE / 系统桌面习惯对齐（裁切铺满）：macOS 视频窗映射 AVLayerVideoGravity 三态活切换；Windows 视频渲染引入全屏 quad 管线（swapchain 改窗口客户区尺寸 + UV 常量采样，几何/填充变更逐圈追赶，Present(1)/PTS 节拍不变）；图片经 spec.fit 直达宿主页 object-fit；shader/html 由内容作者控制画布语义，下拉置灰。采集管线恒按适应全帧语义钉死，缩略图契约不变。

### Changed

### Fixed

- **双击/随机换壁纸 UI 冻结，桌面桥接重构为自家过渡窗（三轮排障定案）**：根因是运行时把系统桌面壁纸写进换图循环——macOS 壁纸代理 FIFO 每张 5–16 秒、拥塞时 `setDesktopImageURL` 单次实测 2 秒，高频指派把代理喂堵后，所有同步等它的调用（设桌面图、建窗落位、WebView 握手）统一付出 ~2 秒，UI 冻结、通知卡顿，且静帧永远来不及在 1–2 秒的建窗间隙内渲染。终局：运行时**永不写系统桌面**。显示器重配的无缝过渡改由 Gesso 自己的过渡窗承担——清扫几何过时屏前先上预解码静帧窗（毫秒级），真壁纸窗点亮后撤；静帧后台预生成/预解码，主线程永不解码视频。连带效果：右键移除 /「恢复系统壁纸」/ 退出的所有等待全部消失、瞬时完成；设置页「桌面桥接」开关移除；0.1.x 旧版把桌面设成桥接帧的残留，由启动一次性清理。旧「恢复系统壁纸」相关的静帧滞留、黑屏、信任窗口等缺陷随接管路径一并消失（运行时不再写桌面，无恢复动作）。已知取舍：图片类条目（非 PNG 源）暂无解码器，其重配间隙退回露出用户壁纸（v1）。
- **更新下载失败后「重试」无反应**：下载线程的三条结束路径（完成/取消/失败）都不释放检查/下载互斥标志，此后所有检查与下载请求被「进行中」挡死直至重启。同时修复更新小窗在失败态关闭后重开仍显示旧错误（关闭即重置失败态）。
- **「随机换一张」不是随机**：实现实为按库顺序循环（+1），与托盘文案不符。现改为真随机（排除当前条目，std RandomState 播种，无新增依赖）。
- **「随机换一张」在恢复系统壁纸之后没有反应**：恢复会释放全部壁纸会话，随机换一张取不到会话即静默空转。现无会话时回退到主显示器重新随机指派一张。
- **显示器页细节（0.1.1 反馈）**：「重新检测」按钮改 Small 档；详情条行距与控件疏密调整；下拉长文本不再截断。
- **沙盘小像 hover/选中描边串扰**：悬停 A 屏小像会连带点亮 B 屏的选中描边——hover 态此前误用全局选中索引，改为视图本地按屏索引。

## [0.1.1] - 2026-10-12

### Fixed

- **桌面宠物 macOS 占位窗口命中修复**：透明区域不再拦截后方应用；自定义 `NSView::hitTest:` 仅让带 padding 的身体占位矩形接收鼠标事件，正式 alpha 轮廓命中和互动动画仍待后续接入。
- **多屏唤醒/重配改为事件驱动两段式收敛（epoch 重构，v3 定稿）**：显示器重配（唤醒/解锁/插拔/改分辨率/主屏切换/合盖开盖）由 CG 重配回调（macOS）/ `WM_DISPLAYCHANGE`（Windows）翻转纪元代数触发，引擎心跳去抖（1.5s 静默期，4s 陈旧期上限）后收敛，替代 2s 轮询发现。30ms 快速观察器先熄灭与现实脱节的窗口，跨采样稳定后再以可见原子切换施加几何；macOS 已完成多轮实机验证，Windows 尚未完成多屏实机验证。
- **Windows 安装版启动闪退（#2）**：0.1.0 的安装包在 Windows 上 100% 无法启动，黑窗一闪而过。单实例锁把文件路径字符串当内核对象名传给 `CreateMutexW`，路径里的 `\` 不被接受。锁名改为扁平的 `Local\gesso-app-lock`。
- **视频壁纸卡顿（macOS / Windows 双平台）**：视频此前在 webview 里播放，浏览器把视频帧跨进程搬上屏的路径没有匀速保证（macOS 实测 60fps 内容只上屏 53~56fps；Windows 的呈现交接层同样的问题），高帧率素材和多屏下肉眼可见地卡。两平台都改为原生播放器直挂壁纸窗口（macOS 用 AVPlayerLayer，Windows 用 Media Foundation 解码 + DXGI swapchain 呈现），webview 只保留给 shader / 网页 / 图片。重写前做了三代构建 × 两种传输 × 三类素材的真机对照，确认问题在呈现层而不是数据供给。
- **循环视频持续读磁盘**：循环播放每圈重读整个文件，内存紧张的机器上页缓存被换出，磁盘一直忙。现在不超过 256MB 的视频首次读入后常驻内存，循环和 seek 都不再触盘；更大的文件仍走磁盘流送，不硬吃内存。
- **锁屏/合盖唤醒后副屏丢壁纸、残留幽灵窗口**：唤醒会重编显示器 ID，旧的兜底逻辑只保得住一块屏的指派。现在按帧几何把会话和配置一起平移到新 ID；被真拆除的会话先隐藏再丢弃，不再残留不可点击的空窗口。
- **退出或崩溃后桌面黑屏**：托盘退出改为正常拆除流程（逐窗销毁、通知 explorer 重绘）；panic 时钩子也会尽力清场。Windows 崩溃原因落在 `%USERPROFILE%\.gesso\gesso.log`。
- **F11 / 切换桌面时的绿屏闪烁与误暂停**（macOS）：移除宿主页的调试底色，全屏判定加上前台应用匹配，不再把桌面切换的过渡窗口当成全屏应用。
- **竖版 / 超宽素材被裁成中间一条**：库卡片和壁纸本体改用 contain 语义，完整显示不再裁切。
- **记事本编辑过的配置被重置**：`config.json` / `library.json` 带 BOM 时解析失败并静默回退默认值，显示器指派被清空。加载时剥离 BOM。
- **库清单损坏时被空库覆盖**：`library.json` 解析失败不再用空清单覆写用户的库，保留原文件并以退出码 78 结束，留待手工修复。
- 「从 URL 导入」弹窗打开即聚焦输入框，出现后可直接粘贴。
- macOS 视频壁纸边缘约 1px 的淡色边框：AVPlayerLayer 非整数缩放的采样劣化带，窗口四周外扩 2pt 裁出可视区。
- NSIS 安装脚本补 UTF-8 BOM：中文 Windows（ACP 936）上此前直接编译失败；英文 CI 只会乱码注释不报错，所以一直没暴露。

### Added

- **桌面桥接（重配过渡无缝化，默认开启）**：Gesso 在屏期间把系统桌面壁纸接管为当前壁纸的静态帧，显示器重配时由静态帧兜底；退出、取消指派、关闭设置开关或使用托盘逃生门时恢复原壁纸。台账按条目保存，用户手动更换壁纸时不覆盖用户选择，硬杀残留由下次启动对账。
- **应用自更新（§4.7 软件更新）**：启动后与每 24 小时静默检查 GitHub Releases，设置页和托盘支持手动检查，提供检查、发现、下载、完成、失败等状态和多层通知；更新下载官方安装包后交由系统安装，不做静默自替换。debug 构建提供 `GESSO_UPDATE_MODAL=1` 与 `GESSO_UPDATE_AUTO_DL=1` 验证钩子。
- 帧率诊断开关 `GESSO_VIDEO_DIAG=1`（双平台）：每 5 秒打印视频上屏帧数、页面渲染节拍和丢帧数，播放类问题先量化再动手。

### Changed

- **网页 / HTML 壁纸真正静音**：`audio=muted` 此前只对视频生效，网页的声音直通系统混音器。Windows 在引擎层关断音频输出；macOS 落回 WKWebView「音频需要手势」的默认行为。
- **锁屏自动暂停**：锁屏即暂停全部壁纸、解锁恢复，不走用户策略（锁屏是系统级安全上下文，壁纸窗口本来就不参与渲染）。
- shader 画布按 devicePixelRatio 渲染：125%+ 缩放屏上此前实际渲染分辨率被压低，星点、粒子类内容发糊。
- 预置壁纸扩充到 10 款（新增 Tide / Starfield / Silk / Ember 四款 shader）；预置内容随版本升级到达老用户（此前只在首次播种时落盘，改版内容永远到不了）；条目和界面文案去掉 demo 措辞。
- 缩略图启动即开始生成（此前要等 30 秒），生成中显示 spinner，重试耗尽退回类型图标，不假装在加载。
- 关闭管理窗口改为隐藏进托盘（回归设计规格）；release 构建改 GUI 子系统，双击不再弹终端黑窗。
- 移除「查看详情」菜单项：它只是把卡片上已有的三条信息再弹一次 toast。

## [0.1.0] - 2026-10-07

### Added

**Engine**

- Dual-crate workspace: `gesso-core` (pure domain logic, 14 unit tests) + `gesso-app`.
- macOS wallpaper pinning: native `NSWindow` at `level = -2147483604`, click-through, all-spaces, transparent fallback to the system desktop (verified by a dedicated spike; GPUI windows are *not* used for wallpaper — see ARCHITECTURE).
- Wallpaper host pipeline: self-contained `file://` entries (host page copied per entry, relative media paths), `ContentSpec` query contract, `__gesso.pause/resume` JS API.
- Session manager: per-monitor sessions driven by the core state machine (`Idle→Loading→Playing→Paused…`), monitor diff sync, config + library persistence, single-instance lock.
- Cursor feed (M5): global mouse location routed to the display it is on → shader `iMouse` (30 Hz push, per-frame smoothing) and html `postMessage`; idle 5 min auto-downscale to 5 fps. No Input Monitoring permission needed — position via `NSEvent::mouseLocation`, buttons/idle via CoreGraphics HID source state (polling, not an event tap).
- Built-in `Cursor Glow` shader sample demonstrating mouse tracking.
- Import flow: extension validation (`mp4/webm/gif/webp/glsl/html`; `mkv`/HEVC rejected with actionable copy), source-extension preservation, random unguessable entry IDs.
- Windows installer: per-user `setup.exe` (NSIS 3 — same installer form as our Tauri projects, no admin) alongside the portable zip — Start-menu + desktop shortcuts, uninstaller with an "Apps & features" entry, installs to `%LOCALAPPDATA%\Gesso`; user data in `~\.gesso` survives uninstall. The release job tolerates a missing installer (zip/dmg always ship).
- Tray: full menu (pause-all / cycle / manager / autostart / quit).
- First-run wizard wired to the real library; autostart via `auto-launch`.
- Thumbnail pipeline: AVFoundation frame extraction (generated objc2 bindings + ImageIO PNG encode, background `autoreleasepool`), `ThumbScheduler` (30s scan, in-flight dedup, 3 attempts/entry), import schedules extraction asynchronously.
- Hover preview on library cards: paused still-frame by default, extracted-frame carousel on hover.
- Shader renderer (M4 part 1): WebGL2 host branch with Shadertoy-subset uniforms (`iTime/iResolution/iMouse/iTimeDelta/iFrame` + `iChannel0..3` bound to runtime-generated 256×256 noise textures), `code=` base64url source param（file:// 下 fetch/XHR 被拦）, Shadertoy `mainImage` auto-wrap, RAF-timestamp fps gating, red-screen diagnostics on compile/link failure; 3 built-in samples (plasma / aurora / noiseflow) with static + hover-carousel thumbnails.
- HTML renderer (M4 part 2): the wallpaper's HTML (fixed at `wallpaper.html` — `index.html` stays the host page) loads in a sandboxed iframe (`sandbox="allow-scripts"` only, so the opaque origin rejects storage/IPC — hardware-verified DoD: `localStorage`/`sessionStorage`/`document.cookie`/`IndexedDB`/`window.top` all throw `SecurityError`, XHR is a `NetworkError`); pause/resume reach the frame via `postMessage {__gesso:"pause"|"resume"}`; built-in Clock sample as the contract reference.
- System data bridge (M5): fullscreen detection (`CGWindowList` layer-0 window covering a display frame, CG top-left ↔ AppKit bottom-left y-flip via primary display height) and battery state (IOKit `IOPSCopyPowerSourcesList`) drive the `Autopause` state machine per settings policy — pause / downscale-to-5fps (live `__gesso.setFps`: shader immediate, html advisory, video keeps playing) / ignore; user pause wins over auto-pause; hardware-verified (fullscreening the manager window pauses, exiting releases). Time feed: engine drives `__gesso.tick` at ~1 Hz so clock wallpapers tick from Rust events.
- Wallpaper Engine support (M6): user-driven import only — the user explicitly selects a WE `project.json` (or folder) via the import dialog; Gesso never scans the disk, enumerates Steam/Workshop, or requires Steam to be installed. Supported kinds `video` and `web`, imported as **zero-copy read-only references** to the user-selected source (`gesso://steam/<entry-id>/…` route: credential is the unguessable library entry id mapped to that entry's `source_dir`; video streamed via Range, main HTML injected with the WE API shim in-memory at the protocol layer — original files untouched, relative subresources same-origin; unknown id → 404). Project main file stored in `LibraryEntry.main_file`. Legal boundary: reads only content the user actively points to — no downloader, scraping, enumeration, redistribution, or on-disk copy. `scene` rejected (M7), `application` never.
- Windows system data bridge (M5-W): `bridge/windows.rs` completes the M5 feature set on Windows — cursor feed (GetCursorPos + GetAsyncKeyState + GetLastInputInfo; shader `iMouse` follows at 30 Hz with the bottom-left contract flip done once at the poll boundary), fullscreen autopause (EnumWindows finds top-level windows whose rect exactly covers a monitor frame, excluding our own wallpaper/capture class, shell windows Progman/WorkerW, toolwindows and DWM-cloaked ghost windows), battery state (GetSystemPowerStatus, no-battery desktops report None), and 5-minute idle downscale. Pure polling, zero permissions. Hardware-verified: glow tracks SetCursorPos, real fullscreen pauses and resume-on-exit, idle downscale fires and clears. Also fixed an enum-state type-punning UB (reference and data tuple-cast together) found while wiring the detection.
- Windows titlebar controls: the 44px top bar now ships the spec'd `– □ ✕` caption buttons (spec §2/§4.3 debt — macOS traffic lights were system-provided, Windows had nothing). Buttons call window methods (`minimize_window`/`zoom_window`/`remove_window`) to keep gpui hover/press visuals; window dragging, double-click maximize and edge snap come from `WindowControlArea::Drag` declared on the non-interactive bar regions (brand area + flexible spacer), handled by the platform hit-test (`HTCAPTION`). macOS path untouched.
- Windows app & tray icons (fix): the exe now embeds `win/gesso.ico` as resource ID 1 via a `winresource` build script (taskbar and GPUI window-class icon previously degraded silently to a generic blank icon — gpui-pre-windows does `LoadImageW` with no error surfaced); the tray icon uses the full-bleed colored compact app icon `tray-app-32.png` instead of an outlined white glyph that read as "transparent" at 16px on light taskbars; icon artwork cropped to full-bleed for Windows (macOS keeps Big Sur margins). Design spec (DESIGN §品牌图形) and the icon regen pipeline updated alongside.
- Windows thumbnails (M4-W): the webview-native frame pipeline — shadow `<video crossorigin=anonymous>` + canvas seek/draw/toDataURL for video (protocol responses already carry `Access-Control-Allow-Origin: *`, so no canvas taint), `preserveDrawingBuffer` + `#gl.toDataURL` for shader freeze-frames, and PrintWindow(PW_RENDERFULLCONTENT) screen capture for live html pages (the single platform seam). Persistent invisible TOPMOST capture window (1.2% alpha, toolwindow, never occluded → WebView2 renders at full speed). Output contract identical to macOS (`thumb.png` + `thumb-1..15.png`, atomic temp-rename), so library cards and hover previews work unchanged. The protocol Range path now serves lazy slices and answers open-ended ranges with a 512KB 206 partial response — multi-hundred-MB videos no longer pay a full-file read per seek (this was the capture readiness timeout root cause). Verified on hardware: all five entries (3 built-in samples + 212/394 MB user videos) produce complete 16-frame sets with correct content.
- Windows wallpaper pinning (M1): own Win32 window + WebView2 child mounted under the desktop icon layer via the WorkerW ladder (Progman `0x052C` spawn → sibling WorkerW → `SetParent`; Progman fallback when icons are hidden; top-level bottom-most until explorer is ready). Mount-first creation order (WebView2's DComp tree binds at controller creation — reverse order renders nothing), `WM_NCHITTEST` click-through, `PerMonitorV2` declared at process start (physical pixels; neither GPUI nor wry sets DPI awareness), `GPUI_DISABLE_DIRECT_COMPOSITION=1`. Explorer-restart self-healing: a hidden listener window receives `TaskbarCreated`, the engine rebuilds each session's window wholesale (cross-process children die with explorer's WorkerW). `gesso://` serving on WebView2 via wry's `http://gesso.<host>/` workaround: page-visible URLs and CSP produced in workaround form engine-side, routing stays platform-agnostic. Verified on hardware: shader/html/video (Range 206 streaming) render below desktop icons at 125% DPI scaling; explorer restart re-pins automatically.

**Polish (post-M6)**

- English i18n: full 简体中文 / English text packs with an Auto option (follows `LC_ALL`/`LANG`/`LANGUAGE`); a Language selector in Settings ▸ Startup persists the choice and applies live (no restart). Default stays Chinese; startup applies the saved language before the UI is built. Covered by a test asserting both packs have no empty fields. Also fixed a latent macOS build break from the Windows titlebar commit (a `cfg!(target_os = "windows")` runtime guard referenced the Windows-only `caption_button`; switched the call site to a compile-time `#[cfg]`).
- Web-wallpaper compatibility: the sandbox iframe now includes `allow-same-origin` (the opaque origin suppressed same-scheme external `.js`/`.css` requests entirely — most WE web wallpapers split their code out), and CSP `connect-src`/`frame` allow same-scheme loads so `fetch()` of local JSON/config works. Verified with a kitchen-sink fixture: WE API shim, canvas2D+RAF, WebGL, AudioContext, localStorage, relative images, setInterval and same-origin fetch all pass. External navigation/window popups remain blocked (no `allow-top-navigation`/`allow-popups`); only `gesso`-scheme frames may embed, keeping wallpapers self-contained and offline. Also cleared the last two warnings (`m15` example, dead-code annotations for Windows-only helpers).
- Static image support: `jpg/jpeg/png/avif` added, new UI "Photo" category (still core `Image`; animated gif/webp distinguished via `is_animated_image_ext`).
- Remote web wallpapers: import any `https://` page straight from the library toolbar (🔗 dialog) — no local copy; the URL loads directly in the host page's sandboxed iframe (`LibraryEntry.source_url` records the origin, live-capture thumbnails for remote entries, "open folder" disabled for them in the card context menu).
- Content-type single source of truth (`gesso_core::content`): kind ↔ extensions ↔ MIME ↔ thumbnail strategy declared once in a `ContentType` table; import classification, main-asset discovery, protocol MIME and thumbnail dispatch all read it. `HostCommand` enum types the Rust→host JS commands (single `to_js` serialization, `WallpaperWindow::send`).

**UI (manager window, gpui-kit)**

- `gesso://` custom resource protocol restored to active (`bf3c294`): host page served from a single shared `gesso://host/index.html`, entry assets from `gesso://library/<id>/…`, cross-origin CORS, and HTTP Range(206) for video. Verified on all four kinds (image/video/html/shader).

- Library page (filter segments, search, cards, status bar, drag-to-monitor assignment overlay), monitors page (desktop-sandbox canvas + detail strip, per-monitor controls), settings page (persisted, applied live).
- Unified engine bridge: `AppState` global + `EngineAction` queue; snapshots preserve UI-local state.

**Project / docs**

- Feasibility spikes with verified conclusions (`SPIKE-REPORT.md`): GPUI integration, macOS desktop layering, Xcode-free builds via `runtime_shaders`.
- Engine API contract (`crates/app/API.md`), engineering notes (architecture rules + pitfall ledger), architecture doc, roadmap, security policy, contribution guide, dual MIT/Apache-2.0 licensing.
- Product icon set ("gesso ground" concept, DESIGN.md §品牌图形): `mac/Gesso.icns` (16→1024), `win/gesso.ico` (16→256), tray glyphs (macOS template @2x / Windows white); tray placeholder pixels in `main.rs` replaced by embedded PNGs (template-aware on macOS), runtime Dock icon via `NSImage`; regen pipeline `crates/app/assets/icons/tools/build.mjs` + brand spec sheet `docs/design/06-品牌图标.brand.html`.

### Changed

- Import entry consolidated: the library toolbar has a single **Import ▾** menu button (Import file… / Import web page from URL…); the standalone link-icon button is gone. Drag-and-drop import is unchanged.
- Control scale rule (DESIGN.md §组件): form-row and display-detail controls use kit **Small** (24px / 14px text — settings selects & buttons & the weather-key input, monitor detail bar incl. the FPS dropdown, the URL-import dialog's input and buttons, text actions like "clear search"); toolbar and canvas primary CTAs stay **Medium** (32px / 16px; the monitor toolbar's Redetect dropped its leftover `compact()` to match the library toolbar). Custom `Size::px` sizes are banned (kit falls back to 16px text for non-tier sizes). Replaces the old "button height 28" contract — gpui-kit has no 28px tier.
- Entry removal now moves the entry's directory to the system Trash/Recycle Bin (recoverable) for `local`/`builtin` copies (the in-library copy is Gesso-owned) and the thumbnail home of `url` entries; Wallpaper Engine references (user's Steam folder) are never touched, and if the Trash is unavailable (e.g. a network volume) the directory is left in place — never permanently deleted. Removal runs only after the manifest save succeeds and is guarded by a canonicalize check that refuses paths outside the library root (also fixed: the 2026-10-05-era orphan entry directories this left behind). Toast copy no longer promises "files kept" (spec §7 updated).
- Library empty states split into three (spec §4.3): empty library → first-run state; non-empty query with no hits → "no match" + clear-search; **filter with no items and no query** (e.g. the WE tab on a library without WE imports) → new dedicated state "No {label} wallpapers yet" with an import hint for WE and a "View all wallpapers" button — previously this showed the search empty state with blank quotes (`No wallpapers match ""`).
- First-run detection flag renamed `wizard_seen` → `initialized` (semantics: one-time built-in sample assignment on first launch, no wizard involved). Legacy configs without the key load as uninitialized and get the sample assignment once on next start (zero-migration, covered by a test).
- macOS titlebar: content left inset 76 → 88px — the traffic-light cluster ends at x=66, so the brand icon sat 10px from the green light; now 22px, matching Apple's 44px toolbar rhythm.
- Protocol request logging (`gesso://` GET / 206 Range lines) is silent by default — video wallpapers fire Range requests on every buffer/seek, which flooded startup logs. Enable with `GESSO_PROTOCOL_LOG=1` for protocol debugging.

### Removed

- **First-run wizard** (superseded decision — spec §4.6, 2026-10-07): the three-step wizard window, its replay button (topbar, later Settings ▸ Advanced), and all wizard strings are gone. First launch now just assigns the built-in sample to the main display once (`config.initialized`); users add content via drag-and-drop or the Import menu. Upgrade path: existing configs keep their assignments; a fresh install (or a config without `initialized`) gets the one-time sample assignment.

### Fixed

- Windows taskbar showed the generic "blue title bar" icon: GPUI (`gpui-pre-windows`) loads the window-class icon once at startup via `LoadImageW(module, MAKEINTRESOURCE(1), IMAGE_ICON)` and silently degrades to a null icon when the resource is missing — and `gesso-app` had no `build.rs`, so the exe carried no icon resource at all. Added `build.rs` (winresource) embedding `assets/icons/win/gesso.ico` as **resource ID 1**, gated on the target OS so macOS builds are untouched; the same resource also covers Explorer's exe file icon.
- Windows tray icon was effectively invisible (user-reported 2026-10-06): the white glyph was chosen under the "Windows taskbars are always dark" assumption, and even a recolored monochrome glyph drowns at tray size — an outline glyph scaled to 16px is just a faint contour. Windows now embeds `tray-app-32.png`, the colored compact app icon (same artwork as the taskbar/exe icon); the monochrome template mechanism stays macOS-only per user decision, and DESIGN §品牌图形 + the brand preview HTML were amended accordingly (`build.mjs` emits the new asset).
- Windows taskbar/tray icons rendered visibly smaller than other apps' (user-reported 2026-10-06): the Big Sur-style transparent margin (100/1024 per side) is a macOS Dock-grid convention, but Windows expects edge-to-edge icons. `gesso.ico` and the tray PNG now use a full-bleed variant (viewBox cropped to the 824 design area, compact design kept for ≤32px); the macOS icns and src masters keep their padding. `build.mjs` also gained a platform guard so `iconutil` (macOS-only) doesn't abort the pipeline on Windows/CI.
- Random SIGSEGV in `objc_release` during GCD autorelease-pool pop (two crashes with identical fingerprint, 2026-10-03/04): the thumbnail extractor hand-rolled ObjC refcounting on background tasks and violated ownership twice — an alloc+init object claimed by two `Retained::from_raw`, and the autoreleased `representationUsingType:properties:` result treated as +1 (over-released, its multi-MB VM region unmapped; the pool's stale record then faulted on release). Rewrote `thumb.rs`: generated objc2 bindings for AVFoundation (ownership in the type system), ImageIO `CGImageDestination` for PNG (pure C, no autoreleases), the whole job wrapped in an `autoreleasepool`, atomic per-frame writes (`.tmp` + rename), and a regression test running the real AVFoundation pipeline.
- Thumbnail scheduling hardening: missing-frame entries were retried forever every 30s (a persistently failing entry became an unbounded FFI churn and crash amplifier) and a `timer(2s)` guess refreshed the UI; extraction also ran synchronously on the main thread during import (~1s freeze per video). Now a dedicated 30s scan drives jobs through `engine::ThumbScheduler` (in-flight dedup + 3 attempts per entry per session), import schedules extraction asynchronously, and completion (`ThumbsDone`) triggers the snapshot refresh.
- Custom-scheme protocol restored: the M2 "zero callbacks / initial-navigation poison" conclusion no longer holds on lb-wry ≥0.53; the wallpaper webview now boots straight into a `gesso://host` URL (an `about:blank` first frame is no longer needed) and subresources resolve through the same handler.
- Static-image entry misidentified its host page as the main asset: an image directory contains both `index.html` (the host page, rewritten every launch) and `index.png`; `read_dir` could yield html first, and the old "any `index.*` fallback" in `main_asset_name` returned the host page → thumbnail/kind detection broke. Fixed at the root: `main_asset_name` only matches the kind's extension whitelist (`content_type(kind).extensions`), and html's main asset is fixed to `wallpaper.html`. The same table now backs import classification, validity and protocol MIME (previously six scattered extension lists).
- Rust↔host JS commands were hand-built strings (`__gesso&&__gesso.setFps(5)` across 8+ call sites, aligned by hand with the host page); replaced by the `HostCommand` enum with one serialization point and `WallpaperWindow::send`.
- Main loop decomposed (`apply_engine_action`/`merge_snapshot`/`focus_or_reopen_main`, named timing constants) and session stateless helpers moved to `encoding.rs`.
- Percent-encode library paths (`Application Support` contains a space).
- Import no longer renames `.webm`/`.webp` to `index.mp4`/`index.gif` (webviews type media by extension); import/validity/host-spec now share one `main_asset_name`.
- Tray "pause all" toggle used `AtomicBool::swap(true)` (only ever pauses) → `fetch_xor`.
- Tray "管理窗口…" click went dead after closing the window via the red dot (cached handle activation failed on a closed window) → rebuild the window automatically on activation failure.
- Settings rows: label and select were not vertically aligned — the kit Select's `size_full` root fills a fixed-height row while its trigger sits top-aligned inside; a `select_slot` wrapper (full-width, height collapsing to the trigger) restores row-level centering.
- Monitors-page FPS dropdown never took effect: its subscription updated only the UI-local projection and never enqueued `SetMonitorFps` (the action existed and was handled but had zero constructors; snapshots also hardcoded `fps: 60`). Now the dropdown enqueues the engine action (hot-reloads that session's host page, persists `monitor_fps`) and snapshots report the real per-display fps.
- UI hover state (`hovered` / `hover_frame`) was wiped by every engine snapshot回灌 — card hover highlight and preview carousel flickered on each refresh cycle; the merge now preserves them like the other UI-local browsing state.
- Library page could not scroll: the scroll container's `flex_1`/`min_h_0` sat under a non-flex wrapper, so its height was content-driven and never overflowed; the wrapper is now a flex container.
- Library grid now matches the prototype contract (`repeat(auto-fill, minmax(196px, 1fr))`): column count adapts to the window width and cards divide each row evenly instead of a fixed 208px width leaving a large trailing gap.
- Shader and HTML thumbnail capture: a persistent on-screen window (one level above the wallpaper, 2% alpha) runs the real host page and snapshots it via WKWebView — shader is frozen per frame via `__gessoSeek(t)`, html is sampled as live frames; captures run on one global serial queue and the capture window is never closed while a snapshot is pending (was an over-release SIGSEGV).
- Hover preview flicker went through three root causes: (1) the dark base color was attached to an `image_cache` element, which only forwards children and never paints its own style — moved to an outer div; (2) the `img` had no element id, so GPUI created no `ImgState` and the loading fallback was skipped entirely — fixed shared id; (3) the frame index advanced on a fixed timer while frames were still decoding (shader/html frames are large), so the preview alternated black/image and played catch-up at variable speed — hover is now two-phase: preload all 16 frames behind a spinner (`fetch_asset`, shared with the display cache), then play at a fixed 125 ms/frame.
- Library page scroll and responsive grid fixes (flex-container height chain; viewport-driven column count replacing the unavailable CSS `auto-fill`).
- Monitors-page redesign: desktop sandbox layout with a per-display detail bar.
- Compiler warnings in `gesso-app` cleared (32 → 0): mechanical fixes via `cargo fix`, `NSWindow::new`/`subviews`/`NSScreen::screens`/`setFrame_display` unsafe blocks adjusted to current objc2 bindings, deprecated `Retained::cast` → `downcast`, `NSColor::clearColor` safe call, dead leftover strings and helpers removed, the gesso:// protocol scaffolding and platform-window surface annotated as intentional (`#[allow(dead_code)]` with pointers to the protocol-fix todo / Windows M1).

### Known limitations (tracked in ROADMAP)

- Windows multi-monitor pinning is implemented but not yet re-verified on a multi-display machine (single-display verification done).
- WE video/web use zero-copy read-only references to the source (`gesso://steam`); no on-disk copy is made. Older copied entries still resolve via `gesso://library`.

- HTML iframe external/top navigation, popups and forms stay blocked; only `gesso`-scheme sub-frames may embed (self-contained, offline).
- Hover preview preload is per-process cache: the first hover shows a spinner briefly; subsequent hovers play immediately.
