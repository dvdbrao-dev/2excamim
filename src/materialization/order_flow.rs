use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    codecs::RehydratedEvent,
    commands::RegisterOrderCommand,
    events::{DecisionFormed, Provenance, SourceKind},
    projections::DecisionProjection,
    queries::{PromotionNextStep, PromotionPolicyStatus, QueryService},
};

use super::MaterializationError;

const ORDER_MATERIALIZATION_PRODUCED_BY: &str = "runtime.order_materialization";
const ORDER_MATERIALIZATION_ACTOR: &str = "order_materialization_flow_v1";
const ORDER_MATERIALIZATION_VENUE: &str = "paper";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderMaterializationOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderMaterializationDisposition {
    Eligible,
    Materialized,
    Skipped,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderMaterializationItem {
    pub decision_id: String,
    pub policy_status: String,
    pub disposition: OrderMaterializationDisposition,
    pub candidate_order_id: Option<String>,
    pub persisted: bool,
    pub reasons: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderMaterializationReport {
    pub dry_run: bool,
    pub batch_trace_id: String,
    pub decisions_inspected: usize,
    pub eligible: usize,
    pub skipped: usize,
    pub blocked: usize,
    pub inconsistent: usize,
    pub orders_registered: usize,
    pub duplicates: usize,
    pub items: Vec<OrderMaterializationItem>,
}

pub fn materialize_orders(
    query_service: &QueryService<'_>,
    options: OrderMaterializationOptions,
) -> Result<OrderMaterializationReport, MaterializationError> {
    let batch_trace_id = build_batch_trace_id();
    let decision_projections = query_service.all_decision_projections()?;
    let mut items = Vec::new();
    let mut eligible = 0usize;
    let mut skipped = 0usize;
    let mut blocked = 0usize;
    let mut inconsistent = 0usize;
    let mut orders_registered = 0usize;
    let mut duplicates = 0usize;

    for projection in decision_projections {
        let policy = query_service
            .decision_promotion_policy(&projection.decision_id)?
            .ok_or_else(|| {
                MaterializationError::invalid_state(format!(
                    "decision {} has projection but no promotion policy",
                    projection.decision_id
                ))
            })?;
        let existing_local_order_ids = query_service
            .decision_lineage(&projection.decision_id)?
            .map(|lineage| lineage.downstream_refs.local_order_ids)
            .unwrap_or_default();

        let candidate_order_id = Some(build_order_id(&projection.decision_id));
        let mut reasons = policy.reasons.clone();
        let notes = policy.notes.clone();

        let (disposition, persisted) = if !existing_local_order_ids.is_empty() {
            reasons.insert(
                0,
                format!("ExistingOrderIds({})", existing_local_order_ids.join(",")),
            );
            skipped += 1;
            (OrderMaterializationDisposition::Skipped, false)
        } else {
            match policy.status {
                PromotionPolicyStatus::Eligible
                    if policy.next_step == Some(PromotionNextStep::RegisterOrder) =>
                {
                    eligible += 1;
                    if options.dry_run {
                        (OrderMaterializationDisposition::Eligible, false)
                    } else {
                        let appended = persist_order_for_decision(
                            query_service,
                            &projection,
                            &batch_trace_id,
                            candidate_order_id.as_deref().expect("candidate id exists"),
                        )?;

                        if appended {
                            orders_registered += 1;
                            (OrderMaterializationDisposition::Materialized, true)
                        } else {
                            duplicates += 1;
                            skipped += 1;
                            reasons.insert(0, "DuplicateOrderMaterialization".to_string());
                            (OrderMaterializationDisposition::Skipped, false)
                        }
                    }
                }
                PromotionPolicyStatus::Blocked => {
                    blocked += 1;
                    (OrderMaterializationDisposition::Blocked, false)
                }
                PromotionPolicyStatus::Inconsistent => {
                    inconsistent += 1;
                    (OrderMaterializationDisposition::Inconsistent, false)
                }
                PromotionPolicyStatus::Weak | PromotionPolicyStatus::Frozen => {
                    skipped += 1;
                    (OrderMaterializationDisposition::Skipped, false)
                }
                PromotionPolicyStatus::Eligible => {
                    skipped += 1;
                    reasons.insert(0, "EligibleWithoutRegisterOrderNextStep".to_string());
                    (OrderMaterializationDisposition::Skipped, false)
                }
            }
        };

        items.push(OrderMaterializationItem {
            decision_id: projection.decision_id.clone(),
            policy_status: format!("{:?}", policy.status),
            disposition,
            candidate_order_id,
            persisted,
            reasons,
            notes,
        });
    }

    Ok(OrderMaterializationReport {
        dry_run: options.dry_run,
        batch_trace_id,
        decisions_inspected: items.len(),
        eligible,
        skipped,
        blocked,
        inconsistent,
        orders_registered,
        duplicates,
        items,
    })
}

fn persist_order_for_decision(
    query_service: &QueryService<'_>,
    projection: &DecisionProjection,
    batch_trace_id: &str,
    order_id: &str,
) -> Result<bool, MaterializationError> {
    let source_event = latest_decision_formed_event(query_service, &projection.decision_id)?
        .ok_or_else(|| {
            MaterializationError::invalid_state(format!(
                "decision {} is eligible but has no decision.formed event",
                projection.decision_id
            ))
        })?;

    let command = RegisterOrderCommand {
        produced_by: ORDER_MATERIALIZATION_PRODUCED_BY.to_string(),
        provenance: Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: Some(format!(
                "materialize-orders://decision/{}",
                projection.decision_id
            )),
            producer_run_id: Some(batch_trace_id.to_string()),
            actor: Some(ORDER_MATERIALIZATION_ACTOR.to_string()),
            trace_id: Some(order_id.to_string()),
            notes: Some(serde_json::to_string(&json!({
                "decision_id": projection.decision_id,
                "materialization_flow": "order_materialization_flow_v1",
                "venue": ORDER_MATERIALIZATION_VENUE,
            }))?),
        },
        order_id: order_id.to_string(),
        decision_id: Some(source_event.payload.decision_id.clone()),
        hypothesis_id: source_event.linkage.hypothesis_id.clone(),
        signal_id: source_event.linkage.signal_id.clone(),
        instrument: source_event.payload.instrument.clone(),
        venue: ORDER_MATERIALIZATION_VENUE.to_string(),
        parent_event_id: Some(source_event.event_id.clone()),
        correlation_id: source_event
            .linkage
            .correlation_id
            .clone()
            .or_else(|| Some(projection.decision_id.clone())),
    };

    let store = query_service.store_ref();
    let envelope = command.execute()?;
    let stored = crate::store::StoredEvent::try_from(envelope)?;
    Ok(store.append_event(&stored)?)
}

fn latest_decision_formed_event(
    query_service: &QueryService<'_>,
    decision_id: &str,
) -> Result<Option<crate::events::EventEnvelope<DecisionFormed>>, MaterializationError> {
    let timeline = query_service.timeline_for_decision(decision_id)?;
    let mut latest = None;

    for stored in timeline {
        if let RehydratedEvent::DecisionFormed(event) = RehydratedEvent::try_from(stored)? {
            latest = Some(event);
        }
    }

    Ok(latest)
}

fn build_order_id(decision_id: &str) -> String {
    format!("order-{decision_id}")
}

fn build_batch_trace_id() -> String {
    format!(
        "order-materialization-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    )
}
