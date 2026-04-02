use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventError {
    ValidationError(String),
    InvariantError(String),
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationError(message) => write!(f, "validation error: {message}"),
            Self::InvariantError(message) => write!(f, "invariant error: {message}"),
        }
    }
}

impl std::error::Error for EventError {}
