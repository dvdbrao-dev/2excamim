use core::fmt;

use crate::{codecs::CodecError, projections::ProjectionError, store::StoreError};

#[derive(Debug)]
pub enum QueryError {
    Store(StoreError),
    Projection(ProjectionError),
    Codec(CodecError),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(source) => write!(f, "query store error: {source}"),
            Self::Projection(source) => write!(f, "query projection error: {source}"),
            Self::Codec(source) => write!(f, "query codec error: {source}"),
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(source) => Some(source),
            Self::Projection(source) => Some(source),
            Self::Codec(source) => Some(source),
        }
    }
}

impl From<StoreError> for QueryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ProjectionError> for QueryError {
    fn from(value: ProjectionError) -> Self {
        Self::Projection(value)
    }
}

impl From<CodecError> for QueryError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}
