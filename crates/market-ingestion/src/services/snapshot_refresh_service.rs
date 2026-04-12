use crate::{
    adapters::polymarket::{FetchPolymarketDiscoveryPayload, PolymarketAdapterError},
    mappers::polymarket::PolymarketSnapshotMapper,
    storage::{RawPayloadRecord, RawPayloadStore, SnapshotRepository, StorageError},
};
use market_domain::MarketSource;

/// Summary returned by a snapshot refresh workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotRefreshSummary {
    /// Number of provider markets fetched.
    pub fetched: usize,
    /// Number of canonical snapshots stored or updated.
    pub stored: usize,
    /// Number of provider rows that failed canonical mapping.
    pub failed: usize,
}

/// Fatal error for the snapshot refresh workflow.
#[derive(Debug)]
pub enum SnapshotRefreshError {
    Adapter(PolymarketAdapterError),
    RawStore(StorageError),
    SnapshotStore(StorageError),
}

impl core::fmt::Display for SnapshotRefreshError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Adapter(err) => write!(f, "{err}"),
            Self::RawStore(err) => write!(f, "{err}"),
            Self::SnapshotStore(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SnapshotRefreshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Adapter(err) => Some(err),
            Self::RawStore(err) => Some(err),
            Self::SnapshotStore(err) => Some(err),
        }
    }
}

/// Read-only refresh workflow for Polymarket snapshots.
pub struct PolymarketSnapshotRefreshService<A, R, S> {
    adapter: A,
    raw_payload_store: R,
    snapshot_repository: S,
    mapper: PolymarketSnapshotMapper,
}

impl<A, R, S> PolymarketSnapshotRefreshService<A, R, S> {
    /// Creates a new refresh workflow.
    pub fn new(adapter: A, raw_payload_store: R, snapshot_repository: S) -> Self {
        Self {
            adapter,
            raw_payload_store,
            snapshot_repository,
            mapper: PolymarketSnapshotMapper,
        }
    }
}

