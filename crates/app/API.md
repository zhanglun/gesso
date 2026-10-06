# 引擎 API 契约（UI 层唯一调用面）

> M2 已跑通（隔离分支 `feat/m2-engine`）。UI 层（`crates/app/src/ui/`）**只允许**调这个文件里列出的方法；
> 不要直接触碰 `pin/`、`protocol.rs`、`PinSession` 内部。所有 UI 需要的状态变化都通过这里的快照/事件获得。

## 1. 会话管理器（唯一编排者）

```rust
// crates/app/src/session.rs
pub struct SessionManager { /* private */ }

impl SessionManager {
    /// 指派/换壁纸：只重建该显示器会话（含状态机转移与落盘）
    pub fn assign(&mut self, monitor_id: &str, entry_id: &str);

    /// 暂停/恢复全部（等价于托盘「暂停全部」）
    pub fn pause_all(&mut self, paused: bool);

    /// 主显示器循环下一张
    pub fn cycle_main(&mut self);

    /// 全量同步显示器（启动、热插拔、手动「重新检测」都调它；幂等）
    pub fn sync_monitors(&mut self);

    /// UI 读状态（渲染数据源，一次拿全）
    pub fn states(&self) -> Vec<(String, String, SessionState)>;   // (显示器ID, 条目ID, 状态)

    /// 库（只读快照）
    pub fn library(&self) -> &[LibraryEntry];

    /// 配置（只读快照）
    pub fn config(&self) -> &AppConfig;

    /// 显示器列表（含 frame 与 is_main）
    pub fn monitors(&self) -> Vec<pin::MonitorInfo>;

    /// 单显示器暂停/恢复（§4.4 屏卡片 ⏸/▶；经状态机转移）
    pub fn pause_one(&mut self, monitor_id: &str, paused: bool);

    /// 导入：扩展名校验 → 随机 ID → 拷贝进库 → 清单落盘（P1）
    pub fn import_entry(&mut self, path: &Path) -> Result<LibraryEntry, ImportError>;
    /// 导入类型判定（UI 预检与引擎执行共用；ImportError = Unsupported/Mkv/Hevc/Io）
    pub fn classify_import(path: &Path) -> ImportCheck;

    /// 设置更新（写内存 + 落盘；设置页全部即时生效）
    pub fn update_settings(&mut self, settings: Settings);
}
```

## 2. 状态与类型（`gesso-core` 已导出）

```rust
pub enum SessionState { Idle, Loading, Playing, PausedUser, Autopause, Error, Stopped }
pub enum SessionEvent { Assign, Loaded, LoadFailed, Reselect, UserPause, UserResume,
                        AutoPauseTrigger, AutoPauseClear, Quit }
pub fn transfer(state: SessionState, event: SessionEvent) -> SessionState;  // 纯函数，UI 不要自己算状态

pub struct LibraryEntry { pub id: String, pub kind: WallpaperKind, pub title: String,
                          pub origin: String, pub source_dir: String }
pub enum WallpaperKind { Video, Image, Shader, Html }
pub struct AppConfig { pub monitors: MonitorMap, pub settings: Settings }
```

## 3. 路径与 URL（不要自己拼路径）

```rust
// crates/app/src/protocol.rs
pub fn config_dir() -> PathBuf;      // ~/Library/Application Support/Gesso
pub fn library_dir() -> PathBuf;     // <config>/library/<entry_id>/index.{mp4,html,…}
pub fn assets_dir() -> PathBuf;      // 宿主页与内置样例
```

## 4. UI → 引擎的调用方式（已实现）

- 引擎把 `SessionManager` 放进 `gpui` 全局状态（`crates/app/src/engine.rs`：`cx.set_global(engine::AppState::new(sm))`）
- 读：`main.rs::snapshot_ui(&sm)` 生成 `ui::app_state::GessoState`（UI 投影），启动注入 + 轮询回灌（保留 tab/selected/query/filter 等浏览状态）
- 写：UI 只 `engine::enqueue(EngineAction::…)`；引擎 150ms 轮询 drain 执行（与托盘同一通道，托盘菜单也已统一走该队列）
  - 动作集：`Assign` / `PauseAll` / `PauseOne` / `SyncMonitors` / `CycleMain` / `Import` / `ImportWe` / `UpdateSettings` / `SetAutostart` / `Remove` / `SetMonitorFps` / `FocusMainWindow`（另有引擎内部的 `ThumbsDone`，UI 不入队）
- **禁止** UI 直接创建/销毁壁纸窗口；`sync_monitors` 负责一切窗口生命周期

## 4.1 类型与命令的单一事实源

- 类型 ↔ 扩展名 ↔ MIME ↔ 缩略图策略：查 `gesso_core::content::{content_type, kind_from_ext, mime_for_ext}`，**不要**在 app/ui 里另列扩展名清单。
- 引擎 → 宿主页命令：构造 `HostCommand`（`app/host_cmd.rs`），序列化只在 `to_js`；不要手拼 `__gesso.xxx(...)` 字符串。
- 宿主页 WebGPU/WebGL 上下文以 `preserveDrawingBuffer: true` 创建（缩略图采集依赖跨任务 toDataURL，见工程笔记 #41）；缩略图采集的注入 JS 由 `capture.rs` 持有（Rust 侧字符串），宿主页只承载壁纸运行时契约。

## 5. 已知限制（别在这上面浪费轮次）

| 事项 | 状态 |
|---|---|
| `gesso://` 自定义协议 | **已打通**（lb-wry ≥0.53，`bf3c294`）：宿主页走 `gesso://host`，条目资源走 `gesso://library/<entry>/…`；视频支持 Range(206)（惰性切片 + 开放范围 512KB 部分响应，M4-W）。零拷贝直引已上线（`gesso://steam/<entry-id>`，M6） |
| 壁纸窗口 | 纯 AppKit 非 GPUI 窗口（M1.5 定稿），UI 无法也不应嵌入它 |
| 渲染器 | 四类全部生效：video / image（gif/webp/jpg/jpeg/png/avif）/ shader（WebGL2 + Shadertoy 子集）/ html（沙箱 iframe）；静态图 Direct 直引源文件，其余类型有真实采集的静态 + hover 帧 |
| 显示器热插拔 | 2s 轮询 diff（平台通知未接，候选 M5+） |
| 单屏暂停 | 已接 `pause_one`（状态机 UserPause/UserResume）；**自动暂停事件源已接（M5）**：全屏 CGWindowList 轮询 + 电池 IOKit，经 `apply_autopause` 驱动；光标 feed 与空闲降帧已接（M5）：NSEvent 位置 + HID 按键/空闲，纯轮询无需授权 |
| 导入 I/O 失败反馈 | 引擎侧仅日志；UI 预检（扩展名）已给红字/气泡，拷贝失败暂静默 |
