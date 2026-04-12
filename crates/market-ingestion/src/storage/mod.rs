//! Shared ingestion records and file-backed storage primitives.

mod activity_repository;
mod error;
mod raw_payload_store;
mod snapshot_repository;

use chrono::{DateTime, Utc};
use market_domain::{MarketActivity, MarketQuote, MarketSnapshot, MarketSource};
use serde::{Deserialize, Serialize};

pub use activity_repository::{ActivityRepository, FilesystemActivityRepository};
pub use error::StorageError;
pub use raw_payload_store::{FilesystemRawPayloadStore, RawPayloadStore};
pub use snapshot_repository::{FilesystemSnapshotRepository, SnapshotRepository};

/// Query used by fetching ports to scope market reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketQuery {
    /// Normalized market source.
    pub source: MarketSource,
    /// Optional canonical market identifier filter.
    pub market_id: Option<String>,
}

impl MarketQuery {
    /// Creates a source-scoped query.
    pub fn for_source(source: MarketSource) -> Self {
        Self {
            source,
            market_id: None,
        }
    }
}

/// Opaque cursor used for incremental fetches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchCursor {
    /// Adapter-defined cursor token.
    pub token: String,
}

impl FetchCursor {
    /// Creates a cursor token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

/// Raw payload captured at the ingestion boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPayloadRecord {
    /// Normalized market source.
    pub source: MarketSource,
    /// Upstream event or object identifier when available.
    pub source_event_id: Option<String>,
    /// Canonical category for the captured payload.
    pub payload_kind: String,
    /// Serialized raw payload bytes.
    pub payload: Vec<u8>,
    /// Capture timestamp in UTC.
    pub captured_at: DateTime<Utc>,
}

impl RawPayloadRecord {
    /// Validates the raw payload record.
    pub fn validate(&self) -> Result<(), StorageError> {
        validate_required_string(&self.payload_kind, "payload_kind")?;
        validate_optional_string(self.source_event_id.as_deref(), "source_event_id")?;

        Ok(())
    }
}

/// Batch of raw payload records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPayloadBatch {
    /// Captured payloads.
    pub records: Vec<RawPayloadRecord>,
}

impl RawPayloadBatch {
    /// Creates an empty batch.
    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
        }
    }
}

/// Batch of canonical market snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalSnapshotBatch {
    /// Snapshot records.
    pub snapshots: Vec<MarketSnapshot>,
}

impl CanonicalSnapshotBatch {
    /// Creates an empty snapshot batch.
    pub fn empty() -> Self {
        Self {
            snapshots: Vec::new(),
        }
    }
}

/// Batch of canonical market quotes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalQuoteBatch {
    /// Quote records.
    pub quotes: Vec<MarketQuote>,
}

impl CanonicalQuoteBatch {
    /// Creates an empty quote batch.
    pub fn empty() -> Self {
        Self { quotes: Vec::new() }
    }
}

/// Batch of canonical market activity records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalActivityBatch {
    /// Activity records.
    pub activity: Vec<MarketActivity>,
}

impl CanonicalActivityBatch {
    /// Creates an empty activity batch.
    pub fn empty() -> Self {
        Self {
            activity: Vec::new(),
        }
    }
}

pub(crate) fn validate_required_string(value: &str, field: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(StorageError::invalid_data(format!(
            "{field} cannot be empty"
        )));
    }

    Ok(())
}

pub(crate) fn validate_optional_string(
    value: Option<&str>,
    field: &str,
) -> Result<(), StorageError> {
    if let Some(value) = value {
        if value.trim().is_empty() {
            return Err(StorageError::invalid_data(format!(
                "{field} cannot be blank when provided"
            )));
        }
    }

    Ok(())
}
