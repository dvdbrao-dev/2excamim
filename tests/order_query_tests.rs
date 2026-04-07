use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, OrderSubmitted, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
    SourceKind,
};
use twoexcamim::queries::{
    ExecutionBoundaryStatus, OrderLifecycleReason, OrderLifecycleStatus, QueryService,
};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-order-query-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn runtime_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("order-query://runtime".into()),
        producer_run_id: Some("run-order-query-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-order-query-1".into()),
        notes: None,
    }
}

fn venue_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::ExecutionVenue,
        source_ref: Some("binance".into()),
        producer_run_id: Some("run-order-query-fill-1".into()),
        actor: Some("venue".into()),
        trace_id: Some("trace-order-query-fill-1".into()),
        notes: None,
    }
}

fn order_linkage(order_id: &str, decision_id: Option<&str>) -> Linkage {
    Linkage {
        order_id: Some(order_id.into()),
        decision_id: decision_id.map(str::to_string),
        signal_id: Some("sig-1".into()),
        hypothesis_id: Some("hyp-1".into()),
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
            runtime_provenance(),
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
            runtime_provenance(),
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
            Linkage {
                decision_id: Some(decision_id.into()),
                signal_id: Some("sig-1".into()),
                hypothesis_id: Some("hyp-1".into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            runtime_provenance(),
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

fn make_order_registered(order_id: &str, decision_id: Option<&str>, venue: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "runtime",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            runtime_provenance(),
            OrderRegistered {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: "BTCUSDT".into(),
                venue: venue.into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_submitted(order_id: &str, decision_id: Option<&str>, venue: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_submitted(
            "runtime",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            runtime_provenance(),
            OrderSubmitted {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: "BTCUSDT".into(),
                venue: venue.into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill_received(
    fill_id: &str,
    order_id: &str,
    decision_id: Option<&str>,
    venue: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            venue_provenance(),
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: decision_id.map(str::to_string),
                order_id: order_id.into(),
                instrument: "BTCUSDT".into(),
                side: FillSide::Buy,
                quantity: 1.0,
                price: 100.0,
                venue: venue.into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn store_with_events(name: &str, events: Vec<StoredEvent>) -> (JsonlEventStore, PathBuf) {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    store.append_events(&events).unwrap();
    (store, path)
}

#[test]
fn order_registered_only_is_registered() {
    let (store, path) = store_with_events(
        "registered-only",
        vec![make_order_registered("ord-1", Some("dec-1"), "binance")],
    );
    let service = QueryService::new(&store);

    let report = service.order_lifecycle("ord-1").unwrap().unwrap();

    assert_eq!(report.status, OrderLifecycleStatus::Registered);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        OrderLifecycleReason::OrderRegisteredObserved { venue } if venue == "binance"
    )));
    cleanup(&path);
}

#[test]
fn order_registered_and_submitted_is_submitted() {
    let (store, path) = store_with_events(
        "registered-submitted",
        vec![
            make_order_registered("ord-1", Some("dec-1"), "binance"),
            make_order_submitted("ord-1", Some("dec-1"), "binance"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.order_lifecycle("ord-1").unwrap().unwrap();

    assert_eq!(report.status, OrderLifecycleStatus::Submitted);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        OrderLifecycleReason::OrderSubmittedObserved { venue } if venue == "binance"
    )));
    cleanup(&path);
}

#[test]
fn order_with_coherent_fill_is_observed_with_fills() {
    let (store, path) = store_with_events(
        "observed-with-fills",
        vec![
            make_signal_generated(),
            make_signal_confirmed(),
            make_decision_formed("dec-1"),
            make_order_registered("ord-1", Some("dec-1"), "binance"),
            make_order_submitted("ord-1", Some("dec-1"), "binance"),
            make_fill_received("fill-1", "ord-1", Some("dec-1"), "binance"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.order_lifecycle("ord-1").unwrap().unwrap();
    let boundary = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, OrderLifecycleStatus::ObservedWithFills);
    assert_eq!(report.observed_fill_ids, vec!["fill-1".to_string()]);
    assert_eq!(boundary.status, ExecutionBoundaryStatus::Clear);
    cleanup(&path);
}

#[test]
fn order_with_fill_but_without_local_entity_is_weak() {
    let (store, path) = store_with_events(
        "fill-without-local-order",
        vec![make_fill_received(
            "fill-1",
            "ord-1",
            Some("dec-1"),
            "binance",
        )],
    );
    let service = QueryService::new(&store);

    let report = service.order_lifecycle("ord-1").unwrap().unwrap();

    assert_eq!(report.status, OrderLifecycleStatus::Weak);
    assert!(report
        .reasons
        .contains(&OrderLifecycleReason::MissingLocalOrderRegistration));
    cleanup(&path);
}

#[test]
fn submitted_without_registered_is_inconsistent() {
    let (store, path) = store_with_events(
        "submitted-without-registered",
        vec![make_order_submitted("ord-1", Some("dec-1"), "binance")],
    );
    let service = QueryService::new(&store);

    let report = service.order_lifecycle("ord-1").unwrap().unwrap();

    assert_eq!(report.status, OrderLifecycleStatus::Inconsistent);
    assert!(report
        .reasons
        .contains(&OrderLifecycleReason::SubmittedWithoutRegistration));
    cleanup(&path);
}

#[test]
fn conflicting_venues_are_inconsistent() {
    let (store, path) = store_with_events(
        "conflicting-venues",
        vec![
            make_order_registered("ord-1", Some("dec-1"), "binance"),
            make_order_submitted("ord-1", Some("dec-1"), "kraken"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.order_lifecycle("ord-1").unwrap().unwrap();

    assert_eq!(report.status, OrderLifecycleStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        OrderLifecycleReason::ConflictingVenueReferences { values }
        if values == &vec!["binance".to_string(), "kraken".to_string()]
    )));
    cleanup(&path);
}

#[test]
fn order_query_relates_cleanly_to_decision_boundary() {
    let (store, path) = store_with_events(
        "order-query-boundary-relation",
        vec![
            make_signal_generated(),
            make_signal_confirmed(),
            make_decision_formed("dec-1"),
            make_order_registered("ord-1", Some("dec-1"), "binance"),
            make_fill_received("fill-1", "ord-1", Some("dec-1"), "binance"),
        ],
    );
    let service = QueryService::new(&store);

    let order = service.order_lifecycle("ord-1").unwrap().unwrap();
    let boundary = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(order.status, OrderLifecycleStatus::Weak);
    assert_eq!(boundary.status, ExecutionBoundaryStatus::Weak);
    assert!(order
        .notes
        .iter()
        .any(|note| note.contains("execution boundary")));
    cleanup(&path);
}
