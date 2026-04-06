use std::collections::BTreeSet;

use crate::{
    codecs::RehydratedEvent,
    events::{EventEnvelope, OrderRegistered},
    store::StoredEvent,
};

use super::{decision_lineage, DecisionLineageStatus, QueryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionBoundaryReport {
    pub primary_ref_id: String,
    pub primary_ref_type: ExecutionBoundaryRefType,
    pub status: ExecutionBoundaryStatus,
    pub reasons: Vec<ExecutionBoundaryReason>,
    pub decision_refs: Vec<String>,
    pub observed_order_ids: Vec<String>,
    pub observed_fill_ids: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBoundaryRefType {
    Decision,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBoundaryStatus {
    Clear,
    Weak,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionBoundaryReason {
    DecisionFormedObserved,
    FillObserved {
        fill_id: String,
        order_id: String,
    },
    LocalOrderRegistered {
        order_id: String,
        venue: String,
    },
    MultipleCoherentFillsObserved {
        count: usize,
        order_id: String,
    },
    NoExecutionObservedForDecision,
    MissingLocalOrderRegistration {
        order_id: String,
    },
    DecisionLineageSupported {
        decision_id: String,
    },
    DecisionLineageWeak {
        decision_id: String,
    },
    DecisionLineageBlocked {
        decision_id: String,
    },
    DecisionLineageInconsistent {
        decision_id: String,
    },
    FillTracesToDecision {
        decision_id: String,
    },
    FillTracesViaLocalOrder {
        decision_id: String,
        order_id: String,
    },
    MissingDecisionReference,
    ExternalOrderReferenceOnly {
        order_id: String,
    },
    AmbiguousDecisionReferences {
        decision_ids: Vec<String>,
    },
    AmbiguousExternalOrderReferences {
        order_ids: Vec<String>,
    },
    DecisionNotFound {
        decision_id: String,
    },
    FillDecisionInstrumentMismatch {
        fill_id: String,
        fill_instrument: String,
        decision_instrument: String,
    },
    BlockedDecisionHasObservedFill {
        decision_id: String,
        fill_id: String,
    },
    LocalOrderReferencesMissingDecision {
        order_id: String,
        decision_id: String,
    },
}

pub fn decision_execution_boundary(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<Option<ExecutionBoundaryReport>, QueryError> {
    let facts = collect_decision_boundary_facts(events, decision_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    let mut notes = vec![
        "order_id is treated as an external observed reference, not a local entity".to_string(),
    ];

    if facts.formed {
        reasons.push(ExecutionBoundaryReason::DecisionFormedObserved);
    }

    if facts.fill_ids.is_empty() {
        reasons.push(ExecutionBoundaryReason::NoExecutionObservedForDecision);
        notes.push(
            "no downstream fill has been observed yet, so the execution boundary remains weak"
                .to_string(),
        );
    } else {
        reasons.extend(facts.fill_refs.iter().map(|(fill_id, order_id)| {
            ExecutionBoundaryReason::FillObserved {
                fill_id: fill_id.clone(),
                order_id: order_id.clone(),
            }
        }));

        if facts.fill_ids.len() > 1 && facts.order_ids.len() == 1 {
            reasons.push(ExecutionBoundaryReason::MultipleCoherentFillsObserved {
                count: facts.fill_ids.len(),
                order_id: facts
                    .order_ids
                    .iter()
                    .next()
                    .expect("single order id exists")
                    .clone(),
            });
        }
    }

    reasons.extend(facts.local_orders.iter().map(|order| {
        ExecutionBoundaryReason::LocalOrderRegistered {
            order_id: order.payload.order_id.clone(),
            venue: order.payload.venue.clone(),
        }
    }));

    if facts.order_ids.len() > 1 {
        reasons.push(ExecutionBoundaryReason::AmbiguousExternalOrderReferences {
            order_ids: facts.order_ids.iter().cloned().collect(),
        });
        notes.push(
            "multiple external order_ids point at the same decision; without a local order model this is ambiguous"
                .to_string(),
        );
    }

    for order_id in &facts.order_ids {
        if !facts.local_order_ids.contains(order_id) {
            reasons.push(ExecutionBoundaryReason::MissingLocalOrderRegistration {
                order_id: order_id.clone(),
            });
        }
    }

    if !facts.instrument_mismatches.is_empty() {
        reasons.extend(facts.instrument_mismatches.iter().map(
            |(fill_id, fill_instrument, decision_instrument)| {
                ExecutionBoundaryReason::FillDecisionInstrumentMismatch {
                    fill_id: fill_id.clone(),
                    fill_instrument: fill_instrument.clone(),
                    decision_instrument: decision_instrument.clone(),
                }
            },
        ));
    }

    match decision_lineage(events, decision_id)? {
        Some(lineage) => match lineage.status {
            DecisionLineageStatus::Supported => {
                reasons.push(ExecutionBoundaryReason::DecisionLineageSupported {
                    decision_id: decision_id.to_string(),
                });
            }
            DecisionLineageStatus::Weak => {
                reasons.push(ExecutionBoundaryReason::DecisionLineageWeak {
                    decision_id: decision_id.to_string(),
                });
            }
            DecisionLineageStatus::Blocked => {
                reasons.push(ExecutionBoundaryReason::DecisionLineageBlocked {
                    decision_id: decision_id.to_string(),
                });
            }
            DecisionLineageStatus::Inconsistent => {
                reasons.push(ExecutionBoundaryReason::DecisionLineageInconsistent {
                    decision_id: decision_id.to_string(),
                });
            }
        },
        None => reasons.push(ExecutionBoundaryReason::DecisionNotFound {
            decision_id: decision_id.to_string(),
        }),
    }

    if matches!(
        reasons.last(),
        Some(ExecutionBoundaryReason::DecisionLineageBlocked { .. })
    ) {
        for fill_id in &facts.fill_ids {
            reasons.push(ExecutionBoundaryReason::BlockedDecisionHasObservedFill {
                decision_id: decision_id.to_string(),
                fill_id: fill_id.clone(),
            });
        }
    }

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::AmbiguousExternalOrderReferences { .. }
                | ExecutionBoundaryReason::DecisionLineageInconsistent { .. }
                | ExecutionBoundaryReason::FillDecisionInstrumentMismatch { .. }
                | ExecutionBoundaryReason::DecisionNotFound { .. }
        )
    });
    let blocked = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::DecisionLineageBlocked { .. }
                | ExecutionBoundaryReason::BlockedDecisionHasObservedFill { .. }
        )
    });
    let clear = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::DecisionLineageSupported { .. }
        )
    }) && !facts.fill_ids.is_empty()
        && facts.order_ids.len() == 1
        && !facts.local_orders.is_empty()
        && reasons.iter().all(|reason| {
            !matches!(
                reason,
                ExecutionBoundaryReason::MissingLocalOrderRegistration { .. }
            )
        });

    let status = if inconsistent {
        ExecutionBoundaryStatus::Inconsistent
    } else if blocked {
        ExecutionBoundaryStatus::Blocked
    } else if clear {
        ExecutionBoundaryStatus::Clear
    } else {
        ExecutionBoundaryStatus::Weak
    };

    Ok(Some(ExecutionBoundaryReport {
        primary_ref_id: decision_id.to_string(),
        primary_ref_type: ExecutionBoundaryRefType::Decision,
        status,
        reasons,
        decision_refs: vec![decision_id.to_string()],
        observed_order_ids: facts.order_ids.iter().cloned().collect(),
        observed_fill_ids: facts.fill_ids.iter().cloned().collect(),
        notes,
    }))
}

