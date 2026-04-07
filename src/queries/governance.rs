use crate::store::StoredEvent;

use super::{
    decision_execution_boundary, decision_lineage, decision_readiness, signal_readiness,
    DecisionLineageReason, DecisionLineageStatus, DecisionReadinessReason, DecisionReadinessStatus,
    ExecutionBoundaryReason, ExecutionBoundaryStatus, QueryError, SignalReadinessReason,
    SignalReadinessStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernanceRefType {
    Signal,
    Decision,
    Hypothesis,
    Veto,
    Order,
    Fill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernanceRef {
    pub ref_id: String,
    pub ref_type: GovernanceRefType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernanceStatus {
    Eligible,
    Weak,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalGovernanceReport {
    pub ref_id: String,
    pub ref_type: GovernanceRefType,
    pub status: GovernanceStatus,
    pub reasons: Vec<SignalGovernanceReason>,
    pub supporting_refs: Vec<GovernanceRef>,
    pub blocking_refs: Vec<GovernanceRef>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalGovernanceReason {
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
    DownstreamDecisionObservedWithoutHealthySignal {
        decision_id: String,
    },
    ConflictingHypothesisReferences {
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionGovernanceReport {
    pub ref_id: String,
    pub ref_type: GovernanceRefType,
    pub status: GovernanceStatus,
    pub reasons: Vec<DecisionGovernanceReason>,
    pub supporting_refs: Vec<GovernanceRef>,
    pub blocking_refs: Vec<GovernanceRef>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionGovernanceReason {
    DecisionFormedObserved,
    MissingDecisionFormed,
    UpstreamSignalEligible {
        signal_id: String,
    },
    UpstreamSignalWeak {
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
    DecisionVetoObserved {
        veto_id: String,
        reason_code: String,
    },
    ExecutionBoundaryClear,
    ExecutionBoundaryPending,
    ExecutionBoundaryWeak,
    ExecutionBoundaryBlocked,
    ExecutionBoundaryInconsistent,
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

pub fn signal_governance(
    events: &[StoredEvent],
    signal_id: &str,
) -> Result<Option<SignalGovernanceReport>, QueryError> {
    let Some(readiness) = signal_readiness(events, signal_id)? else {
        return Ok(None);
    };

    let mut reasons = Vec::new();
    let mut supporting_refs = Vec::new();
    let mut blocking_refs = Vec::new();

    push_ref(
        &mut supporting_refs,
        signal_id.to_string(),
        GovernanceRefType::Signal,
    );

    for reason in readiness.reasons {
        match reason {
            SignalReadinessReason::SignalGeneratedObserved => {
                reasons.push(SignalGovernanceReason::SignalGeneratedObserved);
            }
            SignalReadinessReason::SignalConfirmedObserved { confirmed_by } => {
                reasons.push(SignalGovernanceReason::SignalConfirmedObserved { confirmed_by });
            }
            SignalReadinessReason::SignalVetoObserved {
                veto_id,
                reason_code,
            } => {
                push_ref(&mut blocking_refs, veto_id.clone(), GovernanceRefType::Veto);
                reasons.push(SignalGovernanceReason::SignalVetoObserved {
                    veto_id,
                    reason_code,
                });
            }
            SignalReadinessReason::MissingSignalGenerated => {
                reasons.push(SignalGovernanceReason::MissingSignalGenerated);
            }
            SignalReadinessReason::SignalConfirmationBeforeGeneration => {
                reasons.push(SignalGovernanceReason::SignalConfirmationBeforeGeneration);
            }
            SignalReadinessReason::SignalVetoBeforeGeneration => {
                reasons.push(SignalGovernanceReason::SignalVetoBeforeGeneration);
            }
            SignalReadinessReason::DecisionObservedWithoutConfirmedSignal { decision_id } => {
                push_ref(
                    &mut blocking_refs,
                    decision_id.clone(),
                    GovernanceRefType::Decision,
                );
                reasons.push(
                    SignalGovernanceReason::DownstreamDecisionObservedWithoutHealthySignal {
                        decision_id,
                    },
                );
            }
            SignalReadinessReason::ConflictingHypothesisReferences { values } => {
                reasons.push(SignalGovernanceReason::ConflictingHypothesisReferences { values });
            }
        }
    }

    let status = match readiness.status {
        SignalReadinessStatus::ReadyForDecision => GovernanceStatus::Eligible,
        SignalReadinessStatus::GeneratedUnconfirmed => GovernanceStatus::Weak,
        SignalReadinessStatus::BlockedByVeto => GovernanceStatus::Blocked,
        SignalReadinessStatus::Inconsistent => GovernanceStatus::Inconsistent,
    };

    Ok(Some(SignalGovernanceReport {
        ref_id: signal_id.to_string(),
        ref_type: GovernanceRefType::Signal,
        status,
        reasons,
        supporting_refs,
        blocking_refs,
        notes: vec![
            "governance v1 derives signal eligibility from readiness semantics over the event log"
                .to_string(),
        ],
    }))
}

pub fn decision_governance(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<Option<DecisionGovernanceReport>, QueryError> {
    let readiness = decision_readiness(events, decision_id)?;
    let lineage = decision_lineage(events, decision_id)?;
    let boundary = decision_execution_boundary(events, decision_id)?;

    if readiness.is_none() && lineage.is_none() && boundary.is_none() {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    let mut supporting_refs = Vec::new();
    let mut blocking_refs = Vec::new();
    let mut notes = Vec::new();
    let readiness_status = readiness.as_ref().map(|report| report.status.clone());
    let lineage_status = lineage.as_ref().map(|report| report.status.clone());
    let boundary_status = boundary.as_ref().map(|report| report.status);

    push_ref(
        &mut supporting_refs,
        decision_id.to_string(),
        GovernanceRefType::Decision,
    );

    if let Some(readiness) = readiness {
        for reason in readiness.reasons {
            match reason {
                DecisionReadinessReason::DecisionFormedObserved => {
                    reasons.push(DecisionGovernanceReason::DecisionFormedObserved);
                }
                DecisionReadinessReason::DecisionVetoObserved {
                    veto_id,
                    reason_code,
                } => {
                    push_ref(&mut blocking_refs, veto_id.clone(), GovernanceRefType::Veto);
                    reasons.push(DecisionGovernanceReason::DecisionVetoObserved {
                        veto_id,
                        reason_code,
                    });
                }
                DecisionReadinessReason::UpstreamSignalReady { signal_id } => {
                    push_ref(
                        &mut supporting_refs,
                        signal_id.clone(),
                        GovernanceRefType::Signal,
                    );
                    reasons.push(DecisionGovernanceReason::UpstreamSignalEligible { signal_id });
                }
                DecisionReadinessReason::UpstreamSignalGeneratedUnconfirmed { signal_id } => {
                    push_ref(
                        &mut supporting_refs,
                        signal_id.clone(),
                        GovernanceRefType::Signal,
                    );
                    reasons.push(DecisionGovernanceReason::UpstreamSignalWeak { signal_id });
                }
                DecisionReadinessReason::UpstreamSignalBlocked { signal_id } => {
                    push_ref(
                        &mut blocking_refs,
                        signal_id.clone(),
                        GovernanceRefType::Signal,
                    );
                    reasons.push(DecisionGovernanceReason::UpstreamSignalBlocked { signal_id });
                }
                DecisionReadinessReason::UpstreamSignalInconsistent { signal_id } => {
                    push_ref(
                        &mut blocking_refs,
                        signal_id.clone(),
                        GovernanceRefType::Signal,
                    );
                    reasons
                        .push(DecisionGovernanceReason::UpstreamSignalInconsistent { signal_id });
                }
                DecisionReadinessReason::MissingDecisionFormed => {
                    reasons.push(DecisionGovernanceReason::MissingDecisionFormed);
                }
                DecisionReadinessReason::MissingUpstreamSignalReference => {
                    reasons.push(DecisionGovernanceReason::MissingUpstreamSignalReference);
                }
                DecisionReadinessReason::ConflictingSignalReferences { values } => {
                    reasons.push(DecisionGovernanceReason::ConflictingSignalReferences { values });
                }
                DecisionReadinessReason::FillInstrumentMismatch {
                    fill_id,
                    fill_instrument,
                } => {
                    push_ref(&mut blocking_refs, fill_id.clone(), GovernanceRefType::Fill);
                    reasons.push(DecisionGovernanceReason::FillInstrumentMismatch {
                        fill_id,
                        fill_instrument,
                    });
                }
            }
        }
    }

    if let Some(lineage) = lineage {
        for signal_id in lineage.upstream_refs.signal_ids {
            push_ref(&mut supporting_refs, signal_id, GovernanceRefType::Signal);
        }
        for hypothesis_id in lineage.upstream_refs.hypothesis_ids {
            push_ref(
                &mut supporting_refs,
                hypothesis_id,
                GovernanceRefType::Hypothesis,
            );
        }
        for veto in lineage.upstream_refs.decision_vetoes {
            push_ref(&mut blocking_refs, veto.veto_id, GovernanceRefType::Veto);
        }
        for veto in lineage.upstream_refs.signal_vetoes {
            push_ref(&mut blocking_refs, veto.veto_id, GovernanceRefType::Veto);
            push_ref(
                &mut blocking_refs,
                veto.target_id,
                GovernanceRefType::Signal,
            );
        }
        for order_id in lineage.downstream_refs.local_order_ids {
            push_ref(&mut supporting_refs, order_id, GovernanceRefType::Order);
        }
        for fill_id in lineage.downstream_refs.fill_ids {
            push_ref(&mut supporting_refs, fill_id, GovernanceRefType::Fill);
        }

        notes.extend(lineage.notes);

        for reason in lineage.reasons {
            match reason {
                DecisionLineageReason::HypothesisTraced { hypothesis_id } => {
                    reasons.push(DecisionGovernanceReason::HypothesisTraced { hypothesis_id });
                }
                DecisionLineageReason::ConflictingHypothesisReferences { values } => {
                    reasons
                        .push(DecisionGovernanceReason::ConflictingHypothesisReferences { values });
                }
                DecisionLineageReason::DirectDecisionVetoObserved {
                    veto_id,
                    reason_code,
                } => {
                    reasons.push(DecisionGovernanceReason::DecisionVetoObserved {
                        veto_id,
                        reason_code,
                    });
                }
                DecisionLineageReason::LocalOrderRegistered { order_id, venue } => {
                    reasons
                        .push(DecisionGovernanceReason::LocalOrderRegistered { order_id, venue });
                }
                DecisionLineageReason::DownstreamFillObserved { fill_id, order_id } => {
                    reasons.push(DecisionGovernanceReason::DownstreamFillObserved {
                        fill_id,
                        order_id,
                    });
                }
                DecisionLineageReason::FillInstrumentMismatch {
                    fill_id,
                    fill_instrument,
                } => {
                    reasons.push(DecisionGovernanceReason::FillInstrumentMismatch {
                        fill_id,
                        fill_instrument,
                    });
                }
                DecisionLineageReason::DecisionFormedObserved
                | DecisionLineageReason::MissingDecisionFormed
                | DecisionLineageReason::UpstreamSignalSupported { .. }
                | DecisionLineageReason::UpstreamSignalGeneratedUnconfirmed { .. }
                | DecisionLineageReason::UpstreamSignalBlocked { .. }
                | DecisionLineageReason::UpstreamSignalInconsistent { .. }
                | DecisionLineageReason::MissingUpstreamSignalReference
                | DecisionLineageReason::ConflictingSignalReferences { .. } => {}
            }
        }
    }

    let boundary_allows_progress = boundary.as_ref().is_some_and(allows_decision_progress);

    if let Some(boundary) = boundary {
        notes.extend(boundary.notes);

        match boundary.status {
            ExecutionBoundaryStatus::Clear => {
                reasons.push(DecisionGovernanceReason::ExecutionBoundaryClear);
            }
            ExecutionBoundaryStatus::Weak if is_pending_execution_boundary(&boundary.reasons) => {
                reasons.push(DecisionGovernanceReason::ExecutionBoundaryPending);
            }
            ExecutionBoundaryStatus::Weak => {
                reasons.push(DecisionGovernanceReason::ExecutionBoundaryWeak);
            }
            ExecutionBoundaryStatus::Blocked => {
                reasons.push(DecisionGovernanceReason::ExecutionBoundaryBlocked);
            }
            ExecutionBoundaryStatus::Inconsistent => {
                reasons.push(DecisionGovernanceReason::ExecutionBoundaryInconsistent);
            }
        }
    }

    notes.push(
        "governance v1 does not model a separate frozen state; veto and inconsistency remain the current blockers"
            .to_string(),
    );
    notes.sort();
    notes.dedup();

    let status = if matches!(
        readiness_status,
        Some(DecisionReadinessStatus::Inconsistent)
    ) || matches!(lineage_status, Some(DecisionLineageStatus::Inconsistent))
        || matches!(boundary_status, Some(ExecutionBoundaryStatus::Inconsistent))
    {
        GovernanceStatus::Inconsistent
    } else if matches!(readiness_status, Some(DecisionReadinessStatus::Blocked))
        || matches!(lineage_status, Some(DecisionLineageStatus::Blocked))
        || matches!(boundary_status, Some(ExecutionBoundaryStatus::Blocked))
    {
        GovernanceStatus::Blocked
    } else if matches!(readiness_status, Some(DecisionReadinessStatus::Ready))
        && matches!(lineage_status, Some(DecisionLineageStatus::Supported))
        && boundary_allows_progress
    {
        GovernanceStatus::Eligible
    } else {
        GovernanceStatus::Weak
    };

    Ok(Some(DecisionGovernanceReport {
        ref_id: decision_id.to_string(),
        ref_type: GovernanceRefType::Decision,
        status,
        reasons,
        supporting_refs,
        blocking_refs,
        notes,
    }))
}

fn allows_decision_progress(report: &super::ExecutionBoundaryReport) -> bool {
    matches!(report.status, ExecutionBoundaryStatus::Clear)
        || matches!(report.status, ExecutionBoundaryStatus::Weak)
            && is_pending_execution_boundary(&report.reasons)
}

fn is_pending_execution_boundary(reasons: &[ExecutionBoundaryReason]) -> bool {
    let saw_no_execution = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::NoExecutionObservedForDecision
        )
    });
    let has_problematic_reason = reasons.iter().any(|reason| {
        matches!(
            reason,
            ExecutionBoundaryReason::MissingLocalOrderRegistration { .. }
                | ExecutionBoundaryReason::AmbiguousDecisionReferences { .. }
                | ExecutionBoundaryReason::AmbiguousExternalOrderReferences { .. }
                | ExecutionBoundaryReason::FillDecisionInstrumentMismatch { .. }
                | ExecutionBoundaryReason::BlockedDecisionHasObservedFill { .. }
                | ExecutionBoundaryReason::DecisionLineageWeak { .. }
                | ExecutionBoundaryReason::DecisionLineageBlocked { .. }
                | ExecutionBoundaryReason::DecisionLineageInconsistent { .. }
                | ExecutionBoundaryReason::DecisionNotFound { .. }
                | ExecutionBoundaryReason::ExternalOrderReferenceOnly { .. }
                | ExecutionBoundaryReason::LocalOrderReferencesMissingDecision { .. }
        )
    });

    saw_no_execution && !has_problematic_reason
}

fn push_ref(refs: &mut Vec<GovernanceRef>, ref_id: String, ref_type: GovernanceRefType) {
    if refs
        .iter()
        .any(|existing| existing.ref_id == ref_id && existing.ref_type == ref_type)
    {
        return;
    }

    refs.push(GovernanceRef { ref_id, ref_type });
}
