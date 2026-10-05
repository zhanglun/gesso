# 工程笔记（Gesso）

> 常驻文档：**架构规则 + 踩坑实录 + 关键路径**。设计文档在 [design/](design/)（随仓库版本化，后续迭代以本目录为准），引擎接口在 `crates/app/API.md`。
> 每次新增踩坑请追加到 §2，别让知识随会话消失。

---

## 1. 架构规则（改前必读）

1. **壁纸窗口不是 GPUI 窗口**：`pin/` 用纯 AppKit `NSWindow` + `lb-wry` 直挂（M1.5 定稿）。GPUI 只管管理窗口与托盘。任何把壁纸内容塞回 GPUI 窗口的做法都会重演 M1.5 失败——GPUI 窗口协调器会把窗口 `origin.y` 压到 `-菜单栏高`，外部强制会被它在同一通知循环内改回（150ms 轮询对抗打不赢，且方案本身错误）。
2. **UI 不碰引擎内部**：只经 `crates/app/API.md`。写操作入队 `engine::EngineAction`（引擎 150ms 轮询执行，与托盘同一通道）；读操作走 `snapshot_ui(&sm)` 快照，且**必须保留 UI 本地状态**（`active_tab/selected/query/filter/import_counter`），否则用户输入每 150ms 被冲掉。
3. **改界面先改规格与原型**（`docs/design/界面与交互设计.md` + `docs/design/prototype/index.html`），再改代码；token/组件/文案以 `docs/design/DESIGN.md` 为准。
4. **提交前必须 `cargo check -p gesso-app` 通过**。UI 代码曾因从未编译积累 115 个错误。
5. **类型知识与主资源发现只在单一事实源**：类型↔扩展名↔MIME↔缩略图策略查 `gesso_core::content`；主资源文件名走 `encoding::main_asset_name`，导入/失效判定/宿主页 spec 共用。四处各写一份“类型→扩展名”映射，就会出现“能导入但被判失效”或“宿主页请求错文件名”。
6. **平台 FFI 的所有权不裸写**。ObjC 一律走 objc2 生成绑定（所有权编码在类型里：init/copy 家族 → `Retained<T>`，autoreleased 返回值由绑定内部 `objc_retainAutoreleasedReturnValue` 处理）；手写 extern 只留给纯 C API（CF 的 +1/CFRelease 对称即可，如 ImageIO `CGImageDestination`）。后台线程做 ObjC/AVFoundation 整段包 `objc2::rc::autoreleasepool`，并且只跑在有重试上限 + 在途去重的调度器后面（`thumb.rs` + `engine::ThumbScheduler` 是范本）。

## 2. 踩坑实录（照抄即可）

