use core::fmt;

use crate::{projections::ProjectionError, queries::QueryError, store::StoreError};

#[derive(Debug)]
pub enum ScenarioError {
    Store(StoreError),
    Query(QueryError),
    Projection(ProjectionError),
    UnknownFixture(String),
}

impl ScenarioError {
    pub fn unknown_fixture(name: impl Into<String>) -> Self {
        Self::UnknownFixture(name.into())
    }
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(source) => write!(f, "scenario store error: {source}"),
            Self::Query(source) => write!(f, "scenario query error: {source}"),
            Self::Projection(source) => write!(f, "scenario projection error: {source}"),
            Self::UnknownFixture(name) => write!(f, "unknown scenario fixture: {name}"),
        }
    }
}

impl std::error::Error for ScenarioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(source) => Some(source),
            Self::Query(source) => Some(source),
            Self::Projection(source) => Some(source),
            Self::UnknownFixture(_) => None,
        }
    }
}

impl From<StoreError> for ScenarioError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<QueryError> for ScenarioError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}

impl From<ProjectionError> for ScenarioError {
    fn from(value: ProjectionError) -> Self {
        Self::Projection(value)
    }
}
