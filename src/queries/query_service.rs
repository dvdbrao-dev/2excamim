use crate::{
    projections::{
        build_decision_projections, build_signal_projections, DecisionProjection, SignalProjection,
    },
    store::{JsonlEventStore, StoredEvent},
};

use super::QueryError;

#[derive(Debug, Clone, Copy)]
pub struct QueryService<'a> {
    store: &'a JsonlEventStore,
}

impl<'a> QueryService<'a> {
    pub fn new(store: &'a JsonlEventStore) -> Self {
        Self { store }
    }

    pub fn all_events(&self) -> Result<Vec<StoredEvent>, QueryError> {
        Ok(self.store.read_all()?)
    }

    pub fn signal_projection(
        &self,
        signal_id: &str,
    ) -> Result<Option<SignalProjection>, QueryError> {
        Ok(self
            .signal_projections()?
            .into_iter()
            .find(|projection| projection.signal_id == signal_id))
    }

    pub fn decision_projection(
        &self,
        decision_id: &str,
    ) -> Result<Option<DecisionProjection>, QueryError> {
        Ok(self
            .decision_projections()?
            .into_iter()
            .find(|projection| projection.decision_id == decision_id))
    }

    pub fn timeline_for_signal(&self, signal_id: &str) -> Result<Vec<StoredEvent>, QueryError> {
        Ok(self.store.find_by_signal_id(signal_id)?)
    }

    pub fn timeline_for_decision(&self, decision_id: &str) -> Result<Vec<StoredEvent>, QueryError> {
        Ok(self.store.find_by_decision_id(decision_id)?)
    }

    pub fn timeline_for_correlation(
        &self,
        correlation_id: &str,
    ) -> Result<Vec<StoredEvent>, QueryError> {
        Ok(self.store.find_by_correlation_id(correlation_id)?)
    }

    pub fn confirmed_signals(&self) -> Result<Vec<SignalProjection>, QueryError> {
        Ok(self
            .signal_projections()?
            .into_iter()
            .filter(|projection| projection.confirmed)
            .collect())
    }

    pub fn vetoed_signals(&self) -> Result<Vec<SignalProjection>, QueryError> {
        Ok(self
            .signal_projections()?
            .into_iter()
            .filter(|projection| projection.vetoed)
            .collect())
    }

    pub fn decisions_with_fills(&self) -> Result<Vec<DecisionProjection>, QueryError> {
        Ok(self
            .decision_projections()?
            .into_iter()
            .filter(|projection| projection.fills_count > 0)
            .collect())
    }

    pub fn decisions_without_fills(&self) -> Result<Vec<DecisionProjection>, QueryError> {
        Ok(self
            .decision_projections()?
            .into_iter()
            .filter(|projection| projection.fills_count == 0)
            .collect())
    }

    fn signal_projections(&self) -> Result<Vec<SignalProjection>, QueryError> {
        let events = self.all_events()?;
        Ok(build_signal_projections(&events)?)
    }

    fn decision_projections(&self) -> Result<Vec<DecisionProjection>, QueryError> {
        let events = self.all_events()?;
        Ok(build_decision_projections(&events)?)
    }
}
