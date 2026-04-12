//! Small application-facing runners built on top of ingestion services.

pub mod market_watch_runner;

pub use market_watch_runner::{
    ActivityObservationSummary, MarketWatchConfig, MarketWatchError, MarketWatchObservationSummary,
    MarketWatchRunner, SnapshotObservationSummary,
};
