use core::fmt;
use std::{io, path::PathBuf, string::FromUtf8Error};

use crate::{events::EventError, store::StoreError};

#[derive(Debug)]
pub enum HandoffError {
    Io(io::Error),
    Store(StoreError),
    Event(EventError),
    Serde(serde_json::Error),
    Utf8(FromUtf8Error),
    InvalidInput(String),
    UnsupportedFormat { path: PathBuf, message: String },
    DecoderFailed(String),
}

impl HandoffError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn unsupported_format(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "handoff io error: {error}"),
            Self::Store(error) => write!(f, "handoff store error: {error}"),
            Self::Event(error) => write!(f, "handoff event error: {error}"),
            Self::Serde(error) => write!(f, "handoff serialization error: {error}"),
            Self::Utf8(error) => write!(f, "handoff utf8 error: {error}"),
            Self::InvalidInput(message) => write!(f, "handoff invalid input: {message}"),
            Self::UnsupportedFormat { path, message } => {
                write!(
                    f,
                    "handoff unsupported format for {}: {message}",
                    path.display()
                )
            }
            Self::DecoderFailed(message) => write!(f, "handoff decoder failed: {message}"),
        }
    }
}

impl std::error::Error for HandoffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Event(error) => Some(error),
            Self::Serde(error) => Some(error),
            Self::Utf8(error) => Some(error),
            Self::InvalidInput(_) | Self::UnsupportedFormat { .. } | Self::DecoderFailed(_) => None,
        }
    }
}

impl From<io::Error> for HandoffError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<StoreError> for HandoffError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<EventError> for HandoffError {
    fn from(value: EventError) -> Self {
        Self::Event(value)
    }
}

impl From<serde_json::Error> for HandoffError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

impl From<FromUtf8Error> for HandoffError {
    fn from(value: FromUtf8Error) -> Self {
        Self::Utf8(value)
    }
}
