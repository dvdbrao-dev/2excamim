use serde::Serialize;

use crate::{
    events::EventEnvelope,
    observability::{summary_from_store, ObservabilitySummary},
    projections::{DecisionProjection, SignalProjection},
    queries::QueryService,
    scenarios::{load_fixture_named, ReplayHarness, ReplayResult},
    store::{JsonlEventStore, StoredEvent},
};

use super::ApplicationError;

#[derive(Debug, Clone, Copy)]
pub struct EventAppService<'a> {
    store: &'a JsonlEventStore,
}

impl<'a> EventAppService<'a> {
    pub fn new(store: &'a JsonlEventStore) -> Self {
        Self { store }
    }

    pub fn append_stored_event(&self, event: &StoredEvent) -> Result<bool, ApplicationError> {
        Ok(self.store.append_event(event)?)
    }

    pub fn append_stored_events(&self, events: &[StoredEvent]) -> Result<usize, ApplicationError> {
        Ok(self.store.append_events(events)?)
    }

    pub fn append_envelope<T>(&self, event: &EventEnvelope<T>) -> Result<bool, ApplicationError>
    where
        T: Serialize,
    {
        let stored = StoredEvent::try_from(event)?;
        self.append_stored_event(&stored)
    }

    pub fn current_summary(&self) -> Result<ObservabilitySummary, ApplicationError> {
        Ok(summary_from_store(self.store)?)
    }

    pub fn signal_projection(
        &self,
        signal_id: &str,
    ) -> Result<Option<SignalProjection>, ApplicationError> {
        Ok(QueryService::new(self.store).signal_projection(signal_id)?)
    }

    pub fn decision_projection(
        &self,
        decision_id: &str,
    ) -> Result<Option<DecisionProjection>, ApplicationError> {
        Ok(QueryService::new(self.store).decision_projection(decision_id)?)
    }

    pub fn replay_fixture_by_name(&self, name: &str) -> Result<ReplayResult, ApplicationError> {
        let fixture = load_fixture_named(name)?;
        Ok(ReplayHarness::new(self.store).run_fixture(&fixture)?)
    }
}
