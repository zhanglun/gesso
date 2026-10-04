# 工程笔记（Gesso）

> 常驻文档：**架构规则 + 踩坑实录 + 关键路径**。设计文档在 [design/](design/)（随仓库版本化，后续迭代以本目录为准），引擎接口在 `crates/app/API.md`。
> 每次新增踩坑请追加到 §2，别让知识随会话消失。

---

## 1. 架构规则（改前必读）

1. **壁纸窗口不是 GPUI 窗口**：`pin/` 用纯 AppKit `NSWindow` + `lb-wry` 直挂（M1.5 定稿）。GPUI 只管管理窗口与托盘。任何把壁纸内容塞回 GPUI 窗口的做法都会重演 M1.5 失败——GPUI 窗口协调器会把窗口 `origin.y` 压到 `-菜单栏高`，外部强制会被它在同一通知循环内改回（150ms 轮询对抗打不赢，且方案本身错误）。
2. **UI 不碰引擎内部**：只经 `crates/app/API.md`。写操作入队 `engine::EngineAction`（引擎 150ms 轮询执行，与托盘同一通道）；读操作走 `snapshot_ui(&sm)` 快照，且**必须保留 UI 本地状态**（`active_tab/selected/query/filter/import_counter`），否则用户输入每 150ms 被冲掉。
3. **改界面先改规格与原型**（`docs/design/界面与交互设计.md` + `docs/design/prototype/index.html`），再改代码；token/组件/文案以 `docs/design/DESIGN.md` 为准。
4. **提交前必须 `cargo check -p gesso-app` 通过**。UI 代码曾因从未编译积累 115 个错误。
5. **导入 / 失效判定 / 宿主页 spec 三处必须共用同一函数**（`session::main_asset_name`）。三处各写一份"类型→扩展名"的映射，就会出现"能导入但被判失效"或"宿主页请求错文件名"。
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
| `gesso://` 自定义协议 | 本版 lb-wry/WKWebView **回调零触发**；若当**首帧 URL** 会让 webview 进入"URL 更新但永不绘制"死状态。M2 走**条目自包含 `file://`**（宿主页拷进条目目录 + 相对媒体） |
| 库路径含空格 | `Application Support` 必须百分号编码后才能拼 `file://` |
| 创建顺序 | 必须在 `gpui_kit::init(cx)` 之后、GPUI 窗口之前创建 AppKit 壁纸窗口；否则 tray-icon panic：`Ivar platform not found on class NSApplication` |
| **`swap(true)` 当开关** | `AtomicBool::swap(true)` 永远写入 `true`、永远读到同一个旧值 → 开关变成"只单向"。**用 `fetch_xor(true)`**。（M0.5 的壁纸切换、托盘「暂停全部」各栽过一次） |
| 媒体导入改名 | **保留源扩展名**：WKWebView 按扩展名判定媒体类型，`.webm` 存成 `index.mp4`、`.webp` 存成 `index.gif` 直接播不出来 |
| 单实例多开 | 正常行为：锁生效（第三个实例会打印"已有实例运行，退出"）。开发期用 `GESSO_LOCK=<name>` 可并存多实例——**验证完记得杀掉旧实例**，否则两个壁纸窗口叠在桌面上 |
| 截图验证 | 若 `screencapture` 拍不到窗口内容：系统设置 → 隐私与安全性 → 屏幕录制 → 给终端打开（否则像素判读全部失真） |
| `mkv` / HEVC | `.mkv` 需转封装；HEVC 依赖系统扩展/硬件——导入时给明确文案，不要静默失败 |
| 裸 `msg_send!` 人肉维护 ObjC 引用计数 | v1 缩略图抽帧两处违约（alloc+init 被两次 `Retained::from_raw` 接管；`representationUsingType:properties:` 的 **autoreleased** 返回值被当 +1 接管）→ 对象提前释放，autorelease 池里的悬垂记录在 GCD drain 结束时二次释放 → **池弹出段错误**，且崩溃点远离案发现场（2026-10-03/04 两次线上崩溃同指纹）。正确做法：objc2 生成绑定 + ImageIO 编码 + `autoreleasepool` 包任务（见 `thumb.rs` v2）；PNG 等大对象的 over-release 会 munmap 掉整个 VM region，池弹出时是翻译 fault 而非静默损坏 |
| 缺帧条目无限重试 | 曾每 30s 对 `thumbs.len()<2` 的条目无限重抽——持续失败的条目（坏文件/磁盘满）变成无限 FFI 空转 + 崩溃放大器。用 `engine::ThumbScheduler`（在途去重 + 每会话 3 次上限）收敛；抽帧输出先写 `.tmp` 再 rename，崩溃不留半截帧 |
| `file://` 页面里 fetch/XHR 被拦 | WKWebView 的 file 源是 opaque origin，`fetch("./index.glsl")` 直接 CORS 拒绝——**shader 源码改走 URL 查询参数**（`code=base64url`，无填充免转义），宿主页 `atob` + `TextDecoder` 解码。img/video/script 子资源不受限，别混为一谈 |
| WebGL 像素判读 | 默认帧缓冲合成后即被清空：合成器清屏后 `readPixels`/`toDataURL` 全读到黑。验证渲染要**在同一次 JS 任务里 `drawArrays` 后立刻 `readPixels`**，或建上下文时 `preserveDrawingBuffer: true` |

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
