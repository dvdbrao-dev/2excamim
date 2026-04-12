//! Canonical domain types for prediction markets.

mod error;
mod events;
mod types;
mod validation;

pub use error::ValidationError;
pub use events::{
    MarketActivityReceived, MarketQuoteUpdated, MarketSignalGenerated, MarketSnapshotReceived,
};
pub use types::{
    MarketActivity, MarketActivityKind, MarketQuote, MarketSignal, MarketSignalDirection,
    MarketSnapshot, MarketSource, MarketStatus,
};
pub use validation::Validate;
