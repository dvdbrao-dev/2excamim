//! Provider-agnostic market context enrichment port and small result types.

use chrono::{DateTime, Utc};
use market_domain::MarketSource;
use serde::{Deserialize, Serialize};

/// Canonical request for market context enrichment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketContextRequest {
    /// Normalized market source.
    pub source: MarketSource,
    /// Canonical market identifier.
    pub market_id: String,
    /// Optional time anchor for point-in-time context.
    pub observed_at: Option<DateTime<Utc>>,
}

impl MarketContextRequest {
    /// Creates a minimal market context request.
    pub fn new(source: MarketSource, market_id: impl Into<String>) -> Self {
        Self {
            source,
            market_id: market_id.into(),
            observed_at: None,
        }
    }
}

/// Canonical category for a context entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketContextKind {
    /// Short deterministic summary or note.
    Summary,
    /// Supporting reference or citation-like item.
    Reference,
    /// Additional structured observation.
    Observation,
}

/// Canonical market context entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketContextEntry {
    /// Entry category.
    pub kind: MarketContextKind,
    /// Short human-readable title.
    pub title: String,
    /// Compact context body.
    pub body: String,
    /// Optional external reference.
    pub reference: Option<String>,
    /// UTC timestamp associated with the entry.
    pub observed_at: DateTime<Utc>,
}

/// Canonical context enrichment result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketContextResult {
    /// Original enrichment request.
    pub request: MarketContextRequest,
    /// Enriched context entries.
    pub entries: Vec<MarketContextEntry>,
}

impl MarketContextResult {
    /// Creates an empty context result for the request.
    pub fn empty(request: MarketContextRequest) -> Self {
        Self {
            request,
            entries: Vec::new(),
        }
    }
}

/// No-op market context enricher for future integration points.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopMarketContextEnricher;

/// Enriches a canonical market with external or derived context.
pub trait EnrichMarketContext {
    /// Error type returned by the enricher.
    type Error;

    /// Enriches the market context for a canonical request.
    fn enrich_market_context(
        &self,
        request: &MarketContextRequest,
    ) -> Result<MarketContextResult, Self::Error>;
}

impl EnrichMarketContext for NoopMarketContextEnricher {
    type Error = core::convert::Infallible;

    fn enrich_market_context(
        &self,
        request: &MarketContextRequest,
    ) -> Result<MarketContextResult, Self::Error> {
        Ok(MarketContextResult::empty(request.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::{EnrichMarketContext, MarketContextRequest, NoopMarketContextEnricher};
    use market_domain::MarketSource;

    #[test]
    fn noop_enricher_returns_empty_context() {
        let enricher = NoopMarketContextEnricher;
        let request = MarketContextRequest::new(MarketSource::Polymarket, "0xabc");

        let result = enricher.enrich_market_context(&request).unwrap();

        assert_eq!(result.request, request);
        assert!(result.entries.is_empty());
    }
}
