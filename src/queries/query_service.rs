use crate::{
    projections::{
        build_decision_projections, build_signal_projections, DecisionProjection, SignalProjection,
    },
    store::{JsonlEventStore, StoredEvent},
};

use super::{
    decision_execution_boundary, decision_governance, decision_lineage, decision_promotion_policy,
    decision_readiness, fill_execution_boundary, fill_readiness, order_lifecycle,
    order_promotion_policy, order_submission_policy, signal_governance, signal_promotion_policy,
    signal_readiness, DecisionGovernanceReport, DecisionLineageReport, DecisionPromotionReport,
    DecisionReadiness, ExecutionBoundaryReport, FillReadiness, OrderLifecycleReport,
    OrderPromotionReport, OrderSubmissionPolicyReport, QueryError, SignalGovernanceReport,
    SignalPromotionReport, SignalReadiness,
};

#[derive(Debug, Clone, Copy)]
pub struct QueryService<'a> {
    store: &'a JsonlEventStore,
}

impl<'a> QueryService<'a> {
    pub fn new(store: &'a JsonlEventStore) -> Self {
        Self { store }
    }

    pub fn store_ref(&self) -> &'a JsonlEventStore {
        self.store
    }

    pub fn all_events(&self) -> Result<Vec<StoredEvent>, QueryError> {
        Ok(self.store.read_all()?)
    }

    pub fn all_signal_projections(&self) -> Result<Vec<SignalProjection>, QueryError> {
        self.signal_projections()
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

    pub fn all_decision_projections(&self) -> Result<Vec<DecisionProjection>, QueryError> {
        self.decision_projections()
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

    pub fn signal_readiness(&self, signal_id: &str) -> Result<Option<SignalReadiness>, QueryError> {
        let events = self.all_events()?;
        signal_readiness(&events, signal_id)
    }

    pub fn signal_governance(
        &self,
        signal_id: &str,
    ) -> Result<Option<SignalGovernanceReport>, QueryError> {
        let events = self.all_events()?;
        signal_governance(&events, signal_id)
    }

    pub fn signal_promotion_policy(
        &self,
        signal_id: &str,
    ) -> Result<Option<SignalPromotionReport>, QueryError> {
        let events = self.all_events()?;
        signal_promotion_policy(&events, signal_id)
    }

    pub fn decision_readiness(
        &self,
        decision_id: &str,
    ) -> Result<Option<DecisionReadiness>, QueryError> {
        let events = self.all_events()?;
        decision_readiness(&events, decision_id)
    }

    pub fn decision_governance(
        &self,
        decision_id: &str,
    ) -> Result<Option<DecisionGovernanceReport>, QueryError> {
        let events = self.all_events()?;
        decision_governance(&events, decision_id)
    }

    pub fn decision_promotion_policy(
        &self,
        decision_id: &str,
    ) -> Result<Option<DecisionPromotionReport>, QueryError> {
        let events = self.all_events()?;
        decision_promotion_policy(&events, decision_id)
    }

    pub fn decision_lineage(
        &self,
        decision_id: &str,
    ) -> Result<Option<DecisionLineageReport>, QueryError> {
        let events = self.all_events()?;
        decision_lineage(&events, decision_id)
    }

    pub fn fill_readiness(&self, fill_id: &str) -> Result<Option<FillReadiness>, QueryError> {
        let events = self.all_events()?;
        fill_readiness(&events, fill_id)
    }

    pub fn decision_execution_boundary(
        &self,
        decision_id: &str,
    ) -> Result<Option<ExecutionBoundaryReport>, QueryError> {
        let events = self.all_events()?;
        decision_execution_boundary(&events, decision_id)
    }

    pub fn fill_execution_boundary(
        &self,
        fill_id: &str,
    ) -> Result<Option<ExecutionBoundaryReport>, QueryError> {
        let events = self.all_events()?;
        fill_execution_boundary(&events, fill_id)
    }

    pub fn order_lifecycle(
        &self,
        order_id: &str,
    ) -> Result<Option<OrderLifecycleReport>, QueryError> {
        let events = self.all_events()?;
        order_lifecycle(&events, order_id)
    }

    pub fn order_promotion_policy(
        &self,
        order_id: &str,
    ) -> Result<Option<OrderPromotionReport>, QueryError> {
        let events = self.all_events()?;
        order_promotion_policy(&events, order_id)
    }

    pub fn order_submission_policy(
        &self,
        order_id: &str,
    ) -> Result<Option<OrderSubmissionPolicyReport>, QueryError> {
        let events = self.all_events()?;
        order_submission_policy(&events, order_id)
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
