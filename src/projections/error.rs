use core::fmt;

use crate::codecs::CodecError;

#[derive(Debug)]
pub enum ProjectionError {
    Codec(CodecError),
    InvalidState(String),
}

impl ProjectionError {
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState(message.into())
    }
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(source) => write!(f, "projection codec error: {source}"),
            Self::InvalidState(message) => write!(f, "projection invalid state: {message}"),
        }
    }
}

impl std::error::Error for ProjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(source) => Some(source),
            Self::InvalidState(_) => None,
        }
    }
}

impl From<CodecError> for ProjectionError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}
