use std::collections::{BTreeMap, BTreeSet};

use crate::{
    codecs::RehydratedEvent,
    events::EventType,
    projections::{build_decision_projections, build_signal_projections},
    store::{JsonlEventStore, StoredEvent},
};

use super::ObservabilityError;

#[derive(Debug, Clone, PartialEq)]
pub struct ObservabilitySummary {
    pub total_events: usize,
    pub total_signals: usize,
    pub total_decisions: usize,
    pub confirmed_signals: usize,
    pub vetoed_signals: usize,
    pub decisions_with_fills: usize,
    pub decisions_without_fills: usize,
    pub total_fills: usize,
    pub total_filled_quantity: f64,
    pub unique_correlation_ids: usize,
    pub event_counts_by_type: BTreeMap<String, usize>,
}

pub fn build_observability_summary(
    events: &[StoredEvent],
) -> Result<ObservabilitySummary, ObservabilityError> {
    let signal_projections = build_signal_projections(events)?;
    let decision_projections = build_decision_projections(events)?;

    let mut event_counts_by_type = BTreeMap::new();
    let mut correlation_ids = BTreeSet::new();
    let mut total_fills = 0usize;
    let mut total_filled_quantity = 0.0;

    for stored in events {
        *event_counts_by_type
            .entry(stored.event_type.as_str().to_string())
            .or_insert(0) += 1;

        if let Some(correlation_id) = stored.linkage.correlation_id.as_deref() {
            if !correlation_id.trim().is_empty() {
                correlation_ids.insert(correlation_id.to_string());
            }
        }

        if stored.event_type == EventType::FillReceived {
            total_fills += 1;

            if let RehydratedEvent::FillReceived(event) = RehydratedEvent::try_from(stored)? {
                total_filled_quantity += event.payload.quantity;
            }
        }
    }

    let confirmed_signals = signal_projections
        .iter()
        .filter(|projection| projection.confirmed)
        .count();
    let vetoed_signals = signal_projections
        .iter()
        .filter(|projection| projection.vetoed)
        .count();
    let decisions_with_fills = decision_projections
        .iter()
        .filter(|projection| projection.fills_count > 0)
        .count();
    let decisions_without_fills = decision_projections.len() - decisions_with_fills;

    Ok(ObservabilitySummary {
        total_events: events.len(),
        total_signals: signal_projections.len(),
        total_decisions: decision_projections.len(),
        confirmed_signals,
        vetoed_signals,
        decisions_with_fills,
        decisions_without_fills,
        total_fills,
        total_filled_quantity,
        unique_correlation_ids: correlation_ids.len(),
        event_counts_by_type,
    })
}

pub fn summary_from_store(
    store: &JsonlEventStore,
) -> Result<ObservabilitySummary, ObservabilityError> {
    let events = store.read_all()?;
    build_observability_summary(&events)
}
