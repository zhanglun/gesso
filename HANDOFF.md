# 交接文档 · Gesso UI 会话

> 面向下一个会话。**先读这份，再读 `crates/app/API.md`（引擎调用契约）、`../gesso-design/技术方案.md`（工程方案）、`../gesso-design/界面与交互设计.md` + `../gesso-design/prototype/index.html`（冻结的视觉契约）**。
> 更新日期：2026-10-03 · 仓库状态：`main` 已包含引擎 + UI 的完整可运行版本

---

## 1. 当前状态（已实测）

| 项 | 状态 |
|---|---|
| 构建 | `cargo build -p gesso-app` 通过（0 错误，约 60 条 warning 待清） |
| 运行 | `./target/debug/gesso` → 管理窗口 880×600 + 壁纸窗口 1920×1080 |
| 壁纸链路 | ✅ 视频壁纸钉在桌面图标层下、全屏覆盖（含菜单栏带）、图标可点、Space 跟随 |
| 暂停/恢复 | ✅ 托盘「暂停全部」→ JS 层 `video.pause()`（真停），恢复同理 |
| 配置持久化 | ✅ 重启不重跑首启，`~/Library/Application Support/Gesso/config.json` 生效 |
| 显示器枚举 | ✅ 稳定 ID = `cg-<CGDirectDisplayID>`（本机 `cg-2`） |
| 核心单测 | ✅ `cargo test -p gesso-core` 14 项全绿 |
| UI | ✅ 三页签界面可跑（用户已目视确认"细节 OK"），走 `engine.rs` 的 `AppState` + `EngineAction` 接线 |

## 2. 架构铁律（违反即返工）

1. **壁纸窗口不是 GPUI 窗口**：`pin/` 用纯 AppKit `NSWindow` + `lb-wry` 直挂（M1.5 定稿）。GPUI 只管管理窗口与托盘。**任何把壁纸内容塞进 GPUI 窗口的做法都会重演 M1.5 的失败**（GPUI 窗口协调器会把窗口 origin.y 压到 -菜单栏高，且外部强制会被它改回）。
2. **UI 不碰引擎内部**：只经 `crates/app/API.md` 列出的接口。写操作走 `engine::EngineAction` 入队（引擎 150ms 轮询执行），读操作走 `snapshot_ui(&sm)` 快照。
3. **改界面先改规格与原型**（`gesso-design/界面与交互设计.md` + `prototype/index.html`），再改代码。冻结的 token/组件/文案以 `gesso-design/DESIGN.md` 为准。
4. **提交前必须 `cargo check -p gesso-app` 通过**——上一轮 UI 代码 115 个错误全是从未编译过造成的。

## 3. 已踩过的坑（照抄即可，别重新踩）

| 坑 | 正确做法 |
|---|---|
| trait 未导入导致大片"no method"错误 | `gpui_base::StyledExt as _`（`.v_flex()`/`.h_flex()`）、`gpui::{InteractiveElement, StatefulInteractiveElement, ParentElement, Styled, AppContext, BorrowAppContext, Focusable} as _` |
| 组件自带图标是子集 | 用完整 Lucide：`gpui_kit_assets::IconName`（`Scan/Wallpaper/RotateCcw/Film/Image/Sparkles/Zap` 等都在这里） |
| `Button` 无 `color()` | 用变体：`.danger()` / `.primary()` / `.secondary()` / `.text()`（`ButtonVariants` trait） |
| `InputState` 状态构建器没有 `cleanable` | 在元素上：`Input::new(&state).cleanable(true)` |
| 子菜单写法 | `PopupMenuItem::submenu(label, menu)` 是**构造器**，不是方法链 |
| `IndexPath` 私有路径 | `gpui_kit::component::IndexPath` |
| `overflow_y_scroll` 报"找不到方法" | 它属于 `StatefulInteractiveElement` → **必须在 `.id(...)` 之后**调用 |
| 闭包借用逃逸（`t.accent`、`m.wallpaper`） | 构造期求值成 owned 副本再 `move` 进闭包：`{ let accent = t.accent; move \|s,..\| ... }` |
| `gesso://` 自定义协议 | **本版 lb-wry/WKWebView 下注册后协议回调零触发**，且若把它当**首帧 URL** 会让 webview 进入"URL 更新但永不绘制"的死状态。M2 已走**条目自包含 `file://`** 方案（宿主页拷进条目目录 + 相对路径媒体）。协议修复是独立任务 |
| 库路径含空格 | `~/Library/Application Support/...` 必须百分号编码后才能拼 `file://` |
| 顺序陷阱 | 必须在 `gpui_kit::init(cx)` 之后、且在任何 GPUI 窗口之前创建 AppKit 壁纸窗口；否则 tray-icon 会 panic（`Ivar platform not found on class NSApplication`） |
| 截图验证 | 若 `screencapture` 拍不到窗口内容：系统设置 → 隐私与安全性 → 屏幕录制 → 给终端打开（否则像素判读全部失真） |

