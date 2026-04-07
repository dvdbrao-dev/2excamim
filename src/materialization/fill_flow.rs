use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    codecs::RehydratedEvent,
    commands::ObserveFillCommand,
    events::{FillSide, Provenance, SourceKind},
    queries::{OrderLifecycleStatus, QueryService},
};

use super::MaterializationError;

const FILL_OBSERVATION_PRODUCED_BY: &str = "runtime.fill_observation";
const FILL_OBSERVATION_ACTOR: &str = "execution_observation_v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillObservationRequest {
    pub fill_id: String,
    pub order_id: String,
    pub decision_id: Option<String>,
    pub instrument: Option<String>,
    pub side: FillSide,
    pub quantity: f64,
    pub price: f64,
    pub venue: Option<String>,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FillObservationOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillObservationDisposition {
    Eligible,
    Observed,
    Skipped,
    Inconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FillObservationReport {
    pub dry_run: bool,
    pub batch_trace_id: String,
    pub fill_id: String,
    pub order_id: String,
    pub order_status_before: Option<String>,
    pub resolved_decision_id: Option<String>,
    pub resolved_instrument: String,
    pub resolved_venue: String,
    pub disposition: FillObservationDisposition,
    pub persisted: bool,
    pub duplicate: bool,
    pub reasons: Vec<String>,
    pub notes: Vec<String>,
}

pub fn observe_fill(
    query_service: &QueryService<'_>,
    request: &FillObservationRequest,
    options: FillObservationOptions,
) -> Result<FillObservationReport, MaterializationError> {
    let batch_trace_id = build_batch_trace_id();
    let lifecycle = query_service
        .order_lifecycle(&request.order_id)?
        .ok_or_else(|| {
            MaterializationError::invalid_state(format!(
                "order {} is not visible in local lifecycle",
                request.order_id
            ))
        })?;
    let order_context =
        latest_local_order_context(query_service, &request.order_id)?.ok_or_else(|| {
            MaterializationError::invalid_state(format!(
                "order {} has lifecycle but no local order.registered/order.submitted evidence",
                request.order_id
            ))
        })?;

    let mut reasons = lifecycle
        .reasons
        .iter()
        .map(|reason| format!("{reason:?}"))
        .collect::<Vec<_>>();
    let mut notes = lifecycle.notes.clone();

    if let Some(decision_id) = request.decision_id.as_deref() {
        if let Some(local_decision_id) = order_context.decision_id.as_deref() {
            if decision_id != local_decision_id {
                reasons.insert(
                    0,
                    format!(
                        "DecisionIdMismatch(requested={}, local={})",
                        decision_id, local_decision_id
                    ),
                );
                return Ok(build_report(
                    request,
                    options,
                    &batch_trace_id,
                    &order_context,
                    Some(format!("{:?}", lifecycle.status)),
                    FillObservationDisposition::Inconsistent,
                    false,
                    false,
                    reasons,
                    notes,
                ));
            }
        }
    }

    if let Some(instrument) = request.instrument.as_deref() {
        if instrument != order_context.instrument {
            reasons.insert(
                0,
                format!(
                    "InstrumentMismatch(requested={}, local={})",
                    instrument, order_context.instrument
                ),
            );
            return Ok(build_report(
                request,
                options,
                &batch_trace_id,
                &order_context,
                Some(format!("{:?}", lifecycle.status)),
                FillObservationDisposition::Inconsistent,
                false,
                false,
                reasons,
                notes,
            ));
        }
    }

    if let Some(venue) = request.venue.as_deref() {
        if venue != order_context.venue {
            reasons.insert(
                0,
                format!(
                    "VenueMismatch(requested={}, local={})",
                    venue, order_context.venue
                ),
            );
            return Ok(build_report(
                request,
                options,
                &batch_trace_id,
                &order_context,
                Some(format!("{:?}", lifecycle.status)),
                FillObservationDisposition::Inconsistent,
                false,
                false,
                reasons,
                notes,
            ));
        }
    }

    let disposition = match lifecycle.status {
        OrderLifecycleStatus::Registered => {
            reasons.insert(0, "OrderNotSubmittedLocally".to_string());
            notes.push(
                "fill observation boundary waits for local order submission before recording observed execution"
                    .to_string(),
            );
            FillObservationDisposition::Skipped
        }
        OrderLifecycleStatus::Submitted | OrderLifecycleStatus::ObservedWithFills => {
            FillObservationDisposition::Eligible
        }
        OrderLifecycleStatus::Weak | OrderLifecycleStatus::Inconsistent => {
            reasons.insert(0, format!("LifecycleNotEligible({:?})", lifecycle.status));
            FillObservationDisposition::Inconsistent
        }
    };

    if disposition != FillObservationDisposition::Eligible {
        return Ok(build_report(
            request,
            options,
            &batch_trace_id,
            &order_context,
            Some(format!("{:?}", lifecycle.status)),
            disposition,
            false,
            false,
            reasons,
            notes,
        ));
    }

    if options.dry_run {
        return Ok(build_report(
            request,
            options,
            &batch_trace_id,
            &order_context,
            Some(format!("{:?}", lifecycle.status)),
            FillObservationDisposition::Eligible,
            false,
            false,
            reasons,
            notes,
        ));
    }

    let appended =
        persist_fill_observation(query_service, request, &order_context, &batch_trace_id)?;
    let duplicate = !appended;
    if duplicate {
        reasons.insert(0, "DuplicateFillObservation".to_string());
    }

    Ok(build_report(
        request,
        options,
        &batch_trace_id,
        &order_context,
        Some(format!("{:?}", lifecycle.status)),
        if appended {
            FillObservationDisposition::Observed
        } else {
            FillObservationDisposition::Skipped
        },
        appended,
        duplicate,
        reasons,
        notes,
    ))
}

fn persist_fill_observation(
    query_service: &QueryService<'_>,
    request: &FillObservationRequest,
    context: &LocalOrderContext,
    batch_trace_id: &str,
) -> Result<bool, MaterializationError> {
    let command = ObserveFillCommand {
        produced_by: FILL_OBSERVATION_PRODUCED_BY.to_string(),
        provenance: Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: Some(format!("observe-fill://order/{}", request.order_id)),
            producer_run_id: Some(batch_trace_id.to_string()),
            actor: Some(FILL_OBSERVATION_ACTOR.to_string()),
            trace_id: Some(request.fill_id.clone()),
            notes: Some(serde_json::to_string(&serde_json::json!({
                "fill_id": request.fill_id,
                "execution_observation": "execution_observation_v1",
                "order_id": request.order_id,
            }))?),
        },
        fill_id: request.fill_id.clone(),
        decision_id: request
            .decision_id
            .clone()
            .or_else(|| context.decision_id.clone()),
        hypothesis_id: context.hypothesis_id.clone(),
        signal_id: context.signal_id.clone(),
        order_id: request.order_id.clone(),
        instrument: request
            .instrument
            .clone()
            .unwrap_or_else(|| context.instrument.clone()),
        side: request.side,
        quantity: request.quantity,
        price: request.price,
        venue: request
            .venue
            .clone()
            .unwrap_or_else(|| context.venue.clone()),
        executed_at: request.executed_at,
        parent_event_id: Some(context.parent_event_id.clone()),
        correlation_id: context.correlation_id.clone(),
    };

    let store = query_service.store_ref();
    let envelope = command.execute()?;
    let stored = crate::store::StoredEvent::try_from(envelope)?;
    Ok(store.append_event(&stored)?)
}

