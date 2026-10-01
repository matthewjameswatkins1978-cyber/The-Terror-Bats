//! Error types for Terror Bat M1.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors surfaced to humans. Every message must be understandable without
/// reading Rust source (Constitution 14).
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot read `{path}`: {message}")]
    Io { path: PathBuf, message: String },

    #[error("invalid Bat Spec in `{path}`: {message}")]
    Spec { path: PathBuf, message: String },

    #[error("evidence store error at `{path}`: {message}")]
    Store { path: PathBuf, message: String },
}

impl Error {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Error::Io {
            path: path.to_path_buf(),
            message: source.to_string(),
        }
    }

    pub fn spec(path: &Path, message: impl Into<String>) -> Self {
        Error::Spec {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    pub fn store(path: &Path, message: impl Into<String>) -> Self {
        Error::Store {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
