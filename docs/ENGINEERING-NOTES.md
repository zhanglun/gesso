# 工程笔记（Gesso）

> 常驻文档：**架构规则 + 当前平台纪律 + 高风险调试路径**。设计文档在 [design/](design/)（随仓库版本化，后续迭代以本目录为准），引擎接口在 `crates/app/API.md`。
> 新规则应直接补入对应小节；历史事故不在这里展开。

---

## 1. 架构规则（改前必读）

1. **壁纸窗口不是 GPUI 窗口**：`pin/` 使用平台原生窗口和 `lb-wry` 直挂；GPUI 只管管理窗口与托盘。壁纸窗口的桌面级几何由 `pin/` 和会话收敛负责。
2. **UI 不碰引擎内部**：只经 `crates/app/API.md`。写操作入队 `engine::EngineAction`（引擎 150ms 轮询执行，与托盘同一通道）；读操作走 `snapshot_ui(&sm)` 快照，且**必须保留 UI 本地状态**（`active_tab/selected/query/filter/import_counter`），否则用户输入每 150ms 被冲掉。
3. **改界面先改规格与原型**（`docs/design/界面与交互设计.md` + `docs/design/prototype/index.html`），再改代码；token/组件/文案以 `docs/design/DESIGN.md` 为准。
4. **桌面宠物窗口独立于壁纸窗口**：`pet_window/` 用纯原生宠物面板（macOS: AppKit NSPanel；Windows: Win32 分层窗 `pet_window/windows.rs`，穿透纪律见 §2 P2-W 行），不进入 GPUI 或 `SessionManager`；窗口事件写操作仍必须经 `EngineAction` 进入引擎，不能由 view 直接写 `PetStore`。两平台均以模板声明的蜡身命中区做圆角矩形命中 + 命中区外整窗穿透（macOS `ignoresMouseEvents` / Windows `WS_EX_TRANSPARENT`）；正式资源接入后再替换为 alpha/轮廓命中。
5. **桌面宠物互动已接入 macOS 基础链路**：300ms 内连续点击为 `Touch`，单击延迟确认后为 `Poke`，超过 6pt 进入自定义 `setFrameOrigin` 拖动；互动统计经 `EngineAction` 更新并约 1s 节流保存。行为反馈事件也经 `EngineAction::PetBehavior` 驱动 `PetBehavior`，AppKit 占位层按状态提供戳/摸/拖拽/睡眠反馈，Watching 通过眼睛高光表达注视；macOS reduced-motion 时保持静态。退出前必须 flush dirty 存档；缓存启动恢复只删除版本目录下的 `.tmp-` 临时目录。不要用全局鼠标轮询替代窗口命中，也不要让每次互动同步写盘。
6. **提交前必须 `cargo check -p gesso-app` 通过**。UI 代码曾因从未编译积累 115 个错误。
7. **类型知识与主资源发现只在单一事实源**：类型↔扩展名↔MIME↔缩略图策略查 `gesso_core::content`；主资源文件名走 `encoding::main_asset_name`，导入/失效判定/宿主页 spec 共用。四处各写一份“类型→扩展名”映射，就会出现“能导入但被判失效”或“宿主页请求错文件名”。
8. **平台 FFI 的所有权不裸写**。ObjC 一律走 objc2 生成绑定（所有权编码在类型里：init/copy 家族 → `Retained<T>`，autoreleased 返回值由绑定内部 `objc_retainAutoreleasedReturnValue` 处理）；手写 extern 只留给纯 C API（CF 的 +1/CFRelease 对称即可，如 ImageIO `CGImageDestination`）。后台线程做 ObjC/AVFoundation 整段包 `objc2::rc::autoreleasepool`，并且只跑在有重试上限 + 在途去重的调度器后面（`thumb.rs` + `engine::ThumbScheduler` 是范本）。

## 2. 当前平台纪律

| 范围 | 必须遵守的规则 |
|---|---|
## 2. 当前平台纪律