#[derive(Debug, Clone)]
struct LocalOrderContext {
    decision_id: Option<String>,
    hypothesis_id: Option<String>,
    signal_id: Option<String>,
    instrument: String,
    venue: String,
    parent_event_id: String,
    correlation_id: Option<String>,
}

fn latest_local_order_context(
    query_service: &QueryService<'_>,
    order_id: &str,
) -> Result<Option<LocalOrderContext>, MaterializationError> {
    let events = query_service.all_events()?;
    let mut latest = None;

    for stored in events {
        match RehydratedEvent::try_from(&stored)? {
            RehydratedEvent::OrderSubmitted(event) if event.payload.order_id == order_id => {
                latest = Some(LocalOrderContext {
                    decision_id: event
                        .payload
                        .decision_id
                        .clone()
                        .or_else(|| event.linkage.decision_id.clone()),
                    hypothesis_id: event.linkage.hypothesis_id.clone(),
                    signal_id: event.linkage.signal_id.clone(),
                    instrument: event.payload.instrument.clone(),
                    venue: event.payload.venue.clone(),
                    parent_event_id: event.event_id.clone(),
                    correlation_id: event.linkage.correlation_id.clone(),
                });
            }
            RehydratedEvent::OrderRegistered(event) if event.payload.order_id == order_id => {
                if latest.is_none() {
                    latest = Some(LocalOrderContext {
                        decision_id: event
                            .payload
                            .decision_id
                            .clone()
                            .or_else(|| event.linkage.decision_id.clone()),
                        hypothesis_id: event.linkage.hypothesis_id.clone(),
                        signal_id: event.linkage.signal_id.clone(),
                        instrument: event.payload.instrument.clone(),
                        venue: event.payload.venue.clone(),
                        parent_event_id: event.event_id.clone(),
                        correlation_id: event.linkage.correlation_id.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    Ok(latest)
}

fn build_report(
    request: &FillObservationRequest,
    options: FillObservationOptions,
    batch_trace_id: &str,
    context: &LocalOrderContext,
    order_status_before: Option<String>,
    disposition: FillObservationDisposition,
    persisted: bool,
    duplicate: bool,
    reasons: Vec<String>,
    notes: Vec<String>,
) -> FillObservationReport {
    FillObservationReport {
        dry_run: options.dry_run,
        batch_trace_id: batch_trace_id.to_string(),
        fill_id: request.fill_id.clone(),
        order_id: request.order_id.clone(),
        order_status_before,
        resolved_decision_id: request
            .decision_id
            .clone()
            .or_else(|| context.decision_id.clone()),
        resolved_instrument: request
            .instrument
            .clone()
            .unwrap_or_else(|| context.instrument.clone()),
        resolved_venue: request
            .venue
            .clone()
            .unwrap_or_else(|| context.venue.clone()),
        disposition,
        persisted,
        duplicate,
        reasons,
        notes,
    }
}

fn build_batch_trace_id() -> String {
    format!(
        "fill-observation-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    )
}
