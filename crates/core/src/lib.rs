//! gesso-core：Gesso 的全部纯领域逻辑。
//!
//! 边界约束（编译期保证）：本 crate 不依赖任何平台/UI 框架，
//! 一切能纯函数化的逻辑（状态机、配置 diff、导入登记）都在这里，
//! `cargo test -p gesso-core` 秒级全量跑。

pub mod config;
pub mod content;
pub mod error;
pub mod library;
pub mod session;
pub mod spec;

pub use config::{AppConfig, MonitorDiff, MonitorFpsMap, PausePolicy, Settings, StartupBehavior};
pub use content::{
    content_type, is_animated_image_ext, kind_from_ext, mime_for_ext, ThumbStrategy,
};
pub use error::{GessoError, Result};
pub use library::{generate_id, LibraryEntry, LibraryManifest};
pub use session::{transfer, SessionEvent, SessionState};
pub use spec::{AudioPolicy, ContentSpec, Fit, SpecMeta, WallpaperKind};
