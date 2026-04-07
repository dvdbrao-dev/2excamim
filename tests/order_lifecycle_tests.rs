use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, EventError, FillReceived, FillSide, Linkage,
    OrderRegistered, OrderSubmitted, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
    SourceKind,
};
use twoexcamim::queries::{ExecutionBoundaryReason, ExecutionBoundaryStatus, QueryService};
use twoexcamim::store::{JsonlEventStore, StoreError, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-order-lifecycle-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("order-lifecycle://tests".into()),
        producer_run_id: Some("run-order-lifecycle-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-order-lifecycle-1".into()),
        notes: None,
    }
}

fn decision_linkage(decision_id: &str) -> Linkage {
    Linkage {
        signal_id: Some("sig-1".into()),
        hypothesis_id: Some("hyp-1".into()),
        decision_id: Some(decision_id.into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn order_linkage(order_id: &str, decision_id: Option<&str>) -> Linkage {
    Linkage {
        order_id: Some(order_id.into()),
        decision_id: decision_id.map(str::to_string),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn make_signal_generated() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            Linkage {
                signal_id: Some("sig-1".into()),
                hypothesis_id: Some("hyp-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalGenerated {
                signal_id: "sig-1".into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: "BTCUSDT".into(),
                timeframe: "1h".into(),
                side: SignalSide::Long,
                strength: 0.8,
                rationale: Some("generated".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            Linkage {
                signal_id: Some("sig-1".into()),
                hypothesis_id: Some("hyp-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalConfirmed {
                signal_id: "sig-1".into(),
                confirmed_by: "risk-check".into(),
                confirmation_reason: Some("ok".into()),
                confirmation_score: Some(0.9),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_formed(decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some("BTCUSDT".into()),
            decision_linkage(decision_id),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: "BTCUSDT".into(),
                action: DecisionAction::Enter,
                side: Some(SignalSide::Long),
                size_hint: Some(1.0),
                rationale: Some("follow".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_registered(order_id: &str, decision_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "runtime",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            provenance(),
            OrderRegistered {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: "BTCUSDT".into(),
                venue: "binance".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_submitted(order_id: &str, decision_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_submitted(
            "runtime",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            provenance(),
            OrderSubmitted {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: "BTCUSDT".into(),
                venue: "binance".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill_received(fill_id: &str, decision_id: Option<&str>, order_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some("BTCUSDT".into()),
            Linkage {
                decision_id: decision_id.map(str::to_string),
                order_id: Some(order_id.into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            Provenance {
                source_kind: SourceKind::ExecutionVenue,
                source_ref: Some("binance".into()),
                producer_run_id: Some("run-fill-1".into()),
                actor: Some("venue".into()),
                trace_id: Some("trace-fill-1".into()),
                notes: None,
            },
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: decision_id.map(str::to_string),
                order_id: order_id.into(),
                instrument: "BTCUSDT".into(),
                side: FillSide::Buy,
                quantity: 1.0,
                price: 100.0,
                venue: "binance".into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn order_submitted_valid_event_roundtrips_into_stored_event() {
    let event = EventEnvelope::new_order_submitted(
        "runtime",
        Some("BTCUSDT".into()),
        order_linkage("ord-1", Some("dec-1")),
        provenance(),
        OrderSubmitted {
            order_id: "ord-1".into(),
            decision_id: Some("dec-1".into()),
            instrument: "BTCUSDT".into(),
            venue: "binance".into(),
        },
    )
    .unwrap();

    let stored = StoredEvent::try_from(event).unwrap();

    assert_eq!(stored.event_type.as_str(), "order.submitted");
    assert_eq!(stored.payload["order_id"], "ord-1");
}

#[test]
fn order_submitted_rejects_blank_venue() {
    let err = EventEnvelope::new_order_submitted(
        "runtime",
        Some("BTCUSDT".into()),
        order_linkage("ord-1", Some("dec-1")),
        provenance(),
        OrderSubmitted {
            order_id: "ord-1".into(),
            decision_id: Some("dec-1".into()),
            instrument: "BTCUSDT".into(),
            venue: "".into(),
        },
    )
    .unwrap_err();

    assert!(matches!(err, EventError::ValidationError(message) if message.contains("venue")));
}

#[test]
fn stored_event_rejects_order_submitted_with_contradictory_linkage() {
    let mut stored = make_order_submitted("ord-1", Some("dec-1"));
    stored.linkage.order_id = Some("ord-other".into());

    let err = stored.validate().unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("linkage.order_id must match payload.order_id"))
    );
}

#[test]
fn decision_with_submitted_order_but_no_fill_remains_weak() {
    let path = temp_store_path("decision-submitted-no-fill");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated(),
            make_signal_confirmed(),
            make_decision_formed("dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
            make_order_submitted("ord-1", Some("dec-1")),
        ])
        .unwrap();
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Weak);
    assert_eq!(report.submitted_order_ids, vec!["ord-1".to_string()]);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::LocalOrderSubmitted { order_id, .. } if order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn decision_with_submitted_order_and_fill_is_clear() {
    let path = temp_store_path("decision-submitted-fill");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated(),
            make_signal_confirmed(),
            make_decision_formed("dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
            make_order_submitted("ord-1", Some("dec-1")),
            make_fill_received("fill-1", Some("dec-1"), "ord-1"),
        ])
        .unwrap();
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Clear);
    cleanup(&path);
}

#[test]
fn fill_without_local_submitted_order_is_weak() {
    let path = temp_store_path("fill-without-submitted");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated(),
            make_signal_confirmed(),
            make_decision_formed("dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
            make_fill_received("fill-1", None, "ord-1"),
        ])
        .unwrap();
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Weak);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::MissingLocalOrderSubmission { order_id } if order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn submitted_order_without_registration_is_inconsistent() {
    let path = temp_store_path("submitted-without-registration");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated(),
            make_signal_confirmed(),
            make_decision_formed("dec-1"),
            make_order_submitted("ord-1", Some("dec-1")),
            make_fill_received("fill-1", Some("dec-1"), "ord-1"),
        ])
        .unwrap();
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::SubmittedOrderWithoutRegistration { order_id } if order_id == "ord-1"
    )));
    cleanup(&path);
}