pub fn fill_execution_boundary(
    events: &[StoredEvent],
    fill_id: &str,
) -> Result<Option<ExecutionBoundaryReport>, QueryError> {
    let mut facts = collect_fill_boundary_facts(events, fill_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    let mut notes = vec![
        "order_id is treated as an external observed reference, not a local entity".to_string(),
    ];

    if let Some(order_id) = facts.order_ids.iter().next() {
        if facts.decision_ids.is_empty() && facts.local_orders.is_empty() {
            reasons.push(ExecutionBoundaryReason::ExternalOrderReferenceOnly {
                order_id: order_id.clone(),
            });
        }
    }

    reasons.extend(facts.local_orders.iter().map(|order| {
        ExecutionBoundaryReason::LocalOrderRegistered {
            order_id: order.payload.order_id.clone(),
            venue: order.payload.venue.clone(),
        }
    }));

    if !facts.fill_had_direct_decision_ref {
        if let Some(local_decision_id) = facts
            .local_orders
            .iter()
            .find_map(|order| order.payload.decision_id.clone())
        {
            reasons.push(ExecutionBoundaryReason::FillTracesViaLocalOrder {
                decision_id: local_decision_id.clone(),
                order_id: facts.order_ids.iter().next().cloned().unwrap_or_default(),
            });
            facts.decision_ids.insert(local_decision_id);
        } else if facts.decision_ids.is_empty() {
            reasons.push(ExecutionBoundaryReason::MissingDecisionReference);
            notes.push(
                "the fill is observable but the current model cannot trace it back to a decision"
                    .to_string(),
            );
        }
    }

    if facts.decision_ids.len() == 1 && facts.local_orders.is_empty() {
        for order_id in &facts.order_ids {
            reasons.push(ExecutionBoundaryReason::MissingLocalOrderRegistration {
                order_id: order_id.clone(),
            });
        }
    } else if facts.decision_ids.len() > 1 {
        reasons.push(ExecutionBoundaryReason::AmbiguousDecisionReferences {
            decision_ids: facts.decision_ids.iter().cloned().collect(),
        });
    }

    if facts.decision_ids.len() == 1 {
        let decision_id = facts
            .decision_ids
            .iter()
            .next()
            .expect("single decision id exists")
            .clone();
        reasons.push(ExecutionBoundaryReason::FillTracesToDecision {
            decision_id: decision_id.clone(),
        });

        match decision_lineage(events, &decision_id)? {
            Some(lineage) => match lineage.status {
                DecisionLineageStatus::Supported => {
                    reasons.push(ExecutionBoundaryReason::DecisionLineageSupported { decision_id });
                }
                DecisionLineageStatus::Weak => {
                    reasons.push(ExecutionBoundaryReason::DecisionLineageWeak { decision_id });
                }
                DecisionLineageStatus::Blocked => {
                    reasons.push(ExecutionBoundaryReason::DecisionLineageBlocked { decision_id });
                }
                DecisionLineageStatus::Inconsistent => {
                    reasons
                        .push(ExecutionBoundaryReason::DecisionLineageInconsistent { decision_id });
                }
            },
            None => reasons.push(ExecutionBoundaryReason::DecisionNotFound { decision_id }),
        }
    }

    for order in &facts.local_orders {
        if let Some(decision_id) = order.payload.decision_id.clone() {
            if decision_lineage(events, &decision_id)?.is_none() {
                reasons.push(
                    ExecutionBoundaryReason::LocalOrderReferencesMissingDecision {
                        order_id: order.payload.order_id.clone(),
                        decision_id,
                    },
                );
            }
        }
    }

    if facts.order_ids.len() > 1 {
        reasons.push(ExecutionBoundaryReason::AmbiguousExternalOrderReferences {
            order_ids: facts.order_ids.iter().cloned().collect(),
        });
    }

    if !facts.instrument_mismatches.is_empty() {
        reasons.extend(facts.instrument_mismatches.iter().map(
            |(_decision_id, fill_instrument, decision_instrument)| {
                ExecutionBoundaryReason::FillDecisionInstrumentMismatch {
                    fill_id: fill_id.to_string(),
                    fill_instrument: fill_instrument.clone(),
                    decision_instrument: decision_instrument.clone(),
                }
            },
        ));
        let _ = &facts.decision_ids;
    }

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::AmbiguousDecisionReferences { .. }
                | ExecutionBoundaryReason::AmbiguousExternalOrderReferences { .. }
                | ExecutionBoundaryReason::DecisionLineageInconsistent { .. }
                | ExecutionBoundaryReason::FillDecisionInstrumentMismatch { .. }
                | ExecutionBoundaryReason::DecisionNotFound { .. }
                | ExecutionBoundaryReason::LocalOrderReferencesMissingDecision { .. }
        )
    });
    let blocked = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::DecisionLineageBlocked { .. }
        )
    });
    let clear = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::DecisionLineageSupported { .. }
        )
    }) && facts.decision_ids.len() == 1
        && facts.order_ids.len() == 1
        && !facts.local_orders.is_empty()
        && reasons.iter().all(|reason| {
            !matches!(
                reason,
                ExecutionBoundaryReason::MissingLocalOrderRegistration { .. }
            )
        });

    let status = if inconsistent {
        ExecutionBoundaryStatus::Inconsistent
    } else if blocked {
        ExecutionBoundaryStatus::Blocked
    } else if clear {
        ExecutionBoundaryStatus::Clear
    } else {
        ExecutionBoundaryStatus::Weak
    };

    Ok(Some(ExecutionBoundaryReport {
        primary_ref_id: fill_id.to_string(),
        primary_ref_type: ExecutionBoundaryRefType::Fill,
        status,
        reasons,
        decision_refs: facts.decision_ids.iter().cloned().collect(),
        observed_order_ids: facts.order_ids.iter().cloned().collect(),
        observed_fill_ids: vec![fill_id.to_string()],
        notes,
    }))
}

