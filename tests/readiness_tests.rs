use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage, Provenance,
    SignalConfirmed, SignalGenerated, SignalSide, SourceKind, VetoRaised, VetoScope,
};
use twoexcamim::queries::{
    DecisionReadinessReason, DecisionReadinessStatus, FillReadinessReason, FillReadinessStatus,
    QueryService, SignalReadinessReason, SignalReadinessStatus,
};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-readiness-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("readiness://tests".into()),
        producer_run_id: Some("run-readiness-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-readiness-1".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str, hypothesis_id: &str, correlation_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn decision_linkage(
    signal_id: Option<&str>,
    decision_id: &str,
    hypothesis_id: Option<&str>,
    correlation_id: &str,
) -> Linkage {
    Linkage {
        hypothesis_id: hypothesis_id.map(str::to_string),
        signal_id: signal_id.map(str::to_string),
        decision_id: Some(decision_id.into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn fill_linkage(
    signal_id: Option<&str>,
    decision_id: Option<&str>,
    order_id: &str,
    hypothesis_id: Option<&str>,
    correlation_id: &str,
) -> Linkage {
    Linkage {
        hypothesis_id: hypothesis_id.map(str::to_string),
        signal_id: signal_id.map(str::to_string),
        decision_id: decision_id.map(str::to_string),
        order_id: Some(order_id.into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn make_signal_generated(signal_id: &str, hypothesis_id: &str, instrument: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, "corr-1"),
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some(hypothesis_id.into()),
                instrument: instrument.into(),
                timeframe: "1h".into(),
                side: SignalSide::Long,
                strength: 0.8,
                rationale: Some("signal".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed(signal_id: &str, hypothesis_id: &str, confirmed_by: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, hypothesis_id, "corr-1"),
            provenance(),
            SignalConfirmed {
                signal_id: signal_id.into(),
                confirmed_by: confirmed_by.into(),
                confirmation_reason: Some("ok".into()),
                confirmation_score: Some(0.92),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_veto(signal_id: &str, hypothesis_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, hypothesis_id, "corr-1"),
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
            decision_linkage(signal_id, decision_id, hypothesis_id, "corr-1"),
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

fn make_decision_veto(decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some("BTCUSDT".into()),
            Linkage {
                decision_id: Some(decision_id.into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            provenance(),
            VetoRaised {
                veto_id: format!("veto-{decision_id}"),
                scope: VetoScope::Decision,
                target_id: decision_id.into(),
                reason_code: "manual_block".into(),
                reason_text: Some("blocked".into()),
                raised_by: "risk-engine".into(),
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
            fill_linkage(
                Some("sig-1"),
                decision_id,
                order_id,
                Some("hyp-1"),
                "corr-1",
            ),
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
fn signal_generated_without_confirmation_or_veto_is_generated_unconfirmed() {
    let (store, path) = store_with_events(
        "signal-generated-unconfirmed",
        vec![make_signal_generated("sig-1", "hyp-1", "BTCUSDT")],
    );
    let service = QueryService::new(&store);

    let readiness = service.signal_readiness("sig-1").unwrap().unwrap();

    assert_eq!(
        readiness.status,
        SignalReadinessStatus::GeneratedUnconfirmed
    );
    assert!(readiness
        .reasons
        .contains(&SignalReadinessReason::SignalGeneratedObserved));
    cleanup(&path);
}

#[test]
fn signal_generated_and_confirmed_without_veto_is_ready_for_decision() {
    let (store, path) = store_with_events(
        "signal-ready",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.signal_readiness("sig-1").unwrap().unwrap();

    assert_eq!(readiness.status, SignalReadinessStatus::ReadyForDecision);
    assert!(readiness.reasons.iter().any(|reason| {
        matches!(
            reason,
            SignalReadinessReason::SignalConfirmedObserved { confirmed_by }
            if confirmed_by == "risk-check"
        )
    }));
    cleanup(&path);
}

#[test]
fn signal_generated_and_vetoed_is_blocked() {
    let (store, path) = store_with_events(
        "signal-blocked",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_veto("sig-1", "hyp-1"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.signal_readiness("sig-1").unwrap().unwrap();

    assert_eq!(readiness.status, SignalReadinessStatus::BlockedByVeto);
    assert!(readiness
        .reasons
        .iter()
        .any(|reason| matches!(reason, SignalReadinessReason::SignalVetoObserved { .. })));
    cleanup(&path);
}

#[test]
fn signal_generated_confirmed_and_vetoed_remains_blocked() {
    let (store, path) = store_with_events(
        "signal-confirmed-vetoed",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_signal_veto("sig-1", "hyp-1"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.signal_readiness("sig-1").unwrap().unwrap();

    assert_eq!(readiness.status, SignalReadinessStatus::BlockedByVeto);
    cleanup(&path);
}

#[test]
fn signal_without_generation_but_with_confirmation_is_inconsistent() {
    let (store, path) = store_with_events(
        "signal-inconsistent",
        vec![make_signal_confirmed("sig-1", "hyp-1", "risk-check")],
    );
    let service = QueryService::new(&store);

    let readiness = service.signal_readiness("sig-1").unwrap().unwrap();

    assert_eq!(readiness.status, SignalReadinessStatus::Inconsistent);
    assert!(readiness
        .reasons
        .contains(&SignalReadinessReason::MissingSignalGenerated));
    cleanup(&path);
}

#[test]
fn decision_with_confirmed_unvetoed_signal_is_ready() {
    let (store, path) = store_with_events(
        "decision-ready",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.decision_readiness("dec-1").unwrap().unwrap();

    assert_eq!(readiness.status, DecisionReadinessStatus::Ready);
    assert!(readiness.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionReadinessReason::UpstreamSignalReady { signal_id } if signal_id == "sig-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn decision_without_confirmed_signal_is_upstream_weak() {
    let (store, path) = store_with_events(
        "decision-weak",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.decision_readiness("dec-1").unwrap().unwrap();

    assert_eq!(
        readiness.status,
        DecisionReadinessStatus::FormedButUpstreamWeak
    );
    assert!(readiness.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionReadinessReason::UpstreamSignalGeneratedUnconfirmed { signal_id }
            if signal_id == "sig-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn decision_is_blocked_when_signal_is_vetoed() {
    let (store, path) = store_with_events(
        "decision-blocked",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_signal_veto("sig-1", "hyp-1"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.decision_readiness("dec-1").unwrap().unwrap();

    assert_eq!(readiness.status, DecisionReadinessStatus::Blocked);
    assert!(readiness.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionReadinessReason::UpstreamSignalBlocked { signal_id } if signal_id == "sig-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn decision_with_direct_veto_is_blocked() {
    let (store, path) = store_with_events(
        "decision-direct-veto",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_decision_veto("dec-1"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.decision_readiness("dec-1").unwrap().unwrap();

    assert_eq!(readiness.status, DecisionReadinessStatus::Blocked);
    assert!(readiness
        .reasons
        .iter()
        .any(|reason| matches!(reason, DecisionReadinessReason::DecisionVetoObserved { .. })));
    cleanup(&path);
}

#[test]
fn decision_with_fill_instrument_mismatch_is_inconsistent() {
    let (store, path) = store_with_events(
        "decision-inconsistent",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "ETHUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.decision_readiness("dec-1").unwrap().unwrap();

    assert_eq!(readiness.status, DecisionReadinessStatus::Inconsistent);
    assert!(readiness.reasons.iter().any(|reason| matches!(
        reason,
        DecisionReadinessReason::FillInstrumentMismatch { .. }
    )));
    cleanup(&path);
}

#[test]
fn fill_with_ready_decision_is_sufficient() {
    let (store, path) = store_with_events(
        "fill-sufficient",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.fill_readiness("fill-1").unwrap().unwrap();

    assert_eq!(
        readiness.status,
        FillReadinessStatus::ReceivedWithSufficientReferences
    );
    assert!(readiness.reasons.iter().any(|reason| {
        matches!(
            reason,
            FillReadinessReason::ReferencedDecisionReady { decision_id } if decision_id == "dec-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn fill_without_decision_reference_is_upstream_insufficient() {
    let (store, path) = store_with_events(
        "fill-insufficient",
        vec![make_fill_received("fill-1", None, "ord-1", "BTCUSDT")],
    );
    let service = QueryService::new(&store);

    let readiness = service.fill_readiness("fill-1").unwrap().unwrap();

    assert_eq!(
        readiness.status,
        FillReadinessStatus::ReceivedButUpstreamInsufficient
    );
    assert!(readiness
        .reasons
        .contains(&FillReadinessReason::MissingDecisionReference));
    cleanup(&path);
}

#[test]
fn fill_with_instrument_mismatch_is_inconsistent() {
    let (store, path) = store_with_events(
        "fill-inconsistent",
        vec![
            make_signal_generated("sig-1", "hyp-1", "BTCUSDT"),
            make_signal_confirmed("sig-1", "hyp-1", "risk-check"),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_fill_received("fill-1", Some("dec-1"), "ord-1", "ETHUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let readiness = service.fill_readiness("fill-1").unwrap().unwrap();

    assert_eq!(readiness.status, FillReadinessStatus::Inconsistent);
    assert!(readiness
        .reasons
        .iter()
        .any(|reason| matches!(reason, FillReadinessReason::FillInstrumentMismatch { .. })));
    cleanup(&path);
}
