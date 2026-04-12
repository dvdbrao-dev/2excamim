use core::fmt;
use std::io;

/// Error returned by ingestion storage primitives.
#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Serde(serde_json::Error),
    InvalidData(String),
}

impl StorageError {
    /// Creates a typed invalid-data error.
    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "storage io error: {error}"),
            Self::Serde(error) => write!(f, "storage serialization error: {error}"),
            Self::InvalidData(message) => write!(f, "storage invalid data: {message}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serde(error) => Some(error),
            Self::InvalidData(_) => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}
