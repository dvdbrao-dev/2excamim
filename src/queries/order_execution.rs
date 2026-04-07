use crate::{
    codecs::RehydratedEvent,
    events::{DecisionFormed, EventEnvelope},
    store::StoredEvent,
};

use super::{order_lifecycle, OrderLifecycleStatus, QueryError};

const QUANTITY_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderExecutionStatus {
    SubmittedWithoutFills,
    PartiallyFilled,
    FullyFilled,
    Overfilled,
    TargetQuantityUnknown,
    Inconsistent,
}

impl OrderExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SubmittedWithoutFills => "submitted_without_fills",
            Self::PartiallyFilled => "partially_filled",
            Self::FullyFilled => "fully_filled",
            Self::Overfilled => "overfilled",
            Self::TargetQuantityUnknown => "target_quantity_unknown",
            Self::Inconsistent => "inconsistent",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderExecutionSummary {
    pub order_id: String,
    pub decision_id: Option<String>,
    pub ordered_quantity: Option<f64>,
    pub filled_quantity: f64,
    pub remaining_quantity: Option<f64>,
    pub average_fill_price: Option<f64>,
    pub fill_count: u64,
    pub execution_status: OrderExecutionStatus,
    pub reasons: Vec<String>,
    pub notes: Vec<String>,
}

pub fn order_execution_summary(
    events: &[StoredEvent],
    order_id: &str,
) -> Result<Option<OrderExecutionSummary>, QueryError> {
    let Some(lifecycle) = order_lifecycle(events, order_id)? else {
        return Ok(None);
    };

    let facts = collect_order_execution_facts(events, order_id)?;
    let mut reasons = facts.reasons.clone();
    let mut notes = lifecycle.notes.clone();
    notes.extend(facts.notes.clone());

    if lifecycle.decision_refs.len() > 1 {
        reasons.push("MultipleDecisionReferences".to_string());
        notes.push(
            "execution reconciliation requires a single decision reference for one local order"
                .to_string(),
        );
        return Ok(Some(OrderExecutionSummary {
            order_id: order_id.to_string(),
            decision_id: None,
            ordered_quantity: None,
            filled_quantity: facts.filled_quantity,
            remaining_quantity: None,
            average_fill_price: facts.average_fill_price,
            fill_count: facts.fill_count,
            execution_status: OrderExecutionStatus::Inconsistent,
            reasons,
            notes: dedup_sorted(notes),
        }));
    }

    let decision_id = lifecycle.decision_refs.first().cloned();

    if matches!(
        lifecycle.status,
        OrderLifecycleStatus::Weak | OrderLifecycleStatus::Inconsistent
    ) {
        reasons.push(format!("Lifecycle::{:?}", lifecycle.status));
        notes.push(
            "execution reconciliation requires a coherent local order lifecycle before interpreting fills"
                .to_string(),
        );
        return Ok(Some(OrderExecutionSummary {
            order_id: order_id.to_string(),
            decision_id,
            ordered_quantity: facts.ordered_quantity,
            filled_quantity: facts.filled_quantity,
            remaining_quantity: facts.remaining_quantity(),
            average_fill_price: facts.average_fill_price,
            fill_count: facts.fill_count,
            execution_status: OrderExecutionStatus::Inconsistent,
            reasons,
            notes: dedup_sorted(notes),
        }));
    }

    let execution_status = match (facts.ordered_quantity, facts.fill_count) {
        (_, 0) => {
            reasons.push("NoObservedFills".to_string());
            OrderExecutionStatus::SubmittedWithoutFills
        }
        (None, _) => {
            reasons.push("MissingOrderedQuantity".to_string());
            notes.push(
                "decision.size_hint is absent, so the runtime cannot distinguish partial vs full execution"
                    .to_string(),
            );
            OrderExecutionStatus::TargetQuantityUnknown
        }
        (Some(ordered_quantity), _) => {
            let delta = facts.filled_quantity - ordered_quantity;
            if delta > QUANTITY_EPSILON {
                reasons.push("ObservedQuantityExceedsOrderedQuantity".to_string());
                notes.push(
                    "filled quantity is larger than decision.size_hint under the current local model"
                        .to_string(),
                );
                OrderExecutionStatus::Overfilled
            } else if delta.abs() <= QUANTITY_EPSILON {
                reasons.push("ObservedQuantityMatchesOrderedQuantity".to_string());
                OrderExecutionStatus::FullyFilled
            } else {
                reasons.push("ObservedQuantityBelowOrderedQuantity".to_string());
                OrderExecutionStatus::PartiallyFilled
            }
        }
    };

    Ok(Some(OrderExecutionSummary {
        order_id: order_id.to_string(),
        decision_id,
        ordered_quantity: facts.ordered_quantity,
        filled_quantity: facts.filled_quantity,
        remaining_quantity: facts.remaining_quantity(),
        average_fill_price: facts.average_fill_price,
        fill_count: facts.fill_count,
        execution_status,
        reasons,
        notes: dedup_sorted(notes),
    }))
}