## 4. 关键路径（调试用）

```
壁纸素材库   ~/Library/Application Support/Gesso/library/<条目id>/index.{mp4,html,…}
库清单       ~/Library/Application Support/Gesso/library/library.json
配置         ~/Library/Application Support/Gesso/config.json      （monitors: {cg-id → 条目id}）
宿主页源      crates/app/assets/host/index.html（每次启动同步进条目目录）
内置样例      crates/app/assets/samples/testsrc.mp4
运行日志      stdout（`./target/debug/gesso 2>&1 | tee /tmp/gesso.log`）
```
宿主页契约：URL query `spec=`（urlencoded JSON ContentSpec）→ 渲染对应内容；暴露 `window.__gesso.{pause,resume}` 供引擎经 `evaluate_script` 调用。

## 5. 下一步任务（按优先级）

**P0 · UI↔引擎真实化 ✅（2026-10-03 本轮完成）**
1. ✅ `显示器页「重新检测」` → `EngineAction::SyncMonitors`
2. ✅ 库指派（双击/右键/拖放/全部）→ `Assign`；屏卡片 ⏸/▶ → `PauseOne`（新增 `SessionManager::pause_one`）；托盘「随机换一张」→ `CycleMain`；快照映射核对（`PausedUser→UserPaused`、`Autopause→FullscreenPaused`、`Playing→Playing`）
3. ✅ 失效态真判定：`snapshot_ui` 按 `source_dir/index.<ext按kind>` 存在性置 `broken`

**P1 · 导入流程 ✅（2026-10-03 本轮完成）**
4. ✅ `导入` 按钮 → `rfd` 选择 → `SessionManager::import_entry`（校验/`generate_id`/拷贝/`LibraryManifest::save`）→ 快照刷新
5. ✅ 失败文案（`strings.rs`）：`import_err_mkv/hevc/unsupported/io`；不支持类型 → 状态条红字 + 警告气泡
6. ✅ 拖入文件：gpui 原生 `ExternalPaths` 载荷 + 网格 `on_drop` → 同一导入路径（拖入期间网格 accent 描边）

**P2 · 设置与首启 ✅（2026-10-03 本轮完成）**
7. ✅ 设置页全部控件 → `UpdateSettings`（`Settings::to_core_settings` 映射）+ 落盘；开机自启 → `SetAutostart`（`auto-launch` crate）
8. ✅ 首启向导接真：`bootstrap` 返回 `first_run` 标志 → 启动开窗；样例步列**真实库条目**（双击 → 真指派）；空库给导入出路

**P3 · 原型对齐（部分完成）**
9. ⏳ 库卡片悬停预览（需导入期抽帧管线）；拖拽投放 signature #2 ✅（窗口内）；真机显示器边框描边 signature #1 ⏳（需透明描边窗口——先与引擎确认）
10. ✅ 托盘右键菜单（暂停全部/随机换一张/管理窗口…/开机自启✓/退出）+ 左键快速面板（`ui/quick_panel.rs`，360×280 PopUp，主屏右上角；Esc 关闭；点屏缩略图暂停、点最近缩略图两击换壁纸）

**已知缺口（下轮候选）**
- 托盘「暂停全部」菜单文案恒定（muda handler 要求 Send，不能持 MenuItem 改文案；状态经图标/界面反映）
- 快速面板定位用主屏右上角近似（未按托盘图标坐标精确锚定；retina 缩放换算待做）
- 悬停预览的抽帧动图管线、真机边框描边窗口（signature #1 真机版）

**独立任务（引擎侧，UI 会话不要动）**
- `gesso://` 协议修复（候选：注册前共享 config / 上游 wry 异步协议 API / 自研 WKURLSchemeHandler）
- Windows 贴壁（M1，需 Windows 机器）：自有 Win32 窗口 + `SetParent` 到 WorkerW + `GPUI_DISABLE_DIRECT_COMPOSITION=true`
- shader / html 渲染器（M4）、WE 素材导入（M6）

## 6. 常用命令

```bash
cargo build -p gesso-app            # 构建
cargo run   -p gesso-app            # 运行（托盘 + 管理窗口 + 壁纸）
cargo test  -p gesso-core           # 核心逻辑单测（秒级）
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"       # 退出（单实例锁：二实例会直接退出）
```
