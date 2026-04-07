use crate::store::StoredEvent;

use super::{
    decision_execution_boundary, decision_governance, decision_lineage, order_lifecycle,
    signal_governance, DecisionLineageStatus, ExecutionBoundaryStatus, GovernanceRef,
    GovernanceRefType, GovernanceStatus, OrderLifecycleStatus, QueryError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionPolicyStatus {
    Eligible,
    Weak,
    Blocked,
    Frozen,
    Inconsistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionNextStep {
    FormDecision,
    RegisterOrder,
    SubmitOrder,
    ObserveExecution,
    ReconcileObservedExecution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalPromotionReport {
    pub ref_id: String,
    pub ref_type: GovernanceRefType,
    pub status: PromotionPolicyStatus,
    pub next_step: Option<PromotionNextStep>,
    pub reasons: Vec<String>,
    pub supporting_refs: Vec<GovernanceRef>,
    pub blocking_refs: Vec<GovernanceRef>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionPromotionReport {
    pub ref_id: String,
    pub ref_type: GovernanceRefType,
    pub status: PromotionPolicyStatus,
    pub next_step: Option<PromotionNextStep>,
    pub reasons: Vec<String>,
    pub supporting_refs: Vec<GovernanceRef>,
    pub blocking_refs: Vec<GovernanceRef>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderPromotionReport {
    pub ref_id: String,
    pub ref_type: GovernanceRefType,
    pub status: PromotionPolicyStatus,
    pub next_step: Option<PromotionNextStep>,
    pub reasons: Vec<String>,
    pub supporting_refs: Vec<GovernanceRef>,
    pub blocking_refs: Vec<GovernanceRef>,
    pub notes: Vec<String>,
}

pub fn signal_promotion_policy(
    events: &[StoredEvent],
    signal_id: &str,
) -> Result<Option<SignalPromotionReport>, QueryError> {
    let Some(governance) = signal_governance(events, signal_id)? else {
        return Ok(None);
    };

    let (status, next_step) = match governance.status {
        GovernanceStatus::Eligible => (
            PromotionPolicyStatus::Eligible,
            Some(PromotionNextStep::FormDecision),
        ),
        GovernanceStatus::Weak => (
            PromotionPolicyStatus::Weak,
            Some(PromotionNextStep::FormDecision),
        ),
        GovernanceStatus::Blocked => (PromotionPolicyStatus::Blocked, None),
        GovernanceStatus::Inconsistent => (PromotionPolicyStatus::Inconsistent, None),
    };

    let mut notes = governance.notes;
    notes.push(
        "signal promotion policy v1 is derived from signal governance; no separate freeze rule is defined for signals"
            .to_string(),
    );

    Ok(Some(SignalPromotionReport {
        ref_id: governance.ref_id,
        ref_type: governance.ref_type,
        status,
        next_step,
        reasons: governance
            .reasons
            .into_iter()
            .map(|reason| format!("{reason:?}"))
            .collect(),
        supporting_refs: governance.supporting_refs,
        blocking_refs: governance.blocking_refs,
        notes,
    }))
}

pub fn decision_promotion_policy(
    events: &[StoredEvent],
    decision_id: &str,
) -> Result<Option<DecisionPromotionReport>, QueryError> {
    let governance = decision_governance(events, decision_id)?;
    let lineage = decision_lineage(events, decision_id)?;
    let boundary = decision_execution_boundary(events, decision_id)?;
    let lineage_local_order_ids = lineage
        .as_ref()
        .map(|report| report.downstream_refs.local_order_ids.clone())
        .unwrap_or_default();
    let lineage_submitted_order_ids = lineage
        .as_ref()
        .map(|report| report.downstream_refs.submitted_order_ids.clone())
        .unwrap_or_default();

    if governance.is_none() && lineage.is_none() && boundary.is_none() {
        return Ok(None);
    }

    let mut reasons = Vec::new();
    let mut supporting_refs = Vec::new();
    let mut blocking_refs = Vec::new();
    let mut notes = Vec::new();
    let mut next_step = None;

    if let Some(governance) = governance {
        reasons.extend(
            governance
                .reasons
                .into_iter()
                .map(|reason| format!("{reason:?}")),
        );
        supporting_refs.extend(governance.supporting_refs);
        blocking_refs.extend(governance.blocking_refs);
        notes.extend(governance.notes);

        match governance.status {
            GovernanceStatus::Inconsistent => {
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Inconsistent,
                    next_step: None,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            GovernanceStatus::Blocked => {
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Blocked,
                    next_step: None,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            GovernanceStatus::Weak => {
                next_step = Some(PromotionNextStep::RegisterOrder);
            }
            GovernanceStatus::Eligible => {}
        }
    }

    if let Some(lineage) = lineage {
        reasons.push(format!("DecisionLineage::{:?}", lineage.status));
        notes.extend(lineage.notes);

        match lineage.status {
            DecisionLineageStatus::Inconsistent => {
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Inconsistent,
                    next_step: None,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            DecisionLineageStatus::Blocked => {
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Blocked,
                    next_step: None,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            DecisionLineageStatus::Weak => {
                if next_step.is_none() {
                    next_step = Some(PromotionNextStep::RegisterOrder);
                }
            }
            DecisionLineageStatus::Supported => {}
        }
    }

    if let Some(boundary) = boundary {
        reasons.push(format!("ExecutionBoundary::{:?}", boundary.status));
        notes.extend(boundary.notes.clone());

        match boundary.status {
            ExecutionBoundaryStatus::Inconsistent => {
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Inconsistent,
                    next_step: None,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            ExecutionBoundaryStatus::Blocked => {
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Blocked,
                    next_step: None,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            ExecutionBoundaryStatus::Clear => {
                next_step = Some(PromotionNextStep::ReconcileObservedExecution);
                notes.push(
                    "decision promotion is frozen because execution has already been observed under the current model"
                        .to_string(),
                );
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Frozen,
                    next_step,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }
            ExecutionBoundaryStatus::Weak => {}
        }

        if boundary.observed_order_ids.is_empty() && boundary.submitted_order_ids.is_empty() {
            if !lineage_submitted_order_ids.is_empty() {
                next_step = Some(PromotionNextStep::ObserveExecution);
                notes.push(
                    "decision promotion is frozen because local submission is already in flight and the current model should wait for execution follow-up"
                        .to_string(),
                );
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Frozen,
                    next_step,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }

            if !lineage_local_order_ids.is_empty() {
                next_step = Some(PromotionNextStep::SubmitOrder);
                notes.push(
                    "decision promotion is frozen because local order activity exists but submission is not yet visible"
                        .to_string(),
                );
                return Ok(Some(DecisionPromotionReport {
                    ref_id: decision_id.to_string(),
                    ref_type: GovernanceRefType::Decision,
                    status: PromotionPolicyStatus::Frozen,
                    next_step,
                    reasons,
                    supporting_refs,
                    blocking_refs,
                    notes,
                }));
            }

            next_step = Some(PromotionNextStep::RegisterOrder);
            return Ok(Some(DecisionPromotionReport {
                ref_id: decision_id.to_string(),
                ref_type: GovernanceRefType::Decision,
                status: PromotionPolicyStatus::Eligible,
                next_step,
                reasons,
                supporting_refs,
                blocking_refs,
                notes,
            }));
        }

        if boundary.submitted_order_ids.is_empty() {
            next_step = Some(PromotionNextStep::SubmitOrder);
            notes.push(
                "decision promotion is frozen because local order activity exists but submission is not yet visible"
                    .to_string(),
            );
            return Ok(Some(DecisionPromotionReport {
                ref_id: decision_id.to_string(),
                ref_type: GovernanceRefType::Decision,
                status: PromotionPolicyStatus::Frozen,
                next_step,
                reasons,
                supporting_refs,
                blocking_refs,
                notes,
            }));
        }

        next_step = Some(PromotionNextStep::ObserveExecution);
        notes.push(
            "decision promotion is frozen because local submission is already in flight and the current model should wait for execution follow-up"
                .to_string(),
        );
        return Ok(Some(DecisionPromotionReport {
            ref_id: decision_id.to_string(),
            ref_type: GovernanceRefType::Decision,
            status: PromotionPolicyStatus::Frozen,
            next_step,
            reasons,
            supporting_refs,
            blocking_refs,
            notes,
        }));
    }

    Ok(Some(DecisionPromotionReport {
        ref_id: decision_id.to_string(),
        ref_type: GovernanceRefType::Decision,
        status: PromotionPolicyStatus::Weak,
        next_step,
        reasons,
        supporting_refs,
        blocking_refs,
        notes,
    }))
}

pub fn order_promotion_policy(
    events: &[StoredEvent],
    order_id: &str,
) -> Result<Option<OrderPromotionReport>, QueryError> {
    let Some(lifecycle) = order_lifecycle(events, order_id)? else {
        return Ok(None);
    };

    let mut reasons = lifecycle
        .reasons
        .into_iter()
        .map(|reason| format!("{reason:?}"))
        .collect::<Vec<_>>();
    let mut notes = lifecycle.notes;
    let mut supporting_refs = vec![GovernanceRef {
        ref_id: order_id.to_string(),
        ref_type: GovernanceRefType::Order,
    }];
    let mut blocking_refs = Vec::new();

    for decision_id in &lifecycle.decision_refs {
        supporting_refs.push(GovernanceRef {
            ref_id: decision_id.clone(),
            ref_type: GovernanceRefType::Decision,
        });
    }
    for fill_id in &lifecycle.observed_fill_ids {
        supporting_refs.push(GovernanceRef {
            ref_id: fill_id.clone(),
            ref_type: GovernanceRefType::Fill,
        });
    }

    if lifecycle.decision_refs.len() == 1 {
        let decision_id = lifecycle
            .decision_refs
            .first()
            .expect("single decision exists")
            .clone();
        if let Some(governance) = decision_governance(events, &decision_id)? {
            reasons.push(format!("DecisionGovernance::{:?}", governance.status));
            match governance.status {
                GovernanceStatus::Inconsistent => {
                    blocking_refs.push(GovernanceRef {
                        ref_id: decision_id,
                        ref_type: GovernanceRefType::Decision,
                    });
                    return Ok(Some(OrderPromotionReport {
                        ref_id: order_id.to_string(),
                        ref_type: GovernanceRefType::Order,
                        status: PromotionPolicyStatus::Inconsistent,
                        next_step: None,
                        reasons,
                        supporting_refs,
                        blocking_refs,
                        notes,
                    }));
                }
                GovernanceStatus::Blocked => {
                    blocking_refs.push(GovernanceRef {
                        ref_id: decision_id,
                        ref_type: GovernanceRefType::Decision,
                    });
                    return Ok(Some(OrderPromotionReport {
                        ref_id: order_id.to_string(),
                        ref_type: GovernanceRefType::Order,
                        status: PromotionPolicyStatus::Blocked,
                        next_step: None,
                        reasons,
                        supporting_refs,
                        blocking_refs,
                        notes,
                    }));
                }
                GovernanceStatus::Weak | GovernanceStatus::Eligible => {}
            }
        }
    }

    let (status, next_step) = match lifecycle.status {
        OrderLifecycleStatus::Registered => {
            notes.push(
                "order policy freezes locally registered orders until explicit submission is visible"
                    .to_string(),
            );
            (
                PromotionPolicyStatus::Frozen,
                Some(PromotionNextStep::SubmitOrder),
            )
        }
        OrderLifecycleStatus::Submitted => (
            PromotionPolicyStatus::Eligible,
            Some(PromotionNextStep::ObserveExecution),
        ),
        OrderLifecycleStatus::ObservedWithFills => (
            PromotionPolicyStatus::Eligible,
            Some(PromotionNextStep::ReconcileObservedExecution),
        ),
        OrderLifecycleStatus::Weak => (
            PromotionPolicyStatus::Weak,
            Some(PromotionNextStep::SubmitOrder),
        ),
        OrderLifecycleStatus::Inconsistent => (PromotionPolicyStatus::Inconsistent, None),
    };

    Ok(Some(OrderPromotionReport {
        ref_id: order_id.to_string(),
        ref_type: GovernanceRefType::Order,
        status,
        next_step,
        reasons,
        supporting_refs,
        blocking_refs,
        notes,
    }))
}
