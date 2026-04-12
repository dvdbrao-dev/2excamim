//! Public ingestion ports defined in canonical market-domain terms.

pub mod context;

use crate::storage::{
    CanonicalActivityBatch, CanonicalQuoteBatch, CanonicalSnapshotBatch, FetchCursor, MarketQuery,
    RawPayloadBatch,
};
pub use context::{
    EnrichMarketContext, MarketContextEntry, MarketContextKind, MarketContextRequest,
    MarketContextResult, NoopMarketContextEnricher,
};

/// Fetches canonical market snapshots from an upstream source.
pub trait FetchMarketSnapshots {
    /// Error type returned by the fetcher.
    type Error;

    /// Fetches snapshot data for the given query and optional cursor.
    fn fetch_market_snapshots(
        &self,
        query: &MarketQuery,
        cursor: Option<&FetchCursor>,
    ) -> Result<CanonicalSnapshotBatch, Self::Error>;
}

/// Fetches canonical market quotes from an upstream source.
pub trait FetchMarketQuotes {
    /// Error type returned by the fetcher.
    type Error;

    /// Fetches quote data for the given query and optional cursor.
    fn fetch_market_quotes(
        &self,
        query: &MarketQuery,
        cursor: Option<&FetchCursor>,
    ) -> Result<CanonicalQuoteBatch, Self::Error>;
}

/// Fetches canonical market activity from an upstream source.
pub trait FetchMarketActivity {
    /// Error type returned by the fetcher.
    type Error;

    /// Fetches activity data for the given query and optional cursor.
    fn fetch_market_activity(
        &self,
        query: &MarketQuery,
        cursor: Option<&FetchCursor>,
    ) -> Result<CanonicalActivityBatch, Self::Error>;
}

/// Stores raw ingestion payloads for audit or replay.
pub trait StoreRawPayloads {
    /// Error type returned by the store.
    type Error;

    /// Stores a batch of raw payload records.
    fn store_raw_payloads(&self, batch: &RawPayloadBatch) -> Result<(), Self::Error>;
}

/// Stores canonical snapshot and quote data.
pub trait StoreCanonicalSnapshots {
    /// Error type returned by the store.
    type Error;

    /// Stores a batch of canonical snapshots and quotes.
    fn store_canonical_snapshots(&self, batch: &CanonicalSnapshotBatch) -> Result<(), Self::Error>;
}

/// Stores canonical market activity.
pub trait StoreCanonicalActivity {
    /// Error type returned by the store.
    type Error;

    /// Stores a batch of canonical activity records.
    fn store_canonical_activity(&self, batch: &CanonicalActivityBatch) -> Result<(), Self::Error>;
}
