//! Mapping contracts from adapter-local payloads into canonical market-domain values.

pub mod polymarket;

use crate::storage::{CanonicalActivityBatch, CanonicalSnapshotBatch, RawPayloadRecord};

/// Maps an adapter-local payload into canonical snapshot records.
pub trait MapToSnapshots {
    /// Adapter-local input type.
    type Input;

    /// Performs the canonical mapping.
    fn map_snapshots(&self, input: &Self::Input) -> Result<CanonicalSnapshotBatch, MappingError>;
}

/// Maps an adapter-local payload into canonical activity records.
pub trait MapToActivity {
    /// Adapter-local input type.
    type Input;

    /// Performs the canonical mapping.
    fn map_activity(&self, input: &Self::Input) -> Result<CanonicalActivityBatch, MappingError>;
}

/// Maps an adapter-local payload into a storable raw record.
pub trait MapToRawPayload {
    /// Adapter-local input type.
    type Input;

    /// Performs the raw payload mapping.
    fn map_raw_payload(&self, input: &Self::Input) -> Result<RawPayloadRecord, MappingError>;
}

/// Error returned by canonical mappers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingError {
    message: String,
}

impl MappingError {
    /// Creates a new mapping error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the mapping error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl core::fmt::Display for MappingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "mapping error: {}", self.message)
    }
}

impl std::error::Error for MappingError {}
