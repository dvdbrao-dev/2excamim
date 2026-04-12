use crate::{
    ports::{
        FetchMarketActivity, FetchMarketQuotes, FetchMarketSnapshots, StoreCanonicalActivity,
        StoreCanonicalSnapshots, StoreRawPayloads,
    },
    storage::{
        CanonicalActivityBatch, CanonicalQuoteBatch, CanonicalSnapshotBatch, FetchCursor,
        MarketQuery, RawPayloadBatch,
    },
};

/// Compile-time service facade for canonical market ingestion flows.
pub struct MarketIngestionService<FS, FQ, FA, SR, SS, SA> {
    snapshots: FS,
    quotes: FQ,
    activity: FA,
    raw_store: SR,
    snapshot_store: SS,
    activity_store: SA,
}

impl<FS, FQ, FA, SR, SS, SA> MarketIngestionService<FS, FQ, FA, SR, SS, SA> {
    /// Creates a new service facade from concrete ports.
    pub fn new(
        snapshots: FS,
        quotes: FQ,
        activity: FA,
        raw_store: SR,
        snapshot_store: SS,
        activity_store: SA,
    ) -> Self {
        Self {
            snapshots,
            quotes,
            activity,
            raw_store,
            snapshot_store,
            activity_store,
        }
    }
}

impl<FS, FQ, FA, SR, SS, SA> MarketIngestionService<FS, FQ, FA, SR, SS, SA>
where
    FS: FetchMarketSnapshots,
    FQ: FetchMarketQuotes,
    FA: FetchMarketActivity,
    SR: StoreRawPayloads,
    SS: StoreCanonicalSnapshots,
    SA: StoreCanonicalActivity,
{
    /// Fetches and returns canonical snapshots.
    pub fn fetch_snapshots(
        &self,
        query: &MarketQuery,
        cursor: Option<&FetchCursor>,
    ) -> Result<CanonicalSnapshotBatch, FS::Error> {
        self.snapshots.fetch_market_snapshots(query, cursor)
    }

    /// Fetches and returns canonical quotes.
    pub fn fetch_quotes(
        &self,
        query: &MarketQuery,
        cursor: Option<&FetchCursor>,
    ) -> Result<CanonicalQuoteBatch, FQ::Error> {
        self.quotes.fetch_market_quotes(query, cursor)
    }

    /// Fetches and returns canonical activity.
    pub fn fetch_activity(
        &self,
        query: &MarketQuery,
        cursor: Option<&FetchCursor>,
    ) -> Result<CanonicalActivityBatch, FA::Error> {
        self.activity.fetch_market_activity(query, cursor)
    }

    /// Stores raw payload records.
    pub fn store_raw_payloads(&self, batch: &RawPayloadBatch) -> Result<(), SR::Error> {
        self.raw_store.store_raw_payloads(batch)
    }

    /// Stores canonical snapshots and quotes.
    pub fn store_snapshots(&self, batch: &CanonicalSnapshotBatch) -> Result<(), SS::Error> {
        self.snapshot_store.store_canonical_snapshots(batch)
    }

    /// Stores canonical activity.
    pub fn store_activity(&self, batch: &CanonicalActivityBatch) -> Result<(), SA::Error> {
        self.activity_store.store_canonical_activity(batch)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ports::{
            FetchMarketActivity, FetchMarketQuotes, FetchMarketSnapshots, StoreCanonicalActivity,
            StoreCanonicalSnapshots, StoreRawPayloads,
        },
        storage::{
            CanonicalActivityBatch, CanonicalQuoteBatch, CanonicalSnapshotBatch, FetchCursor,
            MarketQuery, RawPayloadBatch,
        },
    };
    use market_domain::MarketSource;

    use super::MarketIngestionService;

    struct NoopSnapshots;
    struct NoopQuotes;
    struct NoopActivity;
    struct NoopRawStore;
    struct NoopSnapshotStore;
    struct NoopActivityStore;

    impl FetchMarketSnapshots for NoopSnapshots {
        type Error = core::convert::Infallible;

        fn fetch_market_snapshots(
            &self,
            _query: &MarketQuery,
            _cursor: Option<&FetchCursor>,
        ) -> Result<CanonicalSnapshotBatch, Self::Error> {
            Ok(CanonicalSnapshotBatch::empty())
        }
    }

    impl FetchMarketQuotes for NoopQuotes {
        type Error = core::convert::Infallible;

        fn fetch_market_quotes(
            &self,
            _query: &MarketQuery,
            _cursor: Option<&FetchCursor>,
        ) -> Result<CanonicalQuoteBatch, Self::Error> {
            Ok(CanonicalQuoteBatch::empty())
        }
    }

    impl FetchMarketActivity for NoopActivity {
        type Error = core::convert::Infallible;

        fn fetch_market_activity(
            &self,
            _query: &MarketQuery,
            _cursor: Option<&FetchCursor>,
        ) -> Result<CanonicalActivityBatch, Self::Error> {
            Ok(CanonicalActivityBatch::empty())
        }
    }

    impl StoreRawPayloads for NoopRawStore {
        type Error = core::convert::Infallible;

        fn store_raw_payloads(&self, _batch: &RawPayloadBatch) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl StoreCanonicalSnapshots for NoopSnapshotStore {
        type Error = core::convert::Infallible;

        fn store_canonical_snapshots(
            &self,
            _batch: &CanonicalSnapshotBatch,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl StoreCanonicalActivity for NoopActivityStore {
        type Error = core::convert::Infallible;

        fn store_canonical_activity(
            &self,
            _batch: &CanonicalActivityBatch,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn service_compiles_with_small_port_surface() {
        let service = MarketIngestionService::new(
            NoopSnapshots,
            NoopQuotes,
            NoopActivity,
            NoopRawStore,
            NoopSnapshotStore,
            NoopActivityStore,
        );
        let query = MarketQuery::for_source(MarketSource::Polymarket);

        let snapshots = service.fetch_snapshots(&query, None).unwrap();
        let quotes = service.fetch_quotes(&query, None).unwrap();
        let activity = service.fetch_activity(&query, None).unwrap();

        assert!(snapshots.snapshots.is_empty());
        assert!(quotes.quotes.is_empty());
        assert!(activity.activity.is_empty());
    }
}
