use core::fmt;

use crate::{codecs::CodecError, commands::CommandError, queries::QueryError, store::StoreError};

#[derive(Debug)]
pub enum MaterializationError {
    Query(QueryError),
    Store(StoreError),
    Command(CommandError),
    Codec(CodecError),
    InvalidState(String),
    Serde(serde_json::Error),
}

impl MaterializationError {
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState(message.into())
    }
}

impl fmt::Display for MaterializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(error) => write!(f, "materialization query error: {error}"),
            Self::Store(error) => write!(f, "materialization store error: {error}"),
            Self::Command(error) => write!(f, "materialization command error: {error}"),
            Self::Codec(error) => write!(f, "materialization codec error: {error}"),
            Self::InvalidState(message) => write!(f, "materialization invalid state: {message}"),
            Self::Serde(error) => write!(f, "materialization serialization error: {error}"),
        }
    }
}

impl std::error::Error for MaterializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Query(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Command(error) => Some(error),
            Self::Codec(error) => Some(error),
            Self::InvalidState(_) => None,
            Self::Serde(error) => Some(error),
        }
    }
}

impl From<QueryError> for MaterializationError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}

impl From<StoreError> for MaterializationError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<CommandError> for MaterializationError {
    fn from(value: CommandError) -> Self {
        Self::Command(value)
    }
}

impl From<CodecError> for MaterializationError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<serde_json::Error> for MaterializationError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}
