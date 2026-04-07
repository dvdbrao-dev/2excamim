use std::collections::BTreeSet;

use crate::{
    codecs::RehydratedEvent,
    events::{EventEnvelope, FillReceived, OrderRegistered, OrderSubmitted},
    store::StoredEvent,
};

use super::{decision_execution_boundary, decision_governance, QueryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLifecycleReport {
    pub order_id: String,
    pub status: OrderLifecycleStatus,
    pub reasons: Vec<OrderLifecycleReason>,
    pub decision_refs: Vec<String>,
    pub observed_fill_ids: Vec<String>,
    pub venue_refs: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderLifecycleStatus {
    Registered,
    Submitted,
    ObservedWithFills,
    Weak,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderLifecycleReason {
    OrderRegisteredObserved { venue: String },
    OrderSubmittedObserved { venue: String },
    FillObserved { fill_id: String, venue: String },
    DecisionLinked { decision_id: String },
    MissingLocalOrderRegistration,
    MissingLocalOrderSubmission,
    SubmittedWithoutRegistration,
    ConflictingDecisionReferences { values: Vec<String> },
    ConflictingVenueReferences { values: Vec<String> },
    ConflictingInstrumentReferences { values: Vec<String> },
}

pub fn order_lifecycle(
    events: &[StoredEvent],
    order_id: &str,
) -> Result<Option<OrderLifecycleReport>, QueryError> {
    let facts = collect_order_lifecycle_facts(events, order_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    let mut notes = Vec::new();

    reasons.extend(facts.registered.iter().map(|event| {
        OrderLifecycleReason::OrderRegisteredObserved {
            venue: event.payload.venue.clone(),
        }
    }));
    reasons.extend(facts.submitted.iter().map(|event| {
        OrderLifecycleReason::OrderSubmittedObserved {
            venue: event.payload.venue.clone(),
        }
    }));
    reasons.extend(
        facts
            .fills
            .iter()
            .map(|event| OrderLifecycleReason::FillObserved {
                fill_id: event.payload.fill_id.clone(),
                venue: event.payload.venue.clone(),
            }),
    );
    reasons.extend(
        facts
            .decision_ids
            .iter()
            .cloned()
            .map(|decision_id| OrderLifecycleReason::DecisionLinked { decision_id }),
    );

    if facts.decision_ids.len() > 1 {
        reasons.push(OrderLifecycleReason::ConflictingDecisionReferences {
            values: facts.decision_ids.iter().cloned().collect(),
        });
    }
    if facts.venues.len() > 1 {
        reasons.push(OrderLifecycleReason::ConflictingVenueReferences {
            values: facts.venues.iter().cloned().collect(),
        });
    }
    if facts.instruments.len() > 1 {
        reasons.push(OrderLifecycleReason::ConflictingInstrumentReferences {
            values: facts.instruments.iter().cloned().collect(),
        });
    }
    if !facts.submitted.is_empty() && facts.registered.is_empty() {
        reasons.push(OrderLifecycleReason::SubmittedWithoutRegistration);
    }
    if !facts.fills.is_empty() && facts.registered.is_empty() {
        reasons.push(OrderLifecycleReason::MissingLocalOrderRegistration);
    }
    if !facts.fills.is_empty() && facts.submitted.is_empty() {
        reasons.push(OrderLifecycleReason::MissingLocalOrderSubmission);
    }

    if facts.registered.is_empty() && !facts.fills.is_empty() {
        notes.push(
            "fills reference this order_id, but the current log has no local order entity"
                .to_string(),
        );
    }
    if !facts.registered.is_empty() && facts.submitted.is_empty() {
        notes.push(
            "the order is locally known but not yet observed as submitted in the current log"
                .to_string(),
        );
    }
    if !facts.submitted.is_empty() && facts.fills.is_empty() {
        notes.push(
            "submission is locally observed, but no downstream fill has been observed yet"
                .to_string(),
        );
    }

    if facts.decision_ids.len() == 1 {
        let decision_id = facts
            .decision_ids
            .iter()
            .next()
            .expect("single decision id exists")
            .clone();

        if let Some(boundary) = decision_execution_boundary(events, &decision_id)? {
            notes.push(format!(
                "decision {} execution boundary is {:?}",
                decision_id, boundary.status
            ));
        }
        if let Some(governance) = decision_governance(events, &decision_id)? {
            notes.push(format!(
                "decision {} governance is {:?}",
                decision_id, governance.status
            ));
        }
    }

    notes.sort();
    notes.dedup();

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            OrderLifecycleReason::SubmittedWithoutRegistration
                | OrderLifecycleReason::ConflictingDecisionReferences { .. }
                | OrderLifecycleReason::ConflictingVenueReferences { .. }
                | OrderLifecycleReason::ConflictingInstrumentReferences { .. }
        )
    });

    let status = if inconsistent {
        OrderLifecycleStatus::Inconsistent
    } else if !facts.registered.is_empty() && !facts.submitted.is_empty() && !facts.fills.is_empty()
    {
        OrderLifecycleStatus::ObservedWithFills
    } else if !facts.registered.is_empty() && !facts.submitted.is_empty() {
        OrderLifecycleStatus::Submitted
    } else if !facts.registered.is_empty() && facts.fills.is_empty() {
        OrderLifecycleStatus::Registered
    } else {
        OrderLifecycleStatus::Weak
    };

    Ok(Some(OrderLifecycleReport {
        order_id: order_id.to_string(),
        status,
        reasons,
        decision_refs: facts.decision_ids.iter().cloned().collect(),
        observed_fill_ids: facts
            .fills
            .iter()
            .map(|event| event.payload.fill_id.clone())
            .collect(),
        venue_refs: facts.venues.iter().cloned().collect(),
        notes,
    }))
}

struct OrderLifecycleFacts {
    relevant: bool,
    registered: Vec<EventEnvelope<OrderRegistered>>,
    submitted: Vec<EventEnvelope<OrderSubmitted>>,
    fills: Vec<EventEnvelope<FillReceived>>,
    decision_ids: BTreeSet<String>,
    venues: BTreeSet<String>,
    instruments: BTreeSet<String>,
}

fn collect_order_lifecycle_facts(
    events: &[StoredEvent],
    order_id: &str,
) -> Result<OrderLifecycleFacts, QueryError> {
    let mut facts = OrderLifecycleFacts {
        relevant: false,
        registered: Vec::new(),
        submitted: Vec::new(),
        fills: Vec::new(),
        decision_ids: BTreeSet::new(),
        venues: BTreeSet::new(),
        instruments: BTreeSet::new(),
    };

    for stored in events {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::OrderRegistered(event) if event.payload.order_id == order_id => {
                facts.relevant = true;
                if let Some(decision_id) = event
                    .payload
                    .decision_id
                    .clone()
                    .or_else(|| event.linkage.decision_id.clone())
                {
                    facts.decision_ids.insert(decision_id);
                }
                facts.venues.insert(event.payload.venue.clone());
                facts.instruments.insert(event.payload.instrument.clone());
                facts.registered.push(event);
            }
            RehydratedEvent::OrderSubmitted(event) if event.payload.order_id == order_id => {
                facts.relevant = true;
                if let Some(decision_id) = event
                    .payload
                    .decision_id
                    .clone()
                    .or_else(|| event.linkage.decision_id.clone())
                {
                    facts.decision_ids.insert(decision_id);
                }
                facts.venues.insert(event.payload.venue.clone());
                facts.instruments.insert(event.payload.instrument.clone());
                facts.submitted.push(event);
            }
            RehydratedEvent::FillReceived(event) if event.payload.order_id == order_id => {
                facts.relevant = true;
                if let Some(decision_id) = event
                    .payload
                    .decision_id
                    .clone()
                    .or_else(|| event.linkage.decision_id.clone())
                {
                    facts.decision_ids.insert(decision_id);
                }
                facts.venues.insert(event.payload.venue.clone());
                facts.instruments.insert(event.payload.instrument.clone());
                facts.fills.push(event);
            }
            _ => {}
        }
    }

    Ok(facts)
}
