use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, Provenance, SignalConfirmed, SignalGenerated, SignalSide, SourceKind,
    VetoRaised, VetoScope,
};
use twoexcamim::queries::{
    ExecutionBoundaryReason, ExecutionBoundaryRefType, ExecutionBoundaryStatus, QueryService,
};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "twoexcamim-execution-boundary-{name}-{nanos}.jsonl"
    ))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("execution-boundary://tests".into()),
        producer_run_id: Some("run-execution-boundary-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-execution-boundary-1".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str, hypothesis_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn decision_linkage(
    signal_id: Option<&str>,
    decision_id: &str,
    hypothesis_id: Option<&str>,
) -> Linkage {
    Linkage {
        hypothesis_id: hypothesis_id.map(str::to_string),
        signal_id: signal_id.map(str::to_string),
        decision_id: Some(decision_id.into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn fill_linkage(decision_id: Option<&str>, order_id: &str) -> Linkage {
    Linkage {
        decision_id: decision_id.map(str::to_string),
        order_id: Some(order_id.into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn make_signal_generated(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, "hyp-1"),
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
            signal_linkage(signal_id, "hyp-1"),
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

fn make_signal_veto(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, "hyp-1"),
            provenance(),
            VetoRaised {
                veto_id: format!("veto-{signal_id}"),
                scope: VetoScope::Signal,
                target_id: signal_id.into(),
                reason_code: "risk_limit".into(),
                reason_text: Some("blocked".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_formed(
    signal_id: Option<&str>,
    decision_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(instrument.into()),
            decision_linkage(signal_id, decision_id, Some("hyp-1")),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: instrument.into(),
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

fn make_fill_received(
    fill_id: &str,
    decision_id: Option<&str>,
    order_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some(instrument.into()),
            fill_linkage(decision_id, order_id),
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
                instrument: instrument.into(),
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
            Provenance {
                source_kind: SourceKind::Runtime,
                source_ref: Some("runtime://order-register".into()),
                producer_run_id: Some("run-order-1".into()),
                actor: Some("engine".into()),
                trace_id: Some("trace-order-1".into()),
                notes: None,
            },
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

fn store_with_events(name: &str, events: Vec<StoredEvent>) -> (JsonlEventStore, PathBuf) {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    store.append_events(&events).unwrap();
    (store, path)
}

#[test]
fn decision_without_fills_is_weak() {
    let (store, path) = store_with_events(
        "decision-no-fills",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.primary_ref_type, ExecutionBoundaryRefType::Decision);
    assert_eq!(report.status, ExecutionBoundaryStatus::Weak);
    assert!(report
        .reasons
        .contains(&ExecutionBoundaryReason::NoExecutionObservedForDecision));
    cleanup(&path);
}

#[test]
fn decision_with_one_traceable_fill_is_clear() {
    let (store, path) = store_with_events(
        "decision-one-fill",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Clear);
    assert_eq!(report.observed_order_ids, vec!["ord-1".to_string()]);
    assert_eq!(report.observed_fill_ids, vec!["fill-1".to_string()]);
    cleanup(&path);
}

#[test]
fn decision_with_multiple_coherent_fills_remains_clear() {
    let (store, path) = store_with_events(
        "decision-multi-fill-coherent",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
            make_fill_received("fill-2", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Clear);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::MultipleCoherentFillsObserved { count, order_id }
        if *count == 2 && order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn decision_with_conflicting_order_ids_is_inconsistent() {
    let (store, path) = store_with_events(
        "decision-conflicting-orders",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_order_registered("ord-2", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
            make_fill_received("fill-2", Some("dec-1"), "ord-2", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::AmbiguousExternalOrderReferences { order_ids }
        if order_ids == &vec!["ord-1".to_string(), "ord-2".to_string()]
    )));
    cleanup(&path);
}

#[test]
fn blocked_decision_with_observed_fill_is_blocked() {
    let (store, path) = store_with_events(
        "decision-blocked-with-fill",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_signal_veto("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Blocked);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::BlockedDecisionHasObservedFill { decision_id, fill_id }
        if decision_id == "dec-1" && fill_id == "fill-1"
    )));
    cleanup(&path);
}

#[test]
fn fill_with_traceable_decision_is_clear() {
    let (store, path) = store_with_events(
        "fill-traceable",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.primary_ref_type, ExecutionBoundaryRefType::Fill);
    assert_eq!(report.status, ExecutionBoundaryStatus::Clear);
    assert_eq!(report.decision_refs, vec!["dec-1".to_string()]);
    cleanup(&path);
}

#[test]
fn fill_with_only_external_order_reference_is_weak() {
    let (store, path) = store_with_events(
        "fill-external-order-only",
        vec![make_fill_received("fill-1", None, "ord-1", "BTCUSDT")],
    );
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Weak);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::ExternalOrderReferenceOnly { order_id } if order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn decision_with_registered_order_but_no_fill_remains_weak() {
    let (store, path) = store_with_events(
        "decision-registered-order-no-fill",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service
        .decision_execution_boundary("dec-1")
        .unwrap()
        .unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Weak);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::LocalOrderRegistered { order_id, .. } if order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn fill_without_decision_but_with_local_order_can_be_clear() {
    let (store, path) = store_with_events(
        "fill-via-local-order",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", None, "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Clear);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::FillTracesViaLocalOrder { decision_id, order_id }
        if decision_id == "dec-1" && order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn fill_with_decision_but_without_local_order_registration_is_weak() {
    let (store, path) = store_with_events(
        "fill-missing-local-order",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Weak);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::MissingLocalOrderRegistration { order_id } if order_id == "ord-1"
    )));
    cleanup(&path);
}

#[test]
fn fill_with_inconsistent_decision_relationship_is_inconsistent() {
    let (store, path) = store_with_events(
        "fill-instrument-mismatch",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "ETHUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::FillDecisionInstrumentMismatch { fill_id, .. }
        if fill_id == "fill-1"
    )));
    cleanup(&path);
}

#[test]
fn fill_with_ambiguous_incompatible_relations_is_inconsistent() {
    let path = temp_store_path("fill-ambiguous");
    let store = JsonlEventStore::new(&path).unwrap();
    let first = make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT");
    let second = make_fill_received("fill-1", Some("dec-2"), "ord-2", "BTCUSDT");
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        ),
    )
    .unwrap();
    let service = QueryService::new(&store);

    let report = service.fill_execution_boundary("fill-1").unwrap().unwrap();

    assert_eq!(report.status, ExecutionBoundaryStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        ExecutionBoundaryReason::AmbiguousDecisionReferences { decision_ids }
        if decision_ids == &vec!["dec-1".to_string(), "dec-2".to_string()]
    )));
    cleanup(&path);
}