impl<A, R, S> PolymarketSnapshotRefreshService<A, R, S>
where
    A: FetchPolymarketDiscoveryPayload,
    R: RawPayloadStore,
    S: SnapshotRepository,
{
    /// Refreshes snapshots end-to-end from provider fetch to canonical upsert.
    pub fn refresh(&self) -> Result<SnapshotRefreshSummary, SnapshotRefreshError> {
        let payload = self
            .adapter
            .fetch_discovery_payload()
            .map_err(SnapshotRefreshError::Adapter)?;

        let raw_record = RawPayloadRecord {
            source: MarketSource::Polymarket,
            source_event_id: None,
            payload_kind: "polymarket.discovery".into(),
            payload: payload.raw_body.as_bytes().to_vec(),
            captured_at: payload.fetched_at,
        };
        self.raw_payload_store
            .append(&raw_record)
            .map_err(SnapshotRefreshError::RawStore)?;

        let fetched = payload.markets.len();
        let (snapshots, mapping_errors) = self.mapper.map_markets_lossy(&payload.markets);
        let mut stored = 0usize;

        for snapshot in &snapshots {
            if self
                .snapshot_repository
                .upsert_snapshot(snapshot)
                .map_err(SnapshotRefreshError::SnapshotStore)?
            {
                stored += 1;
            }
        }

        Ok(SnapshotRefreshSummary {
            fetched,
            stored,
            failed: mapping_errors.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use market_domain::{MarketSnapshot, MarketSource};

    use super::PolymarketSnapshotRefreshService;
    use crate::{
        adapters::polymarket::{
            FetchPolymarketDiscoveryPayload, PolymarketAdapterError, PolymarketDiscoveryPayload,
        },
        dto::polymarket::PolymarketMarketDto,
        storage::{RawPayloadRecord, RawPayloadStore, SnapshotRepository, StorageError},
    };

    struct FakePolymarketDiscoveryPayload {
        payload: PolymarketDiscoveryPayload,
    }

    #[derive(Default)]
    struct RecordingRawPayloadStore {
        records: std::cell::RefCell<Vec<RawPayloadRecord>>,
    }

    #[derive(Default)]
    struct RecordingSnapshotRepository {
        snapshots: std::cell::RefCell<Vec<MarketSnapshot>>,
    }

    impl FetchPolymarketDiscoveryPayload for FakePolymarketDiscoveryPayload {
        fn fetch_discovery_payload(
            &self,
        ) -> Result<PolymarketDiscoveryPayload, PolymarketAdapterError> {
            Ok(self.payload.clone())
        }
    }

    impl RawPayloadStore for RecordingRawPayloadStore {
        fn append(&self, record: &RawPayloadRecord) -> Result<(), StorageError> {
            self.records.borrow_mut().push(record.clone());
            Ok(())
        }

        fn read_all(&self) -> Result<Vec<RawPayloadRecord>, StorageError> {
            Ok(self.records.borrow().clone())
        }
    }

    impl SnapshotRepository for RecordingSnapshotRepository {
        fn upsert_snapshot(&self, snapshot: &MarketSnapshot) -> Result<bool, StorageError> {
            let mut snapshots = self.snapshots.borrow_mut();
            if let Some(existing) = snapshots.iter_mut().find(|stored| {
                stored.source == snapshot.source
                    && stored.market_id == snapshot.market_id
                    && stored.observed_at == snapshot.observed_at
            }) {
                if existing == snapshot {
                    return Ok(false);
                }

                *existing = snapshot.clone();
                return Ok(true);
            }

            snapshots.push(snapshot.clone());
            Ok(true)
        }

        fn list_snapshots(&self) -> Result<Vec<MarketSnapshot>, StorageError> {
            Ok(self.snapshots.borrow().clone())
        }

        fn find_snapshot(
            &self,
            source: MarketSource,
            market_id: &str,
            observed_at: chrono::DateTime<Utc>,
        ) -> Result<Option<MarketSnapshot>, StorageError> {
            Ok(self
                .snapshots
                .borrow()
                .iter()
                .find(|snapshot| {
                    snapshot.source == source
                        && snapshot.market_id == market_id
                        && snapshot.observed_at == observed_at
                })
                .cloned())
        }
    }

    #[test]
    fn snapshot_refresh_stores_raw_payload_and_upserts_snapshots() {
        let raw_store = RecordingRawPayloadStore::default();
        let snapshot_repo = RecordingSnapshotRepository::default();
        let service = PolymarketSnapshotRefreshService::new(
            FakePolymarketDiscoveryPayload {
                payload: PolymarketDiscoveryPayload {
                    raw_body:
                        r#"[{"condition_id":"0xabc","question":"Example market","active":true}]"#
                            .into(),
                    markets: vec![PolymarketMarketDto {
                        condition_id: "0xabc".into(),
                        question: "Example market".into(),
                        active: true,
                        closed: false,
                        archived: false,
                        accepting_orders: true,
                        best_bid: Some(0.41),
                        best_ask: Some(0.43),
                        last_trade_price: Some(0.42),
                        volume: Some(100.0),
                    }],
                    fetched_at: Utc::now(),
                },
            },
            raw_store,
            snapshot_repo,
        );

        let summary = service.refresh().unwrap();
        assert_eq!(summary.fetched, 1);
        assert_eq!(summary.stored, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(service.raw_payload_store.read_all().unwrap().len(), 1);
        assert_eq!(
            service.snapshot_repository.list_snapshots().unwrap().len(),
            1
        );
    }

    #[test]
    fn snapshot_refresh_counts_mapping_failures_without_aborting_valid_rows() {
        let service = PolymarketSnapshotRefreshService::new(
            FakePolymarketDiscoveryPayload {
                payload: PolymarketDiscoveryPayload {
                    raw_body: "[]".into(),
                    markets: vec![
                        PolymarketMarketDto {
                            condition_id: "0xgood".into(),
                            question: "Valid market".into(),
                            active: true,
                            closed: false,
                            archived: false,
                            accepting_orders: true,
                            best_bid: Some(0.41),
                            best_ask: Some(0.43),
                            last_trade_price: Some(0.42),
                            volume: Some(100.0),
                        },
                        PolymarketMarketDto {
                            condition_id: "0xbad".into(),
                            question: "Broken market".into(),
                            active: true,
                            closed: false,
                            archived: false,
                            accepting_orders: true,
                            best_bid: Some(0.91),
                            best_ask: Some(0.40),
                            last_trade_price: None,
                            volume: Some(1.0),
                        },
                    ],
                    fetched_at: Utc::now(),
                },
            },
            RecordingRawPayloadStore::default(),
            RecordingSnapshotRepository::default(),
        );

        let summary = service.refresh().unwrap();
        assert_eq!(summary.fetched, 2);
        assert_eq!(summary.stored, 1);
        assert_eq!(summary.failed, 1);
    }
}
