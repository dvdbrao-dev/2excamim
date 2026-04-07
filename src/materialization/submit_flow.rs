use std::collections::BTreeSet;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    codecs::RehydratedEvent,
    commands::SubmitOrderCommand,
    events::{OrderRegistered, Provenance, SourceKind},
    queries::{PromotionNextStep, PromotionPolicyStatus, QueryService},
};

use super::MaterializationError;

const ORDER_SUBMISSION_PRODUCED_BY: &str = "runtime.order_submission";
const ORDER_SUBMISSION_ACTOR: &str = "order_submission_boundary_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderSubmissionOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSubmissionDisposition {
    Eligible,
    Submitted,
    Skipped,
    Blocked,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderSubmissionItem {
    pub order_id: String,
    pub policy_status: String,
    pub disposition: OrderSubmissionDisposition,
    pub persisted: bool,
    pub reasons: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderSubmissionReport {
    pub dry_run: bool,
    pub batch_trace_id: String,
    pub orders_inspected: usize,
    pub eligible: usize,
    pub submitted: usize,
    pub skipped: usize,
    pub blocked: usize,
    pub inconsistent: usize,
    pub duplicates: usize,
    pub items: Vec<OrderSubmissionItem>,
}

pub fn submit_orders(
    query_service: &QueryService<'_>,
    options: OrderSubmissionOptions,
) -> Result<OrderSubmissionReport, MaterializationError> {
    let batch_trace_id = build_batch_trace_id();
    let order_ids = all_local_order_ids(query_service)?;
    let mut items = Vec::new();
    let mut eligible = 0usize;
    let mut submitted = 0usize;
    let mut skipped = 0usize;
    let mut blocked = 0usize;
    let mut inconsistent = 0usize;
    let mut duplicates = 0usize;

    for order_id in order_ids {
        let policy = query_service
            .order_submission_policy(&order_id)?
            .ok_or_else(|| {
                MaterializationError::invalid_state(format!(
                    "order {} is locally visible but has no submission policy",
                    order_id
                ))
            })?;

        let mut reasons = policy.reasons.clone();
        let notes = policy.notes.clone();

        let (disposition, persisted) = match policy.status {
            PromotionPolicyStatus::Eligible
                if policy.next_step == Some(PromotionNextStep::SubmitOrder) =>
            {
                eligible += 1;
                if options.dry_run {
                    (OrderSubmissionDisposition::Eligible, false)
                } else {
                    let appended =
                        persist_order_submission(query_service, &batch_trace_id, &order_id)?;
                    if appended {
                        submitted += 1;
                        (OrderSubmissionDisposition::Submitted, true)
                    } else {
                        duplicates += 1;
                        skipped += 1;
                        reasons.insert(0, "DuplicateOrderSubmission".to_string());
                        (OrderSubmissionDisposition::Skipped, false)
                    }
                }
            }
            PromotionPolicyStatus::Blocked => {
                blocked += 1;
                (OrderSubmissionDisposition::Blocked, false)
            }
            PromotionPolicyStatus::Inconsistent => {
                inconsistent += 1;
                (OrderSubmissionDisposition::Inconsistent, false)
            }
            PromotionPolicyStatus::Weak | PromotionPolicyStatus::Frozen => {
                skipped += 1;
                (OrderSubmissionDisposition::Skipped, false)
            }
            PromotionPolicyStatus::Eligible => {
                skipped += 1;
                reasons.insert(0, "EligibleWithoutSubmitOrderNextStep".to_string());
                (OrderSubmissionDisposition::Skipped, false)
            }
        };

        items.push(OrderSubmissionItem {
            order_id,
            policy_status: format!("{:?}", policy.status),
            disposition,
            persisted,
            reasons,
            notes,
        });
    }

    Ok(OrderSubmissionReport {
        dry_run: options.dry_run,
        batch_trace_id,
        orders_inspected: items.len(),
        eligible,
        submitted,
        skipped,
        blocked,
        inconsistent,
        duplicates,
        items,
    })
}

fn persist_order_submission(
    query_service: &QueryService<'_>,
    batch_trace_id: &str,
    order_id: &str,
) -> Result<bool, MaterializationError> {
    let source_event =
        latest_order_registered_event(query_service, order_id)?.ok_or_else(|| {
            MaterializationError::invalid_state(format!(
                "order {} is eligible but has no order.registered event",
                order_id
            ))
        })?;

    let command = SubmitOrderCommand {
        produced_by: ORDER_SUBMISSION_PRODUCED_BY.to_string(),
        provenance: Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: Some(format!("submit-orders://order/{order_id}")),
            producer_run_id: Some(batch_trace_id.to_string()),
            actor: Some(ORDER_SUBMISSION_ACTOR.to_string()),
            trace_id: Some(order_id.to_string()),
            notes: Some(serde_json::to_string(&json!({
                "order_id": order_id,
                "submission_boundary": "order_submission_boundary_v1",
                "venue": source_event.payload.venue,
            }))?),
        },
        order_id: source_event.payload.order_id.clone(),
        decision_id: source_event
            .payload
            .decision_id
            .clone()
            .or_else(|| source_event.linkage.decision_id.clone()),
        hypothesis_id: source_event.linkage.hypothesis_id.clone(),
        signal_id: source_event.linkage.signal_id.clone(),
        instrument: source_event.payload.instrument.clone(),
        venue: source_event.payload.venue.clone(),
        parent_event_id: Some(source_event.event_id.clone()),
        correlation_id: source_event
            .linkage
            .correlation_id
            .clone()
            .or_else(|| Some(order_id.to_string())),
    };

    let store = query_service.store_ref();
    let envelope = command.execute()?;
    let stored = crate::store::StoredEvent::try_from(envelope)?;
    Ok(store.append_event(&stored)?)
}

fn latest_order_registered_event(
    query_service: &QueryService<'_>,
    order_id: &str,
) -> Result<Option<crate::events::EventEnvelope<OrderRegistered>>, MaterializationError> {
    let events = query_service.all_events()?;
    let mut latest = None;

    for stored in events {
        if let RehydratedEvent::OrderRegistered(event) = RehydratedEvent::try_from(&stored)? {
            if event.payload.order_id == order_id {
                latest = Some(event);
            }
        }
    }

    Ok(latest)
}

fn all_local_order_ids(
    query_service: &QueryService<'_>,
) -> Result<Vec<String>, MaterializationError> {
    let events = query_service.all_events()?;
    let mut order_ids = BTreeSet::new();

    for stored in events {
        match RehydratedEvent::try_from(&stored)? {
            RehydratedEvent::OrderRegistered(event) => {
                order_ids.insert(event.payload.order_id.clone());
            }
            RehydratedEvent::OrderSubmitted(event) => {
                order_ids.insert(event.payload.order_id.clone());
            }
            _ => {}
        }
    }

    Ok(order_ids.into_iter().collect())
}

fn build_batch_trace_id() -> String {
    format!(
        "order-submission-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    )
}
