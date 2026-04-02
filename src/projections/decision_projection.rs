use std::collections::HashMap;

use crate::{
    codecs::RehydratedEvent,
    events::{DecisionAction, EventType, FillSide, SignalSide, VetoScope},
    store::StoredEvent,
};

use super::ProjectionError;

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionProjection {
    pub decision_id: String,
    pub instrument: Option<String>,
    pub action: Option<DecisionAction>,
    pub side: Option<SignalSide>,
    pub formed: bool,
    pub vetoed: bool,
    pub fills_count: u64,
    pub filled_quantity: f64,
    pub average_fill_price: Option<f64>,
    pub order_ids: Vec<String>,
    pub last_event_type: EventType,
    pub correlation_id: Option<String>,
}

impl DecisionProjection {
    fn new(decision_id: String, last_event_type: EventType) -> Self {
        Self {
            decision_id,
            instrument: None,
            action: None,
            side: None,
            formed: false,
            vetoed: false,
            fills_count: 0,
            filled_quantity: 0.0,
            average_fill_price: None,
            order_ids: Vec::new(),
            last_event_type,
            correlation_id: None,
        }
    }
}

pub fn build_decision_projections(
    events: &[StoredEvent],
) -> Result<Vec<DecisionProjection>, ProjectionError> {
    let mut projections = Vec::new();
    let mut indexes = HashMap::new();

    for stored in events {
        let rehydrated = RehydratedEvent::try_from(stored)?;

        match rehydrated {
            RehydratedEvent::DecisionFormed(event) => {
                let decision_id = event.payload.decision_id.clone();
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &decision_id,
                    EventType::DecisionFormed,
                );

                projection.formed = true;
                projection.instrument = Some(event.payload.instrument.clone());
                projection.action = Some(event.payload.action);
                projection.side = event.payload.side;
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                if let Some(order_id) = event.linkage.order_id.clone() {
                    push_unique(&mut projection.order_ids, order_id);
                }
                projection.last_event_type = EventType::DecisionFormed;
            }
            RehydratedEvent::FillReceived(event) => {
                let decision_id = resolve_fill_decision_id(&event)?;
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &decision_id,
                    EventType::FillReceived,
                );

                projection.instrument = projection
                    .instrument
                    .clone()
                    .or_else(|| Some(event.payload.instrument.clone()));
                projection.side = projection.side.or_else(|| match event.payload.side {
                    FillSide::Buy => Some(SignalSide::Long),
                    FillSide::Sell => Some(SignalSide::Short),
                });
                projection.fills_count += 1;
                projection.filled_quantity += event.payload.quantity;
                projection.average_fill_price = Some(update_average_fill_price(
                    projection.average_fill_price,
                    projection.filled_quantity - event.payload.quantity,
                    event.payload.quantity,
                    event.payload.price,
                ));
                push_unique(&mut projection.order_ids, event.payload.order_id.clone());
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                projection.last_event_type = EventType::FillReceived;
            }
            RehydratedEvent::VetoRaised(event) => {
                if event.payload.scope != VetoScope::Decision {
                    continue;
                }

                let decision_id = event.payload.target_id.clone();
                let projection = ensure_projection(
                    &mut projections,
                    &mut indexes,
                    &decision_id,
                    EventType::VetoRaised,
                );

                projection.vetoed = true;
                projection.correlation_id = projection
                    .correlation_id
                    .clone()
                    .or_else(|| event.linkage.correlation_id.clone());
                projection.last_event_type = EventType::VetoRaised;
            }
            RehydratedEvent::HypothesisGenerated(_)
            | RehydratedEvent::SignalGenerated(_)
            | RehydratedEvent::SignalConfirmed(_) => {}
        }
    }

    Ok(projections)
}

fn resolve_fill_decision_id(
    event: &crate::events::EventEnvelope<crate::events::FillReceived>,
) -> Result<String, ProjectionError> {
    event
        .payload
        .decision_id
        .clone()
        .or_else(|| event.linkage.decision_id.clone())
        .ok_or_else(|| {
            ProjectionError::invalid_state(format!(
                "fill.received {} is missing decision_id",
                event.event_id
            ))
        })
}

fn update_average_fill_price(
    current_average: Option<f64>,
    current_quantity: f64,
    added_quantity: f64,
    added_price: f64,
) -> f64 {
    let current_notional = current_average.unwrap_or(0.0) * current_quantity;
    let added_notional = added_quantity * added_price;
    (current_notional + added_notional) / (current_quantity + added_quantity)
}

fn ensure_projection<'a>(
    projections: &'a mut Vec<DecisionProjection>,
    indexes: &mut HashMap<String, usize>,
    decision_id: &str,
    last_event_type: EventType,
) -> &'a mut DecisionProjection {
    if let Some(index) = indexes.get(decision_id).copied() {
        return &mut projections[index];
    }

    let index = projections.len();
    projections.push(DecisionProjection::new(
        decision_id.to_string(),
        last_event_type,
    ));
    indexes.insert(decision_id.to_string(), index);
    &mut projections[index]
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}
