use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, Provenance, SignalConfirmed, SignalGenerated, SignalSide, SourceKind,
    VetoRaised, VetoScope,
};
use twoexcamim::queries::{
    DecisionGovernanceReason, GovernanceRefType, GovernanceStatus, QueryService,
    SignalGovernanceReason,
};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-governance-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("governance://tests".into()),
        producer_run_id: Some("run-governance-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-governance-1".into()),
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

fn order_linkage(order_id: &str, decision_id: Option<&str>) -> Linkage {
    Linkage {
        order_id: Some(order_id.into()),
        decision_id: decision_id.map(str::to_string),
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

fn make_signal_generated(signal_id: &str, hypothesis_id: &str, instrument: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id),
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some(hypothesis_id.into()),
                instrument: instrument.into(),
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

fn make_signal_confirmed(signal_id: &str, hypothesis_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, hypothesis_id),
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
            Linkage {
                signal_id: Some(signal_id.into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
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
    hypothesis_id: Option<&str>,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(instrument.into()),
            decision_linkage(signal_id, decision_id, hypothesis_id),
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

fn make_order_registered(
    order_id: &str,
    decision_id: Option<&str>,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "runtime",
            Some(instrument.into()),
            order_linkage(order_id, decision_id),
            Provenance {
                source_kind: SourceKind::Runtime,
                source_ref: Some("runtime://order".into()),
                producer_run_id: Some("run-order-1".into()),
                actor: Some("runtime".into()),
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

fn store_with_events(name: &str, events: Vec<StoredEvent>) -> (JsonlEventStore, PathBuf) {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    store.append_events(&events).unwrap();
    (store, path)
}

#[test]
fn signal_confirmed_without_veto_is_eligible() {
    let (store, path) = store_with_events(
        "signal-eligible",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.signal_governance("sig-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Eligible);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        SignalGovernanceReason::SignalConfirmedObserved { confirmed_by }
        if confirmed_by == "risk-check"
    )));
    cleanup(&path);
}

#[test]
fn signal_generated_but_unconfirmed_is_weak() {
    let (store, path) = store_with_events(
        "signal-weak",
        vec![make_signal_generated("sig-1", "hyp-1", "BTCUSDT")],
    );
    let service = QueryService::new(&store);

    let report = service.signal_governance("sig-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Weak);
    cleanup(&path);
}

#[test]
fn signal_vetoed_is_blocked() {
    let (store, path) = store_with_events(
        "signal-blocked",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1"),
            make_signal_veto("sig-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.signal_governance("sig-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Blocked);
    assert!(report.blocking_refs.iter().any(|reference| {
        reference.ref_type == GovernanceRefType::Veto && reference.ref_id == "veto-sig-1"
    }));
    cleanup(&path);
}

#[test]
fn signal_without_generation_but_with_downstream_decision_is_inconsistent() {
    let (store, path) = store_with_events(
        "signal-inconsistent",
        vec![make_decision_formed(
            Some("sig-1"),
            "dec-1",
            Some("hyp-1"),
            "BTCUSDT",
        )],
    );
    let service = QueryService::new(&store);

    let report = service.signal_governance("sig-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        SignalGovernanceReason::DownstreamDecisionObservedWithoutHealthySignal { decision_id }
        if decision_id == "dec-1"
    )));
    cleanup(&path);
}

#[test]
fn decision_with_supported_upstream_and_no_execution_is_still_eligible_to_advance() {
    let (store, path) = store_with_events(
        "decision-eligible-pending",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_governance("dec-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Eligible);
    assert!(report
        .reasons
        .contains(&DecisionGovernanceReason::ExecutionBoundaryPending));
    assert!(report.supporting_refs.iter().any(|reference| {
        reference.ref_type == GovernanceRefType::Signal && reference.ref_id == "sig-1"
    }));
    cleanup(&path);
}

#[test]
fn decision_with_supported_upstream_but_weak_execution_boundary_is_weak() {
    let (store, path) = store_with_events(
        "decision-weak-boundary",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_governance("dec-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Weak);
    assert!(report
        .reasons
        .contains(&DecisionGovernanceReason::ExecutionBoundaryWeak));
    cleanup(&path);
}

#[test]
fn decision_with_blocked_upstream_and_observed_fill_is_blocked() {
    let (store, path) = store_with_events(
        "decision-blocked",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1"),
            make_signal_veto("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_governance("dec-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Blocked);
    assert!(report
        .reasons
        .contains(&DecisionGovernanceReason::ExecutionBoundaryBlocked));
    assert!(report.blocking_refs.iter().any(|reference| {
        reference.ref_type == GovernanceRefType::Signal && reference.ref_id == "sig-1"
    }));
    cleanup(&path);
}

#[test]
fn decision_with_conflicting_hypothesis_trace_is_inconsistent() {
    let (store, path) = store_with_events(
        "decision-inconsistent",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-2"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_governance("dec-1").unwrap().unwrap();

    assert_eq!(report.status, GovernanceStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        DecisionGovernanceReason::ConflictingHypothesisReferences { values }
        if values == &vec!["hyp-1".to_string(), "hyp-2".to_string()]
    )));
    cleanup(&path);
}
