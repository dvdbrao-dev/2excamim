use core::fmt;

use crate::{codecs::CodecError, projections::ProjectionError, store::StoreError};

#[derive(Debug)]
pub enum ObservabilityError {
    Store(StoreError),
    Projection(ProjectionError),
    Codec(CodecError),
}

impl fmt::Display for ObservabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(source) => write!(f, "observability store error: {source}"),
            Self::Projection(source) => write!(f, "observability projection error: {source}"),
            Self::Codec(source) => write!(f, "observability codec error: {source}"),
        }
    }
}

impl std::error::Error for ObservabilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(source) => Some(source),
            Self::Projection(source) => Some(source),
            Self::Codec(source) => Some(source),
        }
    }
}

impl From<StoreError> for ObservabilityError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ProjectionError> for ObservabilityError {
    fn from(value: ProjectionError) -> Self {
        Self::Projection(value)
    }
}

impl From<CodecError> for ObservabilityError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}
