# 工程笔记（Gesso）

> 常驻文档：**架构规则 + 当前平台纪律 + 高风险调试路径**。设计文档在 [design/](design/)（随仓库版本化，后续迭代以本目录为准），引擎接口在 `crates/app/API.md`。
> 新规则应直接补入对应小节；历史事故不在这里展开。

---

## 1. 架构规则（改前必读）

1. **壁纸窗口不是 GPUI 窗口**：`pin/` 使用平台原生窗口和 `lb-wry` 直挂；GPUI 只管管理窗口与托盘。壁纸窗口的桌面级几何由 `pin/` 和会话收敛负责。
2. **UI 不碰引擎内部**：只经 `crates/app/API.md`。写操作入队 `engine::EngineAction`（引擎 150ms 轮询执行，与托盘同一通道）；读操作走 `snapshot_ui(&sm)` 快照，且**必须保留 UI 本地状态**（`active_tab/selected/query/filter/import_counter`），否则用户输入每 150ms 被冲掉。
3. **改界面先改规格与原型**（`docs/design/界面与交互设计.md` + `docs/design/prototype/index.html`），再改代码；token/组件/文案以 `docs/design/DESIGN.md` 为准。
4. **提交前必须 `cargo check -p gesso-app` 通过**。UI 代码曾因从未编译积累 115 个错误。
5. **类型知识与主资源发现只在单一事实源**：类型↔扩展名↔MIME↔缩略图策略查 `gesso_core::content`；主资源文件名走 `encoding::main_asset_name`，导入/失效判定/宿主页 spec 共用。四处各写一份“类型→扩展名”映射，就会出现“能导入但被判失效”或“宿主页请求错文件名”。
6. **平台 FFI 的所有权不裸写**。ObjC 一律走 objc2 生成绑定（所有权编码在类型里：init/copy 家族 → `Retained<T>`，autoreleased 返回值由绑定内部 `objc_retainAutoreleasedReturnValue` 处理）；手写 extern 只留给纯 C API（CF 的 +1/CFRelease 对称即可，如 ImageIO `CGImageDestination`）。后台线程做 ObjC/AVFoundation 整段包 `objc2::rc::autoreleasepool`，并且只跑在有重试上限 + 在途去重的调度器后面（`thumb.rs` + `engine::ThumbScheduler` 是范本）。

## 2. 当前平台纪律

| 范围 | 必须遵守的规则 |
|---|---|
| 资源协议 | 宿主页和本地资源使用 `gesso://`；路由依据 URI host，路径百分号解码后再做穿越校验。视频媒体经回环 HTTP 并支持 Range，不走自定义协议媒体管线。 |
| WE 导入 | 仅解析用户在导入对话框主动选定的 `project.json`；不扫描 Steam/工坊。video/web 零拷贝只读直引，`source_dir` 只能通过库中不可猜的条目 ID 访问。 |
| 窗口销毁 | 所有仍存活进程内的拆窗路径，先 `set_visible(false)` 再 Drop；隐藏 webview 不能替代隐藏原生窗口。 |
| 显示器重配 | 事件只递增 epoch；由会话层去抖后收敛。几何过时的窗口先隐藏，稳定拓扑后再重施层级和几何；过渡只能使用 Gesso 自己的窗口，运行时不写系统桌面。 |
| Windows 贴壁 | 先将 Win32 窗口挂到 WorkerW，再创建 WebView2；Explorer 重启后重建窗口；在任何窗口创建前声明 PerMonitorV2 DPI awareness。 |
| macOS UI | AppKit/wry/WKWebView 操作只在主线程；异步快照完成前不关闭采集窗口。ObjC 使用 objc2 绑定，后台任务放在 autoreleasepool 内。 |
| 采集 | 采集窗口保持可见但近乎透明，且不被壁纸遮挡；任务全局串行。WebGL 采集需 `preserveDrawingBuffer: true`。 |
| 视频 | macOS 使用 AVPlayerLayer，Windows 使用 Media Foundation + DXGI；循环视频小于等于 256 MB 时由媒体服务缓存到内存。 |
| 宿主页命令 | Rust 仅用 `HostCommand` 生成脚本；光标 30 Hz 推送，宿主页保存最新目标并自行平滑。 |
| 数据完整性 | 清单解析失败时不得回退为空库再写回；临时输出写入 `.tmp` 后原子 rename；环境变量测试须持有共享锁。 |
| UI 细节 | `overflow_y_scroll` 必须在 `.id(...)` 后调用；`AtomicBool` 切换用 `fetch_xor(true)`，不要用 `swap(true)`。 |

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
