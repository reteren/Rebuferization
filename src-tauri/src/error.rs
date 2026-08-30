//! Crate-wide error type. Every fallible module returns `AppResult`.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("image: {0}")]
    Image(#[from] image::ImageError),

    #[error("clipboard is busy")]
    ClipboardBusy,

    #[error("item {0} not found")]
    NotFound(i64),

    #[error("item exceeds the size limit ({0} bytes)")]
    TooLarge(u64),

    #[error("windows api: {0}")]
    Win(String),

    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// Tauri commands must return something `Serialize`; errors cross the IPC
/// boundary as a plain string so the frontend can show them directly.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

impl From<windows::core::Error> for AppError {
    fn from(e: windows::core::Error) -> Self {
        AppError::Win(e.to_string())
    }
}
