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

## 4. UI → 引擎的调用方式（M3 接线）

UI 现在拿不到 `SessionManager`（它活在 `main.rs` 的 gpui 闭包里）。接线方式（由引擎侧提供）：

- 引擎把 `SessionManager` 放进 `gpui` 全局状态（`cx.set_global(AppState::new(sm))`）
- UI 用 `cx.global::<AppState>()` 读快照；写操作用 `cx.update_global` + 动作队列（引擎 150ms 轮询执行，与托盘同一通道）
- **禁止** UI 直接创建/销毁壁纸窗口；`sync_monitors` 负责一切窗口生命周期

## 5. 已知限制（别在这上面浪费轮次）

| 事项 | 状态 |
|---|---|
| `gesso://` 自定义协议 | 本版 lb-wry/WKWebView 零回调，M2 走条目自包含 `file://`；M3 由引擎侧修 |
| 壁纸窗口 | 纯 AppKit 非 GPUI 窗口（M1.5 定稿），UI 无法也不应嵌入它 |
| 渲染器 | 目前仅 video/image 生效；shader/html 在 M4 |
| 显示器热插拔 | 2s 轮询 diff（平台通知 M3 后接） |