| 范围 | 必须遵守的规则 |
|---|---|
| 资源协议 | 宿主页和本地资源使用 `gesso://`；路由依据 URI host，路径百分号解码后再做穿越校验。视频媒体经回环 HTTP 并支持 Range，不走自定义协议媒体管线。 |
| WE 导入 | 仅解析用户在导入对话框主动选定的 `project.json`；不扫描 Steam/工坊。video/web 零拷贝只读直引，`source_dir` 只能通过库中不可猜的条目 ID 访问。 |
| 窗口销毁 | 所有仍存活进程内的拆窗路径，先 `set_visible(false)` 再 Drop；隐藏 webview 不能替代隐藏原生窗口。 |
| 显示器重配 | 事件只递增 epoch；由会话层去抖后收敛。几何过时的窗口先隐藏，稳定拓扑后再重施层级和几何；过渡只能使用 Gesso 自己的窗口，运行时不写系统桌面。 |
| Windows 贴壁 | 先将 Win32 窗口挂到 WorkerW，再创建 WebView2；Explorer 重启后重建窗口；在任何窗口创建前声明 PerMonitorV2 DPI awareness。壁纸 WorkerW 已存在时直接复用，0x052C 只在找不到时发一次——反复发送会堆积孤儿 WorkerW、重组桌面结构并把已挂载壁纸窗孤儿化。跨进程子窗从壁纸 WorkerW 移除后，部分驱动 + DWM 组合不重新合成该子树（残留最后一帧或黑底，失效重绘唤不醒）：退出清理在 InvalidateRect 之后必须同文件重设一次系统壁纸（SPI_SETDESKWALLPAPER）才能露出原壁纸。 |
| Windows 原生视频窗 | D3D11 呈现三要素缺一即全黑且零报错：Draw 前显式 `IASetPrimitiveTopology`（默认拓扑 UNDEFINED 时 Draw 静默不画）；顶点数据必须走顶点缓冲（不绑 VB、仅凭 SV_VertexID 生成的 Draw 在部分 AMD 驱动上同样静默不画，WindowDev/RX580 31.0.12044.3 实测）；常量缓冲必须 `PSSetConstantBuffers` 绑到 PS 槽位（漏绑读全零，fit 缩放除零全屏落越界黑）。此三类失败 Present 仍返回成功，日志全绿，只有实机截图能发现。 |
| macOS UI | AppKit/wry/WKWebView 操作只在主线程；异步快照完成前不关闭采集窗口。ObjC 使用 objc2 绑定，后台任务放在 autoreleasepool 内。 |
| 采集 | 采集窗口保持可见但近乎透明，且不被壁纸遮挡；任务全局串行。WebGL 采集需 `preserveDrawingBuffer: true`。 |
| 视频 | macOS 使用 AVPlayerLayer，Windows 使用 Media Foundation + DXGI；循环视频小于等于 256 MB 时由媒体服务缓存到内存（服务 webview 媒体与 macOS 原生视频窗；Windows 原生视频窗直读本地文件，不依赖该服务）。 |
| 宿主页命令 | Rust 仅用 `HostCommand` 生成脚本；光标 30 Hz 推送，宿主页保存最新目标并自行平滑。 |
| 数据完整性 | 清单解析失败时不得回退为空库再写回；临时输出写入 `.tmp` 后原子 rename；环境变量测试须持有共享锁。 |
| UI 细节 | `overflow_y_scroll` 必须在 `.id(...)` 后调用；`AtomicBool` 切换用 `fetch_xor(true)`，不要用 `swap(true)`。 |
| **Windows 宠物窗口穿透组合（P2-W，2026-10-25 实测）** | `WS_EX_LAYERED` + `UpdateLayeredWindow`（ULW_ALPHA，DIB 预乘 BGRA）逐像素 alpha：alpha=0 区域系统直接放行鼠标（分层窗口文档语义），透明穿透零成本成立。**命中区外（火苗/气泡区）必须整窗切 `WS_EX_TRANSPARENT`**（150ms 同步节拍判定光标 vs 屏幕命中区，等价 macOS `ignoresMouseEvents`）——alpha≠0 像素系统视为窗口命中会吞点击，而 wndproc 返回 HTTRANSPARENT 跨进程不可靠（见 pin 实测行）。按住/拖动期间禁止加 TRANSPARENT，否则 SetCapture 链路中断 |
| **窗口内容验证用 PrintWindow，别信屏幕截图（P2-W）** | 置顶分层窗在**独占全屏游戏**下不参与 DWM 合成（屏幕上看不到），CopyFromScreen 得出"没渲染"的假阴性；`PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` 直接渲染窗口自身内容，被遮挡/独占全屏都照拍（P2-W 靠它确诊渲染管线正常）。另：DPI-unaware 工具（PowerShell 默认）查窗口坐标是虚拟化值，先 `SetProcessDpiAwarenessContext(-4)` 再 GetWindowRect 才是物理像素 |
| RGBA 字节流当调色板索引（P2-W 渲染全透明 bug） | 精灵预渲染产物是 RGBA **字节**数组（`render_rgba` 输出，格号×4 才是字节偏移）不是索引网格——`sprite[格号]` 读到别格 R 字节再乘 4 当偏移，几乎全像素被误判透明跳过，整窗隐身且无报错。写像素循环前先确认数据布局 |
| 跨 DPI 窗口先落位再读 DPI（P2-W） | `GetDpiForWindow` 返回窗口**当前所在显示器**的 DPI——建窗在主屏、宠物在另一块屏（混合 DPI）时首拍读到错缩放。先 SetWindowPos 到目标屏中心再查（`placed` 旗标），每拍重查自然收敛模式切换 |
| macOS 精灵帧序映射 quirk（feat/desktop-pet 遗留） | macos `create_window` 预渲染循环 `match frame_idx {0=>Idle,1=>Flicker,_=>Sleep}`——Blink/GazeLeft/GazeRight（索引 3–5）全落到 Sleep 帧（眨眼/注视时火苗变暗）。Windows 实现六帧各自正确映射；macOS 侧修正是独立变更，勿混进平台移植提交 |
| **宠物 press 状态悬挂（P2-W 复审修复，2026-10-25）** | Win32 按住期间捕获被夺（全屏应用启动把宠物隐藏等）只收 `WM_CAPTURECHANGED` 不收 `WM_LBUTTONUP`——press 悬挂会**永久卡死** sync 的拖拽守卫与穿透切换。必须在 wndproc 处理该消息收尾；且 `on_left_up` 要**先取走 press 再 ReleaseCapture**——ReleaseCapture 同步派发 WM_CAPTURECHANGED，处理器会抢先消费 press 导致点击丢失。macOS 无显式信号（mouseUp 丢失同样悬挂），是两平台共同的潜在形状 |
| **GDI DIB 位图先拷再删（P2-W 复审修复，实机单测 ACCESS_VIOLATION 抓获）** | `CreateDIBSection` 的 bits 指针在 `DeleteObject(dib)` 后即悬垂——清理顺手写在读取前是 use-after-free，多数时候读到未回收内存"碰巧能用"，气泡路径首次真实执行即崩。铁律：**从 DIB 拷出数据后才允许删任何 GDI 对象**；`text_mask` 有单测锁定（GDI 光栅化产出白底+笔画双值掩码） |
| **TrackPopupMenu 模态期冻结宿主窗口几何（P2-W 复审修复）** | 150ms 主循环对宠物窗的 TOPMOST 重申/位置校正在菜单打开期间会把它抖掉（走动还会把菜单甩在原地）——`menu_open` 旗标期间跳过一切窗口几何操作（渲染/统计照常）。另：测菜单行为必须用 `mouse_event` 真实注入，`PostMessage` 合成输入不授予前台权，`SetForegroundWindow` 失败 → 菜单收不到键盘自动退场（命令恒返回 0） |

