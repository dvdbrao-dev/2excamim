use std::collections::BTreeSet;

use crate::{
    codecs::RehydratedEvent,
    events::{EventEnvelope, FillReceived, SignalConfirmed, VetoRaised, VetoScope},
    store::StoredEvent,
};

use super::QueryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalReadiness {
    pub signal_id: String,
    pub status: SignalReadinessStatus,
    pub reasons: Vec<SignalReadinessReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalReadinessStatus {
    GeneratedUnconfirmed,
    ReadyForDecision,
    BlockedByVeto,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalReadinessReason {
    SignalGeneratedObserved,
    SignalConfirmedObserved {
        confirmed_by: String,
    },
    SignalVetoObserved {
        veto_id: String,
        reason_code: String,
    },
    MissingSignalGenerated,
    SignalConfirmationBeforeGeneration,
    SignalVetoBeforeGeneration,
    DecisionObservedWithoutConfirmedSignal {
        decision_id: String,
    },
    ConflictingHypothesisReferences {
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionReadiness {
    pub decision_id: String,
    pub status: DecisionReadinessStatus,
    pub reasons: Vec<DecisionReadinessReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionReadinessStatus {
    Ready,
    FormedButUpstreamWeak,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionReadinessReason {
    DecisionFormedObserved,
    DecisionVetoObserved {
        veto_id: String,
        reason_code: String,
    },
    UpstreamSignalReady {
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
    MissingDecisionFormed,
    MissingUpstreamSignalReference,
    ConflictingSignalReferences {
        values: Vec<String>,
    },
    FillInstrumentMismatch {
        fill_id: String,
        fill_instrument: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillReadiness {
    pub fill_id: String,
    pub status: FillReadinessStatus,
    pub reasons: Vec<FillReadinessReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillReadinessStatus {
    ReceivedWithSufficientReferences,
    ReceivedButUpstreamInsufficient,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillReadinessReason {
    FillReceivedObserved,
    OrderReferencePresent {
        order_id: String,
    },
    DecisionReferencePresent {
        decision_id: String,
    },
    MissingDecisionReference,
    ReferencedDecisionReady {
        decision_id: String,
    },
    ReferencedDecisionWeak {
        decision_id: String,
    },
    ReferencedDecisionBlocked {
        decision_id: String,
    },
    ReferencedDecisionMissing {
        decision_id: String,
    },
    ReferencedDecisionInconsistent {
        decision_id: String,
    },
    AmbiguousFillIdentity {
        decision_ids: Vec<String>,
        order_ids: Vec<String>,
        venues: Vec<String>,
    },
    FillInstrumentMismatch {
        decision_id: String,
        decision_instrument: String,
        fill_instrument: String,
    },
}

pub fn signal_readiness(
    events: &[StoredEvent],
    signal_id: &str,
) -> Result<Option<SignalReadiness>, QueryError> {
    let facts = collect_signal_facts(events, signal_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    if facts.generated_index.is_some() {
        reasons.push(SignalReadinessReason::SignalGeneratedObserved);
    }
    reasons.extend(facts.confirmations.iter().map(|confirmation| {
        SignalReadinessReason::SignalConfirmedObserved {
            confirmed_by: confirmation.payload.confirmed_by.clone(),
        }
    }));
    reasons.extend(
        facts
            .vetoes
            .iter()
            .map(|veto| SignalReadinessReason::SignalVetoObserved {
                veto_id: veto.payload.veto_id.clone(),
                reason_code: veto.payload.reason_code.clone(),
            }),
    );

    if !facts.hypothesis_refs.is_empty() && facts.hypothesis_refs.len() > 1 {
        reasons.push(SignalReadinessReason::ConflictingHypothesisReferences {
            values: facts.hypothesis_refs.into_iter().collect(),
        });
    }
    if facts.generated_index.is_none() {
        reasons.push(SignalReadinessReason::MissingSignalGenerated);
    }
    if facts.confirmation_indexes.iter().any(|index| {
        facts
            .generated_index
            .is_some_and(|generated_index| index < &generated_index)
    }) {
        reasons.push(SignalReadinessReason::SignalConfirmationBeforeGeneration);
    }
    if facts.veto_indexes.iter().any(|index| {
        facts
            .generated_index
            .is_some_and(|generated_index| index < &generated_index)
    }) {
        reasons.push(SignalReadinessReason::SignalVetoBeforeGeneration);
    }
    if facts.generated_index.is_none() {
        for decision_id in &facts.downstream_decision_ids {
            reasons.push(
                SignalReadinessReason::DecisionObservedWithoutConfirmedSignal {
                    decision_id: decision_id.clone(),
                },
            );
        }
    }

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            SignalReadinessReason::MissingSignalGenerated
                | SignalReadinessReason::SignalConfirmationBeforeGeneration
                | SignalReadinessReason::SignalVetoBeforeGeneration
                | SignalReadinessReason::DecisionObservedWithoutConfirmedSignal { .. }
                | SignalReadinessReason::ConflictingHypothesisReferences { .. }
        )
    });

    let status = if inconsistent {
        SignalReadinessStatus::Inconsistent
    } else if !facts.vetoes.is_empty() {
        SignalReadinessStatus::BlockedByVeto
    } else if facts.generated_index.is_some() && !facts.confirmations.is_empty() {
        SignalReadinessStatus::ReadyForDecision
    } else {
        SignalReadinessStatus::GeneratedUnconfirmed
    };

    Ok(Some(SignalReadiness {
        signal_id: signal_id.to_string(),
        status,
        reasons,
    }))
}

pub fn decision_readiness(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<Option<DecisionReadiness>, QueryError> {
    let facts = collect_decision_facts(events, decision_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    if facts.formed_event.is_some() {
        reasons.push(DecisionReadinessReason::DecisionFormedObserved);
    } else {
        reasons.push(DecisionReadinessReason::MissingDecisionFormed);
    }
    reasons.extend(
        facts
            .vetoes
            .iter()
            .map(|veto| DecisionReadinessReason::DecisionVetoObserved {
                veto_id: veto.payload.veto_id.clone(),
                reason_code: veto.payload.reason_code.clone(),
            }),
    );
    if facts.signal_refs.len() > 1 {
        reasons.push(DecisionReadinessReason::ConflictingSignalReferences {
            values: facts.signal_refs.into_iter().collect(),
        });
    }
    reasons.extend(
        facts
            .instrument_mismatches
            .into_iter()
            .map(
                |(fill_id, fill_instrument)| DecisionReadinessReason::FillInstrumentMismatch {
                    fill_id,
                    fill_instrument,
                },
            ),
    );

    let upstream = match facts.signal_ref.as_ref() {
        Some(signal_id) => signal_readiness(events, signal_id)?,
        None => None,
    };

    if let Some(signal_id) = facts.signal_ref.clone() {
        match upstream.as_ref().map(|readiness| &readiness.status) {
            Some(SignalReadinessStatus::ReadyForDecision) => {
                reasons.push(DecisionReadinessReason::UpstreamSignalReady { signal_id });
            }
            Some(SignalReadinessStatus::GeneratedUnconfirmed) => reasons
                .push(DecisionReadinessReason::UpstreamSignalGeneratedUnconfirmed { signal_id }),
            Some(SignalReadinessStatus::BlockedByVeto) => {
                reasons.push(DecisionReadinessReason::UpstreamSignalBlocked { signal_id });
            }
            Some(SignalReadinessStatus::Inconsistent) => {
                reasons.push(DecisionReadinessReason::UpstreamSignalInconsistent { signal_id });
            }
            None => reasons.push(DecisionReadinessReason::MissingUpstreamSignalReference),
        }
    } else {
        reasons.push(DecisionReadinessReason::MissingUpstreamSignalReference);
    }

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionReadinessReason::MissingDecisionFormed
                | DecisionReadinessReason::ConflictingSignalReferences { .. }
                | DecisionReadinessReason::UpstreamSignalInconsistent { .. }
                | DecisionReadinessReason::FillInstrumentMismatch { .. }
        )
    });
    let blocked = reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionReadinessReason::DecisionVetoObserved { .. }
                | DecisionReadinessReason::UpstreamSignalBlocked { .. }
        )
    });
    let upstream_ready = reasons
        .iter()
        .any(|reason| matches!(reason, DecisionReadinessReason::UpstreamSignalReady { .. }));

    let status = if inconsistent {
        DecisionReadinessStatus::Inconsistent
    } else if blocked {
        DecisionReadinessStatus::Blocked
    } else if facts.formed_event.is_some() && upstream_ready {
        DecisionReadinessStatus::Ready
    } else {
        DecisionReadinessStatus::FormedButUpstreamWeak
    };

    Ok(Some(DecisionReadiness {
        decision_id: decision_id.to_string(),
        status,
        reasons,
    }))
}

pub fn fill_readiness(
    events: &[StoredEvent],
    fill_id: &str,
) -> Result<Option<FillReadiness>, QueryError> {
    let facts = collect_fill_facts(events, fill_id)?;
    if !facts.relevant {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    reasons.extend((0..facts.fill_events.len()).map(|_| FillReadinessReason::FillReceivedObserved));

    if facts.decision_ids.len() > 1 || facts.order_ids.len() > 1 || facts.venues.len() > 1 {
        reasons.push(FillReadinessReason::AmbiguousFillIdentity {
            decision_ids: facts.decision_ids.iter().cloned().collect(),
            order_ids: facts.order_ids.iter().cloned().collect(),
            venues: facts.venues.iter().cloned().collect(),
        });
    }

    let fill = facts.fill_events.first();
    if let Some(fill) = fill {
        reasons.push(FillReadinessReason::OrderReferencePresent {
            order_id: fill.payload.order_id.clone(),
        });
    }

    if let Some(decision_id) = facts.decision_id.clone() {
        reasons.push(FillReadinessReason::DecisionReferencePresent {
            decision_id: decision_id.clone(),
        });

        match decision_readiness(events, &decision_id)? {
            Some(readiness) => match readiness.status {
                DecisionReadinessStatus::Ready => {
                    reasons.push(FillReadinessReason::ReferencedDecisionReady { decision_id });
                }
                DecisionReadinessStatus::FormedButUpstreamWeak => {
                    reasons.push(FillReadinessReason::ReferencedDecisionWeak { decision_id });
                }
                DecisionReadinessStatus::Blocked => {
                    reasons.push(FillReadinessReason::ReferencedDecisionBlocked { decision_id });
                }
                DecisionReadinessStatus::Inconsistent => {
                    reasons
                        .push(FillReadinessReason::ReferencedDecisionInconsistent { decision_id });
                }
            },
            None => reasons.push(FillReadinessReason::ReferencedDecisionMissing { decision_id }),
        }
    } else {
        reasons.push(FillReadinessReason::MissingDecisionReference);
    }

    if let Some((decision_id, decision_instrument, fill_instrument)) = facts.instrument_mismatch {
        reasons.push(FillReadinessReason::FillInstrumentMismatch {
            decision_id,
            decision_instrument,
            fill_instrument,
        });
    }

    let inconsistent = reasons.iter().any(|reason| {
        matches!(
            reason,
            FillReadinessReason::AmbiguousFillIdentity { .. }
                | FillReadinessReason::ReferencedDecisionInconsistent { .. }
                | FillReadinessReason::FillInstrumentMismatch { .. }
        )
    });
    let sufficient = reasons
        .iter()
        .any(|reason| matches!(reason, FillReadinessReason::ReferencedDecisionReady { .. }))
        && !reasons
            .iter()
            .any(|reason| matches!(reason, FillReadinessReason::MissingDecisionReference));

    let status = if inconsistent {
        FillReadinessStatus::Inconsistent
    } else if sufficient {
        FillReadinessStatus::ReceivedWithSufficientReferences
    } else {
        FillReadinessStatus::ReceivedButUpstreamInsufficient
    };

    Ok(Some(FillReadiness {
        fill_id: fill_id.to_string(),
        status,
        reasons,
    }))
}

struct SignalFacts {
    relevant: bool,
    generated_index: Option<usize>,
    confirmation_indexes: Vec<usize>,
    veto_indexes: Vec<usize>,
    confirmations: Vec<EventEnvelope<SignalConfirmed>>,
    vetoes: Vec<EventEnvelope<VetoRaised>>,
    hypothesis_refs: BTreeSet<String>,
    downstream_decision_ids: BTreeSet<String>,
}

fn collect_signal_facts(
    events: &[StoredEvent],
    signal_id: &str,
) -> Result<SignalFacts, QueryError> {
    let mut facts = SignalFacts {
        relevant: false,
        generated_index: None,
        confirmation_indexes: Vec::new(),
        veto_indexes: Vec::new(),
        confirmations: Vec::new(),
        vetoes: Vec::new(),
        hypothesis_refs: BTreeSet::new(),
        downstream_decision_ids: BTreeSet::new(),
    };

    for (index, stored) in events.iter().enumerate() {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::SignalGenerated(event) if event.payload.signal_id == signal_id => {
                facts.relevant = true;
                facts.generated_index = Some(index);
                if let Some(hypothesis_id) = event.payload.hypothesis_id.clone() {
                    facts.hypothesis_refs.insert(hypothesis_id);
                }
                if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                    facts.hypothesis_refs.insert(hypothesis_id);
                }
            }
            RehydratedEvent::SignalConfirmed(event) if event.payload.signal_id == signal_id => {
                facts.relevant = true;
                facts.confirmation_indexes.push(index);
                if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                    facts.hypothesis_refs.insert(hypothesis_id);
                }
                facts.confirmations.push(event);
            }
            RehydratedEvent::VetoRaised(event)
                if event.payload.scope == VetoScope::Signal
                    && event.payload.target_id == signal_id =>
            {
                facts.relevant = true;
                facts.veto_indexes.push(index);
                if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                    facts.hypothesis_refs.insert(hypothesis_id);
                }
                facts.vetoes.push(event);
            }
            RehydratedEvent::DecisionFormed(event)
                if event.linkage.signal_id.as_deref() == Some(signal_id) =>
            {
                facts.relevant = true;
                facts
                    .downstream_decision_ids
                    .insert(event.payload.decision_id.clone());
                if let Some(hypothesis_id) = event.linkage.hypothesis_id.clone() {
                    facts.hypothesis_refs.insert(hypothesis_id);
                }
            }
            _ => {}
        }
    }

    Ok(facts)
}

struct DecisionFacts {
    relevant: bool,
    formed_event: Option<EventEnvelope<crate::events::DecisionFormed>>,
    signal_ref: Option<String>,
    signal_refs: BTreeSet<String>,
    vetoes: Vec<EventEnvelope<VetoRaised>>,
    instrument_mismatches: Vec<(String, String)>,
}

fn collect_decision_facts(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<DecisionFacts, QueryError> {
    let mut facts = DecisionFacts {
        relevant: false,
        formed_event: None,
        signal_ref: None,
        signal_refs: BTreeSet::new(),
        vetoes: Vec::new(),
        instrument_mismatches: Vec::new(),
    };

    for stored in events {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::DecisionFormed(event) if event.payload.decision_id == decision_id => {
                facts.relevant = true;
                if let Some(signal_id) = event.linkage.signal_id.clone() {
                    facts.signal_refs.insert(signal_id.clone());
                    if facts.signal_ref.is_none() {
                        facts.signal_ref = Some(signal_id);
                    }
                }
                facts.formed_event = Some(event);
            }
            RehydratedEvent::VetoRaised(event)
                if event.payload.scope == VetoScope::Decision
                    && event.payload.target_id == decision_id =>
            {
                facts.relevant = true;
                facts.vetoes.push(event);
            }
            RehydratedEvent::FillReceived(event)
                if event.payload.decision_id.as_deref() == Some(decision_id)
                    || event.linkage.decision_id.as_deref() == Some(decision_id) =>
            {
                facts.relevant = true;
                if let Some(formed) = facts.formed_event.as_ref() {
                    if formed.payload.instrument != event.payload.instrument {
                        facts.instrument_mismatches.push((
                            event.payload.fill_id.clone(),
                            event.payload.instrument.clone(),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    Ok(facts)
}

struct FillFacts {
    relevant: bool,
    fill_events: Vec<EventEnvelope<FillReceived>>,
    decision_id: Option<String>,
    decision_ids: BTreeSet<String>,
    order_ids: BTreeSet<String>,
    venues: BTreeSet<String>,
    instrument_mismatch: Option<(String, String, String)>,
}

fn collect_fill_facts(events: &[StoredEvent], fill_id: &str) -> Result<FillFacts, QueryError> {
    let mut facts = FillFacts {
        relevant: false,
        fill_events: Vec::new(),
        decision_id: None,
        decision_ids: BTreeSet::new(),
        order_ids: BTreeSet::new(),
        venues: BTreeSet::new(),
        instrument_mismatch: None,
    };

    for stored in events {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::FillReceived(event) if event.payload.fill_id == fill_id => {
                facts.relevant = true;
                facts.order_ids.insert(event.payload.order_id.clone());
                facts.venues.insert(event.payload.venue.clone());

                if let Some(decision_id) = event
                    .payload
                    .decision_id
                    .clone()
                    .or_else(|| event.linkage.decision_id.clone())
                {
                    facts.decision_ids.insert(decision_id.clone());
                    if facts.decision_id.is_none() {
                        facts.decision_id = Some(decision_id);
                    }
                }

                facts.fill_events.push(event);
            }
            _ => {}
        }
    }

    if let (Some(fill), Some(decision_id)) = (facts.fill_events.first(), facts.decision_id.clone())
    {
        for stored in events {
            if let RehydratedEvent::DecisionFormed(event) = RehydratedEvent::try_from(stored)? {
                if event.payload.decision_id == decision_id
                    && event.payload.instrument != fill.payload.instrument
                {
                    facts.instrument_mismatch = Some((
                        decision_id.clone(),
                        event.payload.instrument.clone(),
                        fill.payload.instrument.clone(),
                    ));
                    break;
                }
            }
        }
    }

    Ok(facts)
}
