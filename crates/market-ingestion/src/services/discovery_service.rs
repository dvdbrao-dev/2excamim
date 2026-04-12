use crate::{
    adapters::polymarket::{FetchPolymarketDiscovery, PolymarketAdapterError},
    mappers::{polymarket::PolymarketSnapshotMapper, MappingError},
    storage::CanonicalSnapshotBatch,
};

/// Typed error for Polymarket discovery reads.
#[derive(Debug)]
pub enum PolymarketDiscoveryError {
    Adapter(PolymarketAdapterError),
    Mapping(MappingError),
}

impl core::fmt::Display for PolymarketDiscoveryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Adapter(err) => write!(f, "{err}"),
            Self::Mapping(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PolymarketDiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Adapter(err) => Some(err),
            Self::Mapping(err) => Some(err),
        }
    }
}

/// Small read-only service for Polymarket market discovery.
pub struct PolymarketDiscoveryService<A> {
    adapter: A,
    mapper: PolymarketSnapshotMapper,
}

impl<A> PolymarketDiscoveryService<A> {
    /// Creates a new Polymarket discovery service.
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            mapper: PolymarketSnapshotMapper,
        }
    }
}

impl<A> PolymarketDiscoveryService<A>
where
    A: FetchPolymarketDiscovery,
{
    /// Fetches Polymarket discovery data and maps it into canonical snapshots.
    pub fn discover_snapshots(&self) -> Result<CanonicalSnapshotBatch, PolymarketDiscoveryError> {
        let markets = self
            .adapter
            .fetch_discovery()
            .map_err(PolymarketDiscoveryError::Adapter)?;

        self.mapper
            .map_markets(&markets)
            .map_err(PolymarketDiscoveryError::Mapping)
    }
}

#[cfg(test)]
mod tests {
    use super::{PolymarketDiscoveryError, PolymarketDiscoveryService};
    use crate::{
        adapters::polymarket::{FetchPolymarketDiscovery, PolymarketAdapterError},
        dto::polymarket::PolymarketMarketDto,
    };

    struct FakePolymarketDiscovery {
        markets: Vec<PolymarketMarketDto>,
    }

    impl FetchPolymarketDiscovery for FakePolymarketDiscovery {
        fn fetch_discovery(&self) -> Result<Vec<PolymarketMarketDto>, PolymarketAdapterError> {
            Ok(self.markets.clone())
        }
    }

    #[test]
    fn polymarket_discovery_service_maps_successfully() {
        let service = PolymarketDiscoveryService::new(FakePolymarketDiscovery {
            markets: vec![PolymarketMarketDto {
                condition_id: "0xabc".into(),
                question: "Will ETH close above 5k?".into(),
                active: true,
                closed: false,
                archived: false,
                accepting_orders: true,
                best_bid: Some(0.41),
                best_ask: Some(0.43),
                last_trade_price: Some(0.42),
                volume: Some(1500.0),
            }],
        });

        let batch = service.discover_snapshots().unwrap();
        assert_eq!(batch.snapshots.len(), 1);
        assert_eq!(batch.snapshots[0].market_id, "0xabc");
    }

    #[test]
    fn polymarket_discovery_service_surfaces_mapping_failure() {
        let service = PolymarketDiscoveryService::new(FakePolymarketDiscovery {
            markets: vec![PolymarketMarketDto {
                condition_id: "0xabc".into(),
                question: "Broken market".into(),
                active: true,
                closed: false,
                archived: false,
                accepting_orders: true,
                best_bid: Some(0.91),
                best_ask: Some(0.40),
                last_trade_price: None,
                volume: Some(1.0),
            }],
        });

        let err = service.discover_snapshots().unwrap_err();
        assert!(matches!(err, PolymarketDiscoveryError::Mapping(_)));
    }
}
