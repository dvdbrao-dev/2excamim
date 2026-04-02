mod error;
mod jsonl_store;

pub use error::StoreError;
pub use jsonl_store::{JsonlEventStore, StoredEvent};
