use core::fmt;

use crate::events::EventError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    Validation(String),
    Invariant(String),
}

impl CommandError {
    pub fn message(&self) -> &str {
        match self {
            Self::Validation(message) | Self::Invariant(message) => message,
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => write!(f, "command validation error: {message}"),
            Self::Invariant(message) => write!(f, "command invariant error: {message}"),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<EventError> for CommandError {
    fn from(value: EventError) -> Self {
        match value {
            EventError::ValidationError(message) => Self::Validation(message),
            EventError::InvariantError(message) => Self::Invariant(message),
        }
    }
}