| 坑 | 正确做法 |
|---|---|
| 大片 "no method" 错误 | 多为 trait 未导入：`gpui_base::StyledExt as _`（`.v_flex()/.h_flex()`）、`gpui::{InteractiveElement, StatefulInteractiveElement, ParentElement, Styled, AppContext, BorrowAppContext, Focusable} as _` |
| 组件自带图标是子集 | 用完整 Lucide：`gpui_kit_assets::IconName`（`Scan/Wallpaper/RotateCcw/Film/Image/Sparkles/Zap` 等） |
| `Button` 没有 `color()` | 用变体：`.danger()/.primary()/.secondary()/.text()`（`ButtonVariants`） |
| `InputState` 状态构建器没有 `cleanable` | 在元素上：`Input::new(&state).cleanable(true)` |
| 右键菜单子项 | `PopupMenuItem::submenu(label, menu)` 是**构造器**，不是方法链 |
| `IndexPath` 私有路径 | `gpui_kit::component::IndexPath` |
| `overflow_y_scroll` 找不到 | 属 `StatefulInteractiveElement` → **必须在 `.id(...)` 之后** |
| 闭包借用逃逸（`t.accent`、`m.wallpaper`） | 构造期求值成 owned 副本再 `move` 进闭包 |
| `gesso://` 自定义协议 | **已打通（lb-wry ≥0.53）**——M2 “回调零触发”结论已过时（可能是旧版本/未注册）。独立夹具确认：文档与跨 host 子资源回调均正常。注意 URI 结构：`gesso://<host 段>/<path>`，路由要看 `uri.host()` 不是 `uri.path()`。视频必须支持 **Range(206)**，否则播放器几百次重试。首帧直接用 gesso URL |
| WE 零拷贝直引 | WE video/web 不拷入库，`source_dir` 指 Steam 工坊目录，经 `gesso://steam/<相对工坊路径>` 只读加载；路由强制路径落在 `steamapps/workshop/content/431960` 下、只允许 Normal 分量。video 走 Range，web 主 HTML 在协议层内存注入 shim（原文件只读不改）。主文件名（可能非 index.*）存 `LibraryEntry.main_file`。**坑 1：webview 发来的 `uri.path()` 是百分号编码态（空格=%20），提取后必须 `percent_decode` 再做穿越校验（否则 `%2e%2e` 可绕过），再读盘**。**坑 2：`mime_for_ext("html")` 返回 `text/html; charset=utf-8`，判断类型要用 `starts_with("text/html")` 不能 `==`** |
| WE 多内容库 | Steam 可有多个库（主库 + 其他盘）。主库 `steamapps/libraryfolders.vdf` 里逐行取 `"path" "..."`（反斜杠转义还原），全部纳入扫描/路由。gesso URL **不带库标识**（库编号 vdf 内不稳定、路径才稳定）：`entry_url` 找包含 source_dir 的库取相对路径，route 再遍历全部库找文件存在者归属解析 |
| 测试环境变量竞争 | `STEAM_DIR`/`HOME` 是进程级全局变量。并行测试各自 `set_var` 会互相覆盖 → 间歇性读错夹具（flaky）。夹具目录用随机 id 只解决“文件互删”，不解决“变量值覆盖”。需共用一把 `ENV_LOCK`，所有读写这些变量的测试先持锁（`main.rs` 里的 `static ENV_LOCK`） |
| 路径编码 | 旧 file:// 时代 `Application Support` 空格需百分号编码；gesso 协议由 handler 内部解析文件路径，URL 只传相对段，不再需要 path 编码 |
| 创建顺序 | 必须在 `gpui_kit::init(cx)` 之后、GPUI 窗口之前创建 AppKit 壁纸窗口；否则 tray-icon panic：`Ivar platform not found on class NSApplication` |
| **`swap(true)` 当开关** | `AtomicBool::swap(true)` 永远写入 `true`、永远读到同一个旧值 → 开关变成"只单向"。**用 `fetch_xor(true)`**。（M0.5 的壁纸切换、托盘「暂停全部」各栽过一次） |
| 媒体导入改名 | **保留源扩展名**：WKWebView 按扩展名判定媒体类型，`.webm` 存成 `index.mp4`、`.webp` 存成 `index.gif` 直接播不出来 |
| 单实例多开 | 正常行为：锁生效（第三个实例会打印"已有实例运行，退出"）。开发期用 `GESSO_LOCK=<name>` 可并存多实例——**验证完记得杀掉旧实例**，否则两个壁纸窗口叠在桌面上。**重启验证以日志内容为准**（如"会话数"行）：`pgrep` 只能证明"有进程"，旧实例存活时新实例已静默退出，pgrep 误报"运行中" |
| 截图验证 | 若 `screencapture` 拍不到窗口内容：系统设置 → 隐私与安全性 → 屏幕录制 → 给终端打开（否则像素判读全部失真） |
| `mkv` / HEVC | `.mkv` 需转封装；HEVC 依赖系统扩展/硬件——导入时给明确文案，不要静默失败 |
| 裸 `msg_send!` 人肉维护 ObjC 引用计数 | v1 缩略图抽帧两处违约（alloc+init 被两次 `Retained::from_raw` 接管；`representationUsingType:properties:` 的 **autoreleased** 返回值被当 +1 接管）→ 对象提前释放，autorelease 池里的悬垂记录在 GCD drain 结束时二次释放 → **池弹出段错误**，且崩溃点远离案发现场（2026-10-03/04 两次线上崩溃同指纹）。正确做法：objc2 生成绑定 + ImageIO 编码 + `autoreleasepool` 包任务（见 `thumb.rs` v2）；PNG 等大对象的 over-release 会 munmap 掉整个 VM region，池弹出时是翻译 fault 而非静默损坏 |
| 缺帧条目无限重试 | 曾每 30s 对 `thumbs.len()<2` 的条目无限重抽——持续失败的条目（坏文件/磁盘满）变成无限 FFI 空转 + 崩溃放大器。用 `engine::ThumbScheduler`（在途去重 + 每会话 3 次上限）收敛；抽帧输出先写 `.tmp` 再 rename，崩溃不留半截帧 |
| `file://` 页面里 fetch/XHR 被拦 | WKWebView 的 file 源是 opaque origin，`fetch("./index.glsl")` 直接 CORS 拒绝——**shader 源码改走 URL 查询参数**（`code=base64url`，无填充免转义），宿主页 `atob` + `TextDecoder` 解码。img/video/script 子资源不受限，别混为一谈 |
| shader/html 缩略图采集（capture.rs，原 shader_thumb.rs 泛化） | 两条纪律，均实测踩过：① **采集窗口必须「屏内 + 不被遮挡」**——屏幕外或被壁纸完全盖住时 WebKit 停摆 RAF/合成，快照全黑（RAF 探针 400ms 增量=0 可确诊）；现为壁纸层之上一档 + alphaValue(0.02) 常驻，肉眼不可见。② **快照 completion 悬挂期间绝不能 close 窗口**——WebKit 异步回调摸已释放层 → 池排出时 over-release SIGSEGV（同 thumb v1 指纹）；窗口进程级持久永不 close + 采集全局串行 |
| WebGL 像素判读 | 默认帧缓冲合成后即被清空：合成器清屏后 `readPixels`/`toDataURL` 全读到黑。验证渲染要**在同一次 JS 任务里 `drawArrays` 后立刻 `readPixels`**，或建上下文时 `preserveDrawingBuffer: true` |
| IOKit 电源 API 的符号在新 SDK 被移除 | 借用式 `IOPSGetPowerSourceList` 在 macOS 15.4 SDK 的 IOKit.tbd 里**已无导出**（链接期 undefined）——改用 `IOPSCopyPowerSourcesList`（+1，用完 `CFRelease`），`IOPSGetPowerSourceDescription` 仍是借用随 info 释放。纯 C API 也会踩符号可用性，extern 前先 `grep` 本机 tbd |
| 全屏检测的坐标系 | `CGWindowList` bounds 是**全局顶左原点**，`MonitorInfo.frame` 是 AppKit **底左原点**——y 不能直接比。翻转基准 = 主显示器高度：`y_appkit = primary_h - win_y - win_h`。全屏判定：layer 0 窗口 bounds 与显示器 frame 重合（±3pt），菜单栏(24)/Dock(20)/壁纸(-2147483604) 天然被 layer 过滤 |
| `flex_1`/`min_h_0` 在非 flex 父级无效 | 嵌在普通 `div()` 里的滚动容器（`overflow_y_scroll`）高度被内容撑开、永不溢出（库页滚不动的根因）——中间包裹层必须也是 flex（交叉轴默认 stretch 给子级定高）。对照：设置页滚动容器直接挂在 `v_flex` 下所以一直正常。排查口诀：滚动不动先查**高度约束链**上有没有断点 |
| 网格响应式列数 | GPUI 没有 CSS `auto-fill`：在 `render` 里用 `window.viewport_size()` 按容器宽算列数（min 宽 + gap），卡片等分宽撑满整行；窗口 resize 会触发重渲，无需额外监听（库页 880→4 列 / 1200→5 列实机验证）。所有卡片同一渲染帧必须同一宽度，否则最后一行参差 |
| GPUI 图片元素三条机制（hover 预览多轮踩坑） | ① **`image_cache` 元素不绘制自身样式**——只转发子元素，`.bg()` 挂它上面是死的（底色必须画在外层普通 div 上）；② **`img` 无元素 id → 不建 `ImgState`** → `with_loading` fallback 分支整体跳过；③ **默认 loading 延迟 200ms** 且同一帧的资源加载完成会批量 notify——固定节奏轮播未就绪帧 = 底色/图片交替 + 播速忽快忽慢。正解：hover 两段式，`fetch_asset::<ImgResourceLoader>` 预载全部帧（与显示共用同一缓存）后再固定节奏播放 |
| 光标跟随三个坐标系/线程坑（M5 光标 feed 多轮实测） | ① **`CGEventGetLocation` 是左上原点 CG 坐标**，与 MonitorInfo/NSEvent（左下 AppKit 坐标）混用上下颠倒——位置用 `NSEvent::mouseLocation()`；② 左下原点的归一化坐标喂给 Shadertoy `fragCoord`/`iMouse`（本就左下原点）要**直接映射，别再 `1-y`** 翻一次；③ **`NSEvent.mouseLocation` 绝不能从后台线程读**——快速移动时是陈旧值，光团卡住、鼠标停下才闪现到终点；后台线程只出节拍，读 AppKit 状态必须主线程 |
| 光标跟随的推送节奏 | 60Hz 连续 `evaluate_script` 会在跨进程 FIFO 队列堆积→滞后；推送/渲染两个独立时钟→有的帧空转有的帧双跳→闪现。正解：**Rust 30Hz 推送（队列不堆积）+ 宿主页只存最新目标 + 每帧帧率无关平滑**（`k=1-e^{-35·dt}`≈2 帧追上），视觉 60fps 连续、无堆积 |
| WebView2 在 WorkerW 下整窗不渲染（M1 实测） | **创建顺序是生死线**：先 `SetParent` 挂 WorkerW、后 `CreateCoreWebView2Controller`——DComp 视觉树在 controller 创建时绑定宿主层级，反序则窗口树全绿（visible/非 cloak/尺寸正确）但 DWM 不合成，桌面透出系统壁纸；脱离 WorkerW 同一窗口立即渲染可确诊。重钉同理：SetParent 后必须重建 webview。旁证：Lively 也是先挂载后建控件 |
| explorer 重启连带销毁壁纸窗口（M1 实测） | 跨进程 `SetParent` 后本方窗口成为 explorer WorkerW 的子窗口——explorer 死亡时子窗口被一并销毁（本进程 Drop 未运行、句柄已失效），重钉不能对旧句柄操作（重建 controller 报 0x80070578 无效句柄），必须**整窗重建**（session 层 `remount_all`）。`TaskbarCreated` 广播只发给顶层窗口——SetParent 后收不到，需要常驻隐藏监听窗口代收 |
| Windows 进程 DPI awareness 没人替你设（M1 实测） | GPUI 0.3.7 快照与 wry 均不调 `SetProcessDpiAwarenessContext`，默认 DPI-unaware：显示器枚举/窗口定位全部拿到虚拟化坐标，125% 缩放下与 DPI-aware 的 explorer/WorkerW 错位 25%。main() 顶部（任何窗口创建前）运行时声明 `PerMonitorV2` 等效于 manifest |
| VDF/路径文本按字节解析会毁 UTF-8（M6 实测） | `libraryfolders.vdf` 的库路径含非 ASCII（中文库名）时，字节级 `push(b as char)` 产出 mojibake → 目录判定失败 → 库静默丢失。按字符（`chars()`）处理转义还原；测试夹具写 Windows 路径记得转义反斜杠（真实 Steam 是 `D:\Steam`） |
| 条目目录里宿主页与用户资源共用 `index.*` 命名空间 | 真实事故：图片条目目录有 `index.html`（每次启动 `ensure_entry_host` 拷入的宿主页）+ `index.png`，`read_dir` 恰好先返回 html；旧 `main_asset_name` 的“任意 `index.*` 兑底”把宿主页当主资源 → 缩略图/类型判定全错。根因做法：`main_asset_name` 只在该类型的扩展名白名单（`content_type(kind).extensions`）内匹配，html 类型主资源固定 `wallpaper.html`。**修 bug 先 grep 所有调用点，在共享函数加一次守卫，而非每个调用方打补丁** |
| 手拼跨语言命令字符串 | Rust 多处手拼 `__gesso&&__gesso.setFps(5)`，宿主端各自定义，改名靠人肉；`&&__gesso.` 这种空指针守卫还散落各点。正解：`HostCommand` 枚举 + 唯一 `to_js` 序列化点，`WallpaperWindow::send` 统一入口，加/改命令由编译器扫所有调用点 |

