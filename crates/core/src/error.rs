//! 统一错误类型。

use thiserror::Error;

pub type Result<T> = std::result::Result<T, GessoError>;

#[derive(Debug, Error)]
pub enum GessoError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("序列化错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("不支持的平台：{0}")]
    UnsupportedPlatform(String),

    #[error("无效的内容规格：{0}")]
    InvalidSpec(String),

    #[error("库条目不存在：{0}")]
    EntryNotFound(String),
}
