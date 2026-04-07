use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    codecs::RehydratedEvent,
    commands::FormDecisionCommand,
    events::{DecisionAction, Provenance, SignalGenerated, SourceKind},
    projections::SignalProjection,
    queries::{PromotionNextStep, PromotionPolicyStatus, QueryService},
};

use super::MaterializationError;

const DECISION_MATERIALIZATION_PRODUCED_BY: &str = "runtime.decision_materialization";
const DECISION_MATERIALIZATION_ACTOR: &str = "decision_materialization_flow_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecisionMaterializationOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionMaterializationDisposition {
    Eligible,
    Materialized,
    Skipped,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionMaterializationItem {
    pub signal_id: String,
    pub policy_status: String,
    pub disposition: DecisionMaterializationDisposition,
    pub candidate_decision_id: Option<String>,
    pub persisted: bool,
    pub reasons: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionMaterializationReport {
    pub dry_run: bool,
    pub batch_trace_id: String,
    pub signals_inspected: usize,
    pub eligible: usize,
    pub skipped: usize,
    pub blocked: usize,
    pub inconsistent: usize,
    pub decisions_materialized: usize,
    pub duplicates: usize,
    pub items: Vec<DecisionMaterializationItem>,
}

pub fn materialize_decisions(
    query_service: &QueryService<'_>,
    options: DecisionMaterializationOptions,
) -> Result<DecisionMaterializationReport, MaterializationError> {
    let batch_trace_id = build_batch_trace_id();
    let signal_projections = query_service.all_signal_projections()?;
    let mut items = Vec::new();
    let mut eligible = 0usize;
    let mut skipped = 0usize;
    let mut blocked = 0usize;
    let mut inconsistent = 0usize;
    let mut decisions_materialized = 0usize;
    let mut duplicates = 0usize;

    for projection in signal_projections {
        let policy = query_service
            .signal_promotion_policy(&projection.signal_id)?
            .ok_or_else(|| {
                MaterializationError::invalid_state(format!(
                    "signal {} has projection but no promotion policy",
                    projection.signal_id
                ))
            })?;

        let candidate_decision_id = Some(build_decision_id(&projection.signal_id));
        let mut reasons = policy.reasons.clone();
        let notes = policy.notes.clone();

        let (disposition, persisted) = if !projection.decision_ids.is_empty() {
            reasons.insert(
                0,
                format!("ExistingDecisionIds({})", projection.decision_ids.join(",")),
            );
            skipped += 1;
            (DecisionMaterializationDisposition::Skipped, false)
        } else {
            match policy.status {
                PromotionPolicyStatus::Eligible
                    if policy.next_step == Some(PromotionNextStep::FormDecision) =>
                {
                    eligible += 1;
                    if options.dry_run {
                        (DecisionMaterializationDisposition::Eligible, false)
                    } else {
                        let appended = persist_decision_for_signal(
                            query_service,
                            &projection,
                            &batch_trace_id,
                            candidate_decision_id
                                .as_deref()
                                .expect("candidate id exists"),
                        )?;
                        if appended {
                            decisions_materialized += 1;
                            (DecisionMaterializationDisposition::Materialized, true)
                        } else {
                            duplicates += 1;
                            skipped += 1;
                            reasons.insert(0, "DuplicateDecisionMaterialization".to_string());
                            (DecisionMaterializationDisposition::Skipped, false)
                        }
                    }
                }
                PromotionPolicyStatus::Blocked => {
                    blocked += 1;
                    (DecisionMaterializationDisposition::Blocked, false)
                }
                PromotionPolicyStatus::Inconsistent => {
                    inconsistent += 1;
                    (DecisionMaterializationDisposition::Inconsistent, false)
                }
                PromotionPolicyStatus::Weak | PromotionPolicyStatus::Frozen => {
                    skipped += 1;
                    (DecisionMaterializationDisposition::Skipped, false)
                }
                PromotionPolicyStatus::Eligible => {
                    skipped += 1;
                    reasons.insert(0, "EligibleWithoutFormDecisionNextStep".to_string());
                    (DecisionMaterializationDisposition::Skipped, false)
                }
            }
        };

        items.push(DecisionMaterializationItem {
            signal_id: projection.signal_id.clone(),
            policy_status: format!("{:?}", policy.status),
            disposition,
            candidate_decision_id,
            persisted,
            reasons,
            notes,
        });
    }

    Ok(DecisionMaterializationReport {
        dry_run: options.dry_run,
        batch_trace_id,
        signals_inspected: items.len(),
        eligible,
        skipped,
        blocked,
        inconsistent,
        decisions_materialized,
        duplicates,
        items,
    })
}

fn persist_decision_for_signal(
    query_service: &QueryService<'_>,
    projection: &SignalProjection,
    batch_trace_id: &str,
    decision_id: &str,
) -> Result<bool, MaterializationError> {
    let source_event = latest_signal_generated_event(query_service, &projection.signal_id)?
        .ok_or_else(|| {
            MaterializationError::invalid_state(format!(
                "signal {} is eligible but has no signal.generated event",
                projection.signal_id
            ))
        })?;

    let command = FormDecisionCommand {
        produced_by: DECISION_MATERIALIZATION_PRODUCED_BY.to_string(),
        provenance: Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: Some(format!(
                "materialize-decisions://signal/{}",
                projection.signal_id
            )),
            producer_run_id: Some(batch_trace_id.to_string()),
            actor: Some(DECISION_MATERIALIZATION_ACTOR.to_string()),
            trace_id: Some(decision_id.to_string()),
            notes: Some(serde_json::to_string(&json!({
                "signal_id": projection.signal_id,
                "materialization_flow": "decision_materialization_flow_v1",
            }))?),
        },
        decision_id: decision_id.to_string(),
        hypothesis_id: source_event
            .payload
            .hypothesis_id
            .clone()
            .or_else(|| source_event.linkage.hypothesis_id.clone()),
        signal_id: Some(source_event.payload.signal_id.clone()),
        instrument: source_event.payload.instrument.clone(),
        action: DecisionAction::Enter,
        side: Some(source_event.payload.side),
        size_hint: None,
        rationale: Some(format!(
            "materialized from signal {} via decision materialization flow v1",
            projection.signal_id
        )),
        parent_event_id: Some(source_event.event_id.clone()),
        correlation_id: source_event
            .linkage
            .correlation_id
            .clone()
            .or_else(|| Some(projection.signal_id.clone())),
    };

    let store = query_service.store_ref();
    let envelope = command.execute()?;
    let stored = crate::store::StoredEvent::try_from(envelope)?;
    Ok(store.append_event(&stored)?)
}

fn latest_signal_generated_event(
    query_service: &QueryService<'_>,
    signal_id: &str,
) -> Result<Option<crate::events::EventEnvelope<SignalGenerated>>, MaterializationError> {
    let timeline = query_service.timeline_for_signal(signal_id)?;
    let mut latest = None;

    for stored in timeline {
        if let RehydratedEvent::SignalGenerated(event) = RehydratedEvent::try_from(stored)? {
            latest = Some(event);
        }
    }

    Ok(latest)
}

fn build_decision_id(signal_id: &str) -> String {
    format!("decision-{signal_id}")
}

fn build_batch_trace_id() -> String {
    format!(
        "decision-materialization-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    )
}
