//! gesso-core：Gesso 的全部纯领域逻辑。
//!
//! 边界约束（编译期保证）：本 crate 不依赖任何平台/UI 框架，
//! 一切能纯函数化的逻辑（状态机、配置 diff、导入登记）都在这里，
//! `cargo test -p gesso-core` 秒级全量跑。

pub mod adoption;
pub mod config;
pub mod content;
pub mod error;
pub mod library;
pub mod pet;
pub mod pet_save;
pub mod session;
pub mod spec;

pub use adoption::{transition as adoption_transition, AdoptionEvent, AdoptionState, CandidateSet};
pub use config::{AppConfig, MonitorDiff, MonitorFpsMap, PausePolicy, Settings, StartupBehavior};
pub use content::{
    content_type, is_animated_image_ext, kind_from_ext, mime_for_ext, ThumbStrategy,
};
pub use error::{GessoError, Result};
pub use library::{generate_id, LibraryEntry, LibraryManifest};
pub use pet::{
    generate_candidate, generate_candidates, AppearanceParams, Candidate, CandidateGenerationError,
    PetName, PetSeed, PetSpecies, ASSET_VERSION, GENERATOR_VERSION,
};
pub use pet_save::{
    PetActivity, PetIndividual, PetInteraction, PetPosition, PetSave, PetSaveError, PetSize,
    PetStats, PET_SAVE_VERSION,
};
pub use session::{transfer, SessionEvent, SessionState};
pub use spec::{AudioPolicy, ContentSpec, Fit, SpecMeta, WallpaperKind};