struct DecisionBoundaryFacts {
    relevant: bool,
    formed: bool,
    fill_ids: BTreeSet<String>,
    order_ids: BTreeSet<String>,
    fill_refs: Vec<(String, String)>,
    local_order_ids: BTreeSet<String>,
    local_orders: Vec<EventEnvelope<OrderRegistered>>,
    instrument_mismatches: Vec<(String, String, String)>,
}

fn collect_decision_boundary_facts(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<DecisionBoundaryFacts, QueryError> {
    let mut facts = DecisionBoundaryFacts {
        relevant: false,
        formed: false,
        fill_ids: BTreeSet::new(),
        order_ids: BTreeSet::new(),
        fill_refs: Vec::new(),
        local_order_ids: BTreeSet::new(),
        local_orders: Vec::new(),
        instrument_mismatches: Vec::new(),
    };

    let mut decision_instruments = BTreeSet::new();
    for stored in events {
        if let RehydratedEvent::DecisionFormed(event) = RehydratedEvent::try_from(stored)? {
            if event.payload.decision_id == decision_id {
                facts.relevant = true;
                facts.formed = true;
                decision_instruments.insert(event.payload.instrument.clone());
            }
        }
    }

    for stored in events {
        if let RehydratedEvent::OrderRegistered(event) = RehydratedEvent::try_from(stored)? {
            if event.payload.decision_id.as_deref() == Some(decision_id)
                || event.linkage.decision_id.as_deref() == Some(decision_id)
            {
                facts.relevant = true;
                facts.local_order_ids.insert(event.payload.order_id.clone());
                facts.local_orders.push(event);
            }
        }
    }

    for stored in events {
        if let RehydratedEvent::FillReceived(event) = RehydratedEvent::try_from(stored)? {
            if event.payload.decision_id.as_deref() == Some(decision_id)
                || event.linkage.decision_id.as_deref() == Some(decision_id)
                || facts.local_order_ids.contains(&event.payload.order_id)
            {
                facts.relevant = true;
                facts.fill_ids.insert(event.payload.fill_id.clone());
                facts.order_ids.insert(event.payload.order_id.clone());
                facts.fill_refs.push((
                    event.payload.fill_id.clone(),
                    event.payload.order_id.clone(),
                ));
                if !decision_instruments.is_empty()
                    && !decision_instruments.contains(&event.payload.instrument)
                {
                    for decision_instrument in &decision_instruments {
                        facts.instrument_mismatches.push((
                            event.payload.fill_id.clone(),
                            event.payload.instrument.clone(),
                            decision_instrument.clone(),
                        ));
                    }
                }
            }
        }
    }

    Ok(facts)
}

struct FillBoundaryFacts {
    relevant: bool,
    decision_ids: BTreeSet<String>,
    order_ids: BTreeSet<String>,
    local_orders: Vec<EventEnvelope<OrderRegistered>>,
    fill_had_direct_decision_ref: bool,
    instrument_mismatches: Vec<(String, String, String)>,
}

fn collect_fill_boundary_facts(
    events: &[StoredEvent],
    fill_id: &str,
) -> Result<FillBoundaryFacts, QueryError> {
    let mut facts = FillBoundaryFacts {
        relevant: false,
        decision_ids: BTreeSet::new(),
        order_ids: BTreeSet::new(),
        local_orders: Vec::new(),
        fill_had_direct_decision_ref: false,
        instrument_mismatches: Vec::new(),
    };

    let mut fill_instrument = None;

    for stored in events {
        if let RehydratedEvent::FillReceived(event) = RehydratedEvent::try_from(stored)? {
            if event.payload.fill_id == fill_id {
                facts.relevant = true;
                facts.order_ids.insert(event.payload.order_id.clone());
                if let Some(decision_id) = event
                    .payload
                    .decision_id
                    .clone()
                    .or_else(|| event.linkage.decision_id.clone())
                {
                    facts.fill_had_direct_decision_ref = true;
                    facts.decision_ids.insert(decision_id);
                }
                fill_instrument = Some(event.payload.instrument.clone());
            }
        }
    }

    for stored in events {
        if let RehydratedEvent::OrderRegistered(event) = RehydratedEvent::try_from(stored)? {
            if facts.order_ids.contains(&event.payload.order_id) {
                facts.relevant = true;
                if let Some(decision_id) = event
                    .payload
                    .decision_id
                    .clone()
                    .or_else(|| event.linkage.decision_id.clone())
                {
                    facts.decision_ids.insert(decision_id);
                }
                facts.local_orders.push(event);
            }
        }
    }

    if let Some(fill_instrument) = fill_instrument {
        for decision_id in &facts.decision_ids {
            for stored in events {
                if let RehydratedEvent::DecisionFormed(EventEnvelope { payload, .. }) =
                    RehydratedEvent::try_from(stored)?
                {
                    if payload.decision_id == *decision_id && payload.instrument != fill_instrument
                    {
                        facts.instrument_mismatches.push((
                            decision_id.clone(),
                            fill_instrument.clone(),
                            payload.instrument.clone(),
                        ));
                    }
                }
            }
        }
    }

    Ok(facts)
}
