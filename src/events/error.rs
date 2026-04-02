use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventError {
    ValidationError(String),
    InvariantError(String),
}

impl EventError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::ValidationError(message.into())
    }

    pub fn invariant(message: impl Into<String>) -> Self {
        Self::InvariantError(message.into())
    }

    pub fn message(&self) -> &str {
        match self {
            Self::ValidationError(message) | Self::InvariantError(message) => message,
        }
    }
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
