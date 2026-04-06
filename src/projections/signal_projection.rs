use std::collections::HashMap;

use crate::{
    codecs::RehydratedEvent,
    events::{EventType, VetoScope},
    store::StoredEvent,
};

use super::ProjectionError;

#[derive(Debug, Clone, PartialEq)]
pub struct SignalProjection {
    pub signal_id: String,
    pub hypothesis_id: Option<String>,
    pub instrument: Option<String>,
    pub timeframe: Option<String>,
    pub generated: bool,
    pub confirmed: bool,
    pub vetoed: bool,
    pub decision_ids: Vec<String>,
    pub last_event_type: EventType,
    pub correlation_id: Option<String>,
}

impl SignalProjection {
    fn new(signal_id: String, last_event_type: EventType) -> Self {
        Self {
            signal_id,
            hypothesis_id: None,
            instrument: None,
            timeframe: None,
            generated: false,
            confirmed: false,
            vetoed: false,
            decision_ids: Vec::new(),
            last_event_type,
            correlation_id: None,
        }
    }
}

pub fn build_signal_projections(
    events: &[StoredEvent],
) -> Result<Vec<SignalProjection>, ProjectionError> {
    let mut projections = Vec::new();
    let mut indexes = HashMap::new();

    for stored in events {
        let rehydrated = RehydratedEvent::try_from(stored)?;

        match rehydrated {
            RehydratedEvent::SignalGenerated(event) => {
                let signal_id = event.payload.signal_id.clone();
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &signal_id,
                    EventType::SignalGenerated,
                );

                projection.generated = true;
                projection.hypothesis_id = event
                    .payload
                    .hypothesis_id
                    .clone()
                    .or_else(|| event.linkage.hypothesis_id.clone());
                projection.instrument = Some(event.payload.instrument.clone());
                projection.timeframe = Some(event.payload.timeframe.clone());
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                projection.last_event_type = EventType::SignalGenerated;
            }
            RehydratedEvent::SignalConfirmed(event) => {
                let signal_id = event.payload.signal_id.clone();
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &signal_id,
                    EventType::SignalConfirmed,
                );

                projection.confirmed = true;
                projection.hypothesis_id = projection
                    .hypothesis_id
                    .clone()
                    .or_else(|| event.linkage.hypothesis_id.clone());
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                projection.last_event_type = EventType::SignalConfirmed;
            }
            RehydratedEvent::VetoRaised(event) => {
                if event.payload.scope != VetoScope::Signal {
                    continue;
                }

                let signal_id = event.payload.target_id.clone();
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &signal_id,
                    EventType::VetoRaised,
                );

                projection.vetoed = true;
                projection.hypothesis_id = projection
                    .hypothesis_id
                    .clone()
                    .or_else(|| event.linkage.hypothesis_id.clone());
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                projection.last_event_type = EventType::VetoRaised;
            }
            RehydratedEvent::DecisionFormed(event) => {
                let Some(signal_id) = event.linkage.signal_id.clone() else {
                    continue;
                };
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &signal_id,
                    EventType::DecisionFormed,
                );

                if !projection
                    .decision_ids
                    .iter()
                    .any(|decision_id| decision_id == &event.payload.decision_id)
                {
                    projection
                        .decision_ids
                        .push(event.payload.decision_id.clone());
                }
                projection.hypothesis_id = projection
                    .hypothesis_id
                    .clone()
                    .or_else(|| event.linkage.hypothesis_id.clone());
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                projection.last_event_type = EventType::DecisionFormed;
            }
            RehydratedEvent::HypothesisGenerated(_)
            | RehydratedEvent::OrderRegistered(_)
            | RehydratedEvent::FillReceived(_) => {}
        }
    }

    Ok(projections)
}

fn ensure_projection<'a>(
    projections: &'a mut Vec<SignalProjection>,
    indexes: &mut HashMap<String, usize>,
    signal_id: &str,
    last_event_type: EventType,
) -> &'a mut SignalProjection {
    if let Some(index) = indexes.get(signal_id).copied() {
        return &mut projections[index];
    }

    let index = projections.len();
    projections.push(SignalProjection::new(
        signal_id.to_string(),
        last_event_type,
    ));
    indexes.insert(signal_id.to_string(), index);
    &mut projections[index]
}
