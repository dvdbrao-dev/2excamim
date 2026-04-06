use std::collections::BTreeSet;

use crate::{
    codecs::RehydratedEvent,
    events::{DecisionFormed, EventEnvelope, FillReceived, OrderRegistered, VetoRaised, VetoScope},
    store::StoredEvent,
};

use super::{signal_readiness, QueryError, SignalReadinessStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionLineageReport {
    pub decision_id: String,
    pub status: DecisionLineageStatus,
    pub reasons: Vec<DecisionLineageReason>,
    pub upstream_refs: DecisionUpstreamRefs,
    pub downstream_refs: DecisionDownstreamRefs,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionLineageStatus {
    Supported,
    Weak,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionLineageReason {
    DecisionFormedObserved,
    MissingDecisionFormed,
    UpstreamSignalSupported {
        signal_id: String,
    },
    UpstreamSignalGeneratedUnconfirmed {
        signal_id: String,
    },
    UpstreamSignalBlocked {
        signal_id: String,
    },
    UpstreamSignalInconsistent {
        signal_id: String,
    },
    MissingUpstreamSignalReference,
    ConflictingSignalReferences {
        values: Vec<String>,
    },
    HypothesisTraced {
        hypothesis_id: String,
    },
    ConflictingHypothesisReferences {
        values: Vec<String>,
    },
    DirectDecisionVetoObserved {
        veto_id: String,
        reason_code: String,
    },
    LocalOrderRegistered {
        order_id: String,
        venue: String,
    },
    DownstreamFillObserved {
        fill_id: String,
        order_id: String,
    },
    FillInstrumentMismatch {
        fill_id: String,
        fill_instrument: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecisionUpstreamRefs {
    pub signal_ids: Vec<String>,
    pub hypothesis_ids: Vec<String>,
    pub decision_vetoes: Vec<LineageVetoRef>,
    pub signal_vetoes: Vec<LineageVetoRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecisionDownstreamRefs {
    pub fill_ids: Vec<String>,
    pub order_ids: Vec<String>,
    pub local_order_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageVetoRef {
    pub veto_id: String,
    pub target_id: String,
    pub reason_code: String,
}

pub fn decision_lineage(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<Option<DecisionLineageReport>, QueryError> {
    let facts = collect_decision_lineage_facts(events, decision_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    let mut notes = Vec::new();

    if !facts.formed_events.is_empty() {
        reasons.push(DecisionLineageReason::DecisionFormedObserved);
    } else {
        reasons.push(DecisionLineageReason::MissingDecisionFormed);
    }

    if facts.signal_ids.is_empty() {
        reasons.push(DecisionLineageReason::MissingUpstreamSignalReference);
    } else if facts.signal_ids.len() > 1 {
        reasons.push(DecisionLineageReason::ConflictingSignalReferences {
            values: facts.signal_ids.iter().cloned().collect(),
        });
    }

    if facts.hypothesis_ids.len() == 1 {
        reasons.push(DecisionLineageReason::HypothesisTraced {
            hypothesis_id: facts
                .hypothesis_ids
                .iter()
                .next()
                .expect("single hypothesis exists")
                .clone(),
        });
    } else if facts.hypothesis_ids.len() > 1 {
        reasons.push(DecisionLineageReason::ConflictingHypothesisReferences {
            values: facts.hypothesis_ids.iter().cloned().collect(),
        });
    }

    reasons.extend(facts.decision_vetoes.iter().map(|veto| {
        DecisionLineageReason::DirectDecisionVetoObserved {
            veto_id: veto.payload.veto_id.clone(),
            reason_code: veto.payload.reason_code.clone(),
        }
    }));

    reasons.extend(facts.local_orders.iter().map(|order| {
        DecisionLineageReason::LocalOrderRegistered {
            order_id: order.payload.order_id.clone(),
            venue: order.payload.venue.clone(),
        }
    }));

    reasons.extend(
        facts
            .fills
            .iter()
            .map(|fill| DecisionLineageReason::DownstreamFillObserved {
                fill_id: fill.payload.fill_id.clone(),
                order_id: fill.payload.order_id.clone(),
            }),
    );

    reasons.extend(
        facts
            .fill_instrument_mismatches
            .iter()
            .map(
                |(fill_id, fill_instrument)| DecisionLineageReason::FillInstrumentMismatch {
                    fill_id: fill_id.clone(),
                    fill_instrument: fill_instrument.clone(),
                },
            ),
    );

    for signal_id in &facts.signal_ids {
        match signal_readiness(events, signal_id)? {
            Some(readiness) => match readiness.status {
                SignalReadinessStatus::ReadyForDecision => {
                    reasons.push(DecisionLineageReason::UpstreamSignalSupported {
                        signal_id: signal_id.clone(),
                    });
                }
                SignalReadinessStatus::GeneratedUnconfirmed => {
                    reasons.push(DecisionLineageReason::UpstreamSignalGeneratedUnconfirmed {
                        signal_id: signal_id.clone(),
                    });
                }
                SignalReadinessStatus::BlockedByVeto => {
                    reasons.push(DecisionLineageReason::UpstreamSignalBlocked {
                        signal_id: signal_id.clone(),
                    });
                }
                SignalReadinessStatus::Inconsistent => {
                    reasons.push(DecisionLineageReason::UpstreamSignalInconsistent {
                        signal_id: signal_id.clone(),
                    });
                }
            },
            None => reasons.push(DecisionLineageReason::UpstreamSignalInconsistent {
                signal_id: signal_id.clone(),
            }),
        }
    }

    if !facts.fills.is_empty() {
        notes.push(
            "fill observation is downstream external evidence; order lifecycle remains out of scope in v1"
                .to_string(),
        );
    }
    if !facts.local_orders.is_empty() {
        notes.push(
            "order.registered provides local contractual support for downstream execution traceability"
                .to_string(),
        );
    }
    if facts.hypothesis_ids.is_empty() {
        notes.push("no hypothesis is contractually traceable from the current lineage".to_string());
    }
    if facts.signal_ids.is_empty() {
        notes.push(
            "decision lineage is weak because the current model has no traceable signal upstream"
                .to_string(),
        );
    }

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::MissingDecisionFormed
                | DecisionLineageReason::ConflictingSignalReferences { .. }
                | DecisionLineageReason::ConflictingHypothesisReferences { .. }
                | DecisionLineageReason::UpstreamSignalInconsistent { .. }
                | DecisionLineageReason::FillInstrumentMismatch { .. }
        )
    });
    let blocked = reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::DirectDecisionVetoObserved { .. }
                | DecisionLineageReason::UpstreamSignalBlocked { .. }
        )
    });
    let supported = reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::UpstreamSignalSupported { .. }
        )
    });

    let status = if inconsistent {
        DecisionLineageStatus::Inconsistent
    } else if blocked {
        DecisionLineageStatus::Blocked
    } else if !facts.formed_events.is_empty() && supported {
        DecisionLineageStatus::Supported
    } else {
        DecisionLineageStatus::Weak
    };

    Ok(Some(DecisionLineageReport {
        decision_id: decision_id.to_string(),
        status,
        reasons,
        upstream_refs: DecisionUpstreamRefs {
            signal_ids: facts.signal_ids.iter().cloned().collect(),
            hypothesis_ids: facts.hypothesis_ids.iter().cloned().collect(),
            decision_vetoes: facts.decision_vetoes.iter().map(lineage_veto_ref).collect(),
            signal_vetoes: facts.signal_vetoes.iter().map(lineage_veto_ref).collect(),
        },
        downstream_refs: DecisionDownstreamRefs {
            fill_ids: facts
                .fills
                .iter()
                .map(|fill| fill.payload.fill_id.clone())
                .collect(),
            order_ids: facts
                .fills
                .iter()
                .map(|fill| fill.payload.order_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            local_order_ids: facts
                .local_orders
                .iter()
                .map(|order| order.payload.order_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        },
        notes,
    }))
}

fn lineage_veto_ref(veto: &EventEnvelope<VetoRaised>) -> LineageVetoRef {
    LineageVetoRef {
        veto_id: veto.payload.veto_id.clone(),
        target_id: veto.payload.target_id.clone(),
        reason_code: veto.payload.reason_code.clone(),
    }
}

struct DecisionLineageFacts {
    relevant: bool,
    formed_events: Vec<EventEnvelope<DecisionFormed>>,
    signal_ids: BTreeSet<String>,
    hypothesis_ids: BTreeSet<String>,
    decision_vetoes: Vec<EventEnvelope<VetoRaised>>,
    signal_vetoes: Vec<EventEnvelope<VetoRaised>>,
    local_orders: Vec<EventEnvelope<OrderRegistered>>,
    local_order_ids: BTreeSet<String>,
    fills: Vec<EventEnvelope<FillReceived>>,
    fill_instrument_mismatches: Vec<(String, String)>,
}

fn collect_decision_lineage_facts(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<DecisionLineageFacts, QueryError> {
    let mut facts = DecisionLineageFacts {
        relevant: false,
        formed_events: Vec::new(),
        signal_ids: BTreeSet::new(),
        hypothesis_ids: BTreeSet::new(),
        decision_vetoes: Vec::new(),
        signal_vetoes: Vec::new(),
        local_orders: Vec::new(),
        local_order_ids: BTreeSet::new(),
        fills: Vec::new(),
        fill_instrument_mismatches: Vec::new(),
    };

    let mut formed_instruments = BTreeSet::new();

    for stored in events {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::DecisionFormed(event) if event.payload.decision_id == decision_id => {
                facts.relevant = true;
                formed_instruments.insert(event.payload.instrument.clone());
                if let Some(signal_id) = event.linkage.signal_id.clone() {
                    facts.signal_ids.insert(signal_id);
                }
                if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                    facts.hypothesis_ids.insert(hypothesis_id);
                }
                facts.formed_events.push(event);
            }
            RehydratedEvent::VetoRaised(event)
                if event.payload.scope == VetoScope::Decision
                    && event.payload.target_id == decision_id =>
            {
                facts.relevant = true;
                facts.decision_vetoes.push(event);
            }
            RehydratedEvent::OrderRegistered(event)
                if event.payload.decision_id.as_deref() == Some(decision_id)
                    || event.linkage.decision_id.as_deref() == Some(decision_id) =>
            {
                facts.relevant = true;
                facts.local_order_ids.insert(event.payload.order_id.clone());
                facts.local_orders.push(event);
            }
            RehydratedEvent::FillReceived(event)
                if event.payload.decision_id.as_deref() == Some(decision_id)
                    || event.linkage.decision_id.as_deref() == Some(decision_id)
                    || facts.local_order_ids.contains(&event.payload.order_id) =>
            {
                facts.relevant = true;
                if !formed_instruments.is_empty()
                    && !formed_instruments.contains(&event.payload.instrument)
                {
                    facts.fill_instrument_mismatches.push((
                        event.payload.fill_id.clone(),
                        event.payload.instrument.clone(),
                    ));
                }
                facts.fills.push(event);
            }
            _ => {}
        }
    }

    if !facts.signal_ids.is_empty() {
        for stored in events {
            match RehydratedEvent::try_from(stored)? {
                RehydratedEvent::SignalGenerated(event)
                    if facts.signal_ids.contains(&event.payload.signal_id) =>
                {
                    if let Some(hypothesis_id) = event.payload.hypothesis_id.clone() {
                        facts.hypothesis_ids.insert(hypothesis_id);
                    }
                    if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                        facts.hypothesis_ids.insert(hypothesis_id);
                    }
                }
                RehydratedEvent::SignalConfirmed(event)
                    if facts.signal_ids.contains(&event.payload.signal_id) =>
                {
                    if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                        facts.hypothesis_ids.insert(hypothesis_id);
                    }
                }
                RehydratedEvent::VetoRaised(event)
                    if event.payload.scope == VetoScope::Signal
                        && facts.signal_ids.contains(&event.payload.target_id) =>
                {
                    facts.signal_vetoes.push(event);
                }
                _ => {}
            }
        }
    }

    Ok(facts)
}
