//! Ingestion ports and compile-time scaffolding for prediction market data.

pub mod adapters;
pub mod application;
pub mod dto;
pub mod mappers;
pub mod ports;
pub mod services;
pub mod storage;

pub use application::{
    ActivityObservationSummary, MarketWatchConfig, MarketWatchError, MarketWatchObservationSummary,
    MarketWatchRunner, SnapshotObservationSummary,
};
pub use ports::{
    EnrichMarketContext, FetchMarketActivity, FetchMarketQuotes, FetchMarketSnapshots,
    MarketContextEntry, MarketContextKind, MarketContextRequest, MarketContextResult,
    NoopMarketContextEnricher, StoreCanonicalActivity, StoreCanonicalSnapshots, StoreRawPayloads,
};
pub use services::MarketIngestionService;
pub use storage::{
    ActivityRepository, CanonicalActivityBatch, CanonicalQuoteBatch, CanonicalSnapshotBatch,
    FetchCursor, FilesystemActivityRepository, FilesystemRawPayloadStore,
    FilesystemSnapshotRepository, MarketQuery, RawPayloadBatch, RawPayloadRecord, RawPayloadStore,
    SnapshotRepository, StorageError,
};