## 3. 关键路径（调试用）

```
壁纸素材库   ~/Library/Application Support/Gesso/library/<条目id>/index.<ext>
库清单       ~/Library/Application Support/Gesso/library/library.json
配置         ~/Library/Application Support/Gesso/config.json   （monitors: {cg-id → 条目id}）
宿主页源      crates/app/assets/host/index.html（每次启动同步进条目目录）
内置样例      crates/app/assets/samples/testsrc.mp4
运行日志      stdout：./target/debug/gesso 2>&1 | tee /tmp/gesso.log
```
宿主页契约：URL query `spec=`（urlencoded JSON `ContentSpec`）；暴露 `window.__gesso.{pause,resume}` 供引擎经 `evaluate_script` 调用（暂停 = JS 层停帧，窗口常驻）。

## 4. 常用命令

```bash
cargo build -p gesso-app                 # 构建
cargo run   -p gesso-app                 # 运行（托盘 + 管理窗口 + 壁纸）
cargo test  -p gesso-core -p gesso-app   # 核心单测 + 引擎/导入逻辑单测
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"            # 退出
GESSO_LOCK=dev ./target/debug/gesso      # 开发期多实例并存（用完记得关）
```

## 5. 已删除的历史资产（勿重复引入）

- `crates/app/examples/m05.rs`：GPUI 窗口内嵌 webview 的 spike。结论已入 `SPIKE-REPORT.md`，该路线被 M1.5 否决。
- `HANDOFF.md`：一次性交接清单，任务完成后其耐久部分已并入本文档 §1–§4。
- `crates/app/src/ui/quick_panel.rs`（托盘左键快速面板）：**2026-10-03 用户决策撤销**——浮动小窗不锚定托盘图标，观感突兀；同类产品（WE/Plash）均为纯菜单形态。规格 §4.1/DESIGN.md signature（×3→×2）/原型已同步改版；勿以「signature #3」名义再引入。
