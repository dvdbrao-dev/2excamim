use crate::{
    projections::{
        build_decision_projections, build_signal_projections, DecisionProjection, SignalProjection,
    },
    queries::QueryService,
    store::JsonlEventStore,
};

use super::{ScenarioError, ScenarioFixture};

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayResult {
    pub total_events: usize,
    pub signal_projections: Vec<SignalProjection>,
    pub decision_projections: Vec<DecisionProjection>,
    pub confirmed_signals_count: usize,
    pub vetoed_signals_count: usize,
    pub decisions_with_fills_count: usize,
    pub decisions_without_fills_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ReplayHarness<'a> {
    store: &'a JsonlEventStore,
}

impl<'a> ReplayHarness<'a> {
    pub fn new(store: &'a JsonlEventStore) -> Self {
        Self { store }
    }

    pub fn run_fixture(&self, fixture: &ScenarioFixture) -> Result<ReplayResult, ScenarioError> {
        self.store.append_events(&fixture.events)?;

        let query_service = QueryService::new(self.store);
        let all_events = query_service.all_events()?;
        let signal_projections = build_signal_projections(&all_events)?;
        let decision_projections = build_decision_projections(&all_events)?;
        let confirmed_signals_count = query_service.confirmed_signals()?.len();
        let vetoed_signals_count = query_service.vetoed_signals()?.len();
        let decisions_with_fills_count = query_service.decisions_with_fills()?.len();
        let decisions_without_fills_count = query_service.decisions_without_fills()?.len();

        Ok(ReplayResult {
            total_events: all_events.len(),
            signal_projections,
            decision_projections,
            confirmed_signals_count,
            vetoed_signals_count,
            decisions_with_fills_count,
            decisions_without_fills_count,
        })
    }
}
