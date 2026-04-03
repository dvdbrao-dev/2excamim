use core::fmt;

use crate::{
    codecs::CodecError, observability::ObservabilityError, queries::QueryError,
    scenarios::ScenarioError, store::StoreError,
};

#[derive(Debug)]
pub enum ApplicationError {
    Store(StoreError),
    Query(QueryError),
    Observability(ObservabilityError),
    Scenario(ScenarioError),
    Codec(CodecError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(source) => write!(f, "application store error: {source}"),
            Self::Query(source) => write!(f, "application query error: {source}"),
            Self::Observability(source) => write!(f, "application observability error: {source}"),
            Self::Scenario(source) => write!(f, "application scenario error: {source}"),
            Self::Codec(source) => write!(f, "application codec error: {source}"),
        }
    }
}

impl std::error::Error for ApplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(source) => Some(source),
            Self::Query(source) => Some(source),
            Self::Observability(source) => Some(source),
            Self::Scenario(source) => Some(source),
            Self::Codec(source) => Some(source),
        }
    }
}

impl From<StoreError> for ApplicationError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<QueryError> for ApplicationError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}

impl From<ObservabilityError> for ApplicationError {
    fn from(value: ObservabilityError) -> Self {
        Self::Observability(value)
    }
}

impl From<ScenarioError> for ApplicationError {
    fn from(value: ScenarioError) -> Self {
        Self::Scenario(value)
    }
}

impl From<CodecError> for ApplicationError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}
