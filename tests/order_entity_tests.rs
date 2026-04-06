use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, Provenance, SignalConfirmed, SignalGenerated, SignalSide, SourceKind,
};
use twoexcamim::queries::{ExecutionBoundaryStatus, QueryService};
use twoexcamim::store::{JsonlEventStore, StoreError, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-order-entity-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("order-entity://tests".into()),
        producer_run_id: Some("run-order-entity-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-order-entity-1".into()),
        notes: None,
    }
}

fn make_order_registered(
    order_id: &str,
    decision_id: Option<&str>,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "execution-boundary",
            Some(instrument.into()),
            Linkage {
                order_id: Some(order_id.into()),
                decision_id: decision_id.map(str::to_string),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            OrderRegistered {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: instrument.into(),
                venue: "binance".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_generated(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            Linkage {
                signal_id: Some(signal_id.into()),
                hypothesis_id: Some("hyp-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
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

fn make_signal_confirmed(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            Linkage {
                signal_id: Some(signal_id.into()),
                hypothesis_id: Some("hyp-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalConfirmed {
                signal_id: signal_id.into(),
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
            Linkage {
                signal_id: Some("sig-1".into()),
                hypothesis_id: Some("hyp-1".into()),
                decision_id: Some(decision_id.into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
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
fn order_registered_valid_event_roundtrips_into_stored_event() {
    let event = EventEnvelope::new_order_registered(
        "execution-boundary",
        Some("BTCUSDT".into()),
        Linkage {
            order_id: Some("ord-1".into()),
            decision_id: Some("dec-1".into()),
            correlation_id: Some("corr-1".into()),
            ..Linkage::default()
        },
        provenance(),
        OrderRegistered {
            order_id: "ord-1".into(),
            decision_id: Some("dec-1".into()),
            instrument: "BTCUSDT".into(),
            venue: "binance".into(),
        },
    )
    .unwrap();

    let stored = StoredEvent::try_from(event).unwrap();

    assert_eq!(stored.event_type.as_str(), "order.registered");
    assert_eq!(stored.payload["order_id"], "ord-1");
}

#[test]
fn stored_event_rejects_order_registered_with_contradictory_linkage() {
    let mut stored = make_order_registered("ord-1", Some("dec-1"), "BTCUSDT");
    stored.linkage.decision_id = Some("dec-other".into());

    let err = stored.validate().unwrap_err();

    assert!(
        matches!(err, StoreError::InvalidData(message) if message.contains("linkage.decision_id must match payload.decision_id"))
    );
}

#[test]
fn order_registered_improves_execution_boundary_for_decision_and_fill() {
    let path = temp_store_path("order-improves-boundary");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed("dec-1"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", None, "ord-1"),
        ])
        .unwrap();
    let service = QueryService::new(&store);

    let decision_boundary = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();
    let fill_boundary = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(decision_boundary.status, ExecutionBoundaryStatus::Clear);
    assert_eq!(fill_boundary.status, ExecutionBoundaryStatus::Clear);
    cleanup(&path);
}