## 3. 高风险调试

- 截图验证前授予终端屏幕录制权限；否则 `screencapture` 不能证明壁纸窗口的实际内容。
- 排查视频先确认使用原生视频窗口，再检查媒体服务与解码日志；`GESSO_VIDEO_DIAG=1` 探针只覆盖 webview 播放，对原生视频窗当前无输出。不要以网页能播放代替帧节奏验证。
- 路径、清单或导入异常先检查共享解析函数及其全部调用方，避免在调用端逐个打补丁。
- Windows WebView2 `ExecuteScript` 返回 JSON 编码值；异步脚本的 Promise 不会自动等待，应将结果写到页面状态再读取。
- 使用开发期多实例后清理旧进程；单靠 `pgrep` 不能证明新实例已启动。

## 4. 关键路径（调试用）

```
壁纸素材库   ~/Library/Application Support/Gesso/library/<条目id>/index.<ext>（网页条目为 wallpaper.html）
库清单       ~/Library/Application Support/Gesso/library/library.json
配置         ~/Library/Application Support/Gesso/config.json   （monitors: {显示器 ID → 条目id}）
宿主页源      crates/app/assets/host/index.html（统一副本经 gesso://host 提供，不拷入条目目录）
内置样例      crates/app/assets/samples/testsrc.mp4
运行日志      stdout：./target/debug/gesso 2>&1 | tee /tmp/gesso.log
```
宿主页契约：URL query `spec=`（urlencoded JSON `ContentSpec`）；Rust 侧命令经 `HostCommand`（`host_cmd.rs` 单点序列化）驱动 `window.__gesso.{pause,resume,setFps,tick,mouse,…}`（暂停 = JS 层停帧，窗口常驻）。

## 5. 常用命令

```bash
cargo build -p gesso-app                 # 构建
cargo run   -p gesso-app                 # 运行（托盘 + 管理窗口 + 壁纸）
cargo test  -p gesso-core -p gesso-app   # 核心单测 + 引擎/导入逻辑单测
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"            # 退出
GESSO_LOCK=dev ./target/debug/gesso      # 开发期多实例并存（用完记得关）
```

## 6. 已删除的历史资产（勿重复引入）

- `crates/app/examples/m05.rs`：GPUI 窗口内嵌 webview 的历史 spike；不要恢复这条路线。
- `HANDOFF.md`：一次性交接清单，任务完成后其耐久部分已并入本文档。
- `crates/app/src/ui/quick_panel.rs`（已删除的托盘左键快速面板）：浮动小窗不锚定托盘图标；托盘当前使用纯菜单形态。勿恢复。
