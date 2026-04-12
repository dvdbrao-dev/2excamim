//! Adapter-facing boundaries for provider integrations.
//!
//! Concrete providers should remain behind this module and map into canonical records before
//! crossing public crate boundaries.

pub mod polymarket;
