use core::fmt;

use crate::{
    handoff::HandoffError, materialization::MaterializationError, observability::ObservabilityError,
};

#[derive(Debug)]
pub enum BatchRunnerError {
    Ingest(HandoffError),
    Materialization(MaterializationError),
    Summary(ObservabilityError),
}

impl fmt::Display for BatchRunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ingest(error) => write!(f, "batch phase ingest failed: {error}"),
            Self::Materialization(error) => {
                write!(f, "batch phase materialize failed: {error}")
            }
            Self::Summary(error) => write!(f, "batch phase summary failed: {error}"),
        }
    }
}

impl std::error::Error for BatchRunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Ingest(error) => Some(error),
            Self::Materialization(error) => Some(error),
            Self::Summary(error) => Some(error),
        }
    }
}

impl From<HandoffError> for BatchRunnerError {
    fn from(value: HandoffError) -> Self {
        Self::Ingest(value)
    }
}

impl From<MaterializationError> for BatchRunnerError {
    fn from(value: MaterializationError) -> Self {
        Self::Materialization(value)
    }
}

impl From<ObservabilityError> for BatchRunnerError {
    fn from(value: ObservabilityError) -> Self {
        Self::Summary(value)
    }
}