struct OrderExecutionFacts {
    ordered_quantity: Option<f64>,
    filled_quantity: f64,
    average_fill_price: Option<f64>,
    fill_count: u64,
    reasons: Vec<String>,
    notes: Vec<String>,
}

impl OrderExecutionFacts {
    fn remaining_quantity(&self) -> Option<f64> {
        self.ordered_quantity.map(|ordered_quantity| {
            let remaining = ordered_quantity - self.filled_quantity;
            if remaining.abs() <= QUANTITY_EPSILON {
                0.0
            } else {
                normalize_quantity(remaining)
            }
        })
    }
}

fn collect_order_execution_facts(
    events: &[StoredEvent],
    order_id: &str,
) -> Result<OrderExecutionFacts, QueryError> {
    let mut decision_events = Vec::new();
    let mut fills = Vec::new();

    for stored in events {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::DecisionFormed(event) => decision_events.push(event),
            RehydratedEvent::FillReceived(event) if event.payload.order_id == order_id => {
                fills.push(event);
            }
            _ => {}
        }
    }

    let mut ordered_quantity = None;
    let mut reasons = Vec::new();
    let mut notes = Vec::new();

    if let Some(decision_event) =
        resolve_decision_event_for_order(events, order_id, &decision_events)?
    {
        ordered_quantity = decision_event.payload.size_hint;
        if ordered_quantity.is_none() {
            notes.push(format!(
                "decision {} has no size_hint in the current log",
                decision_event.payload.decision_id
            ));
        }
    }

    let fill_count = fills.len() as u64;
    let mut filled_quantity = 0.0;
    let mut notional = 0.0;

    for fill in fills {
        filled_quantity += fill.payload.quantity;
        notional += fill.payload.quantity * fill.payload.price;
    }

    let average_fill_price = if filled_quantity > 0.0 {
        Some(notional / filled_quantity)
    } else {
        None
    };

    if fill_count > 0 {
        reasons.push(format!("ObservedFillCount({fill_count})"));
    }

    Ok(OrderExecutionFacts {
        ordered_quantity,
        filled_quantity,
        average_fill_price,
        fill_count,
        reasons,
        notes,
    })
}

fn resolve_decision_event_for_order(
    events: &[StoredEvent],
    order_id: &str,
    decision_events: &[EventEnvelope<DecisionFormed>],
) -> Result<Option<EventEnvelope<DecisionFormed>>, QueryError> {
    let lifecycle = order_lifecycle(events, order_id)?;
    let Some(lifecycle) = lifecycle else {
        return Ok(None);
    };
    let Some(decision_id) = lifecycle.decision_refs.first() else {
        return Ok(None);
    };

    Ok(decision_events
        .iter()
        .rev()
        .find(|event| event.payload.decision_id == *decision_id)
        .cloned())
}

fn dedup_sorted(mut notes: Vec<String>) -> Vec<String> {
    notes.sort();
    notes.dedup();
    notes
}

fn normalize_quantity(value: f64) -> f64 {
    let precision = 1_000_000_000f64;
    (value * precision).round() / precision
}
