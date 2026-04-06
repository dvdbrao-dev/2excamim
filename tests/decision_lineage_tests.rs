use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, Provenance, SignalConfirmed, SignalGenerated, SignalSide, SourceKind,
    VetoRaised, VetoScope,
};
use twoexcamim::queries::{DecisionLineageReason, DecisionLineageStatus, QueryService};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-decision-lineage-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("decision-lineage://tests".into()),
        producer_run_id: Some("run-lineage-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-lineage-1".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str, hypothesis_id: Option<&str>) -> Linkage {
    Linkage {
        hypothesis_id: hypothesis_id.map(str::to_string),
        signal_id: Some(signal_id.into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
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
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
    }
}

fn fill_linkage(decision_id: &str, order_id: &str) -> Linkage {
    Linkage {
        decision_id: Some(decision_id.into()),
        order_id: Some(order_id.into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn make_signal_generated(signal_id: &str, hypothesis_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, hypothesis_id),
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: hypothesis_id.map(str::to_string),
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

fn make_signal_confirmed(signal_id: &str, hypothesis_id: Option<&str>) -> StoredEvent {
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

fn make_signal_veto(signal_id: &str, hypothesis_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id, hypothesis_id),
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
                rationale: Some("follow signal".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill_received(
    fill_id: &str,
    decision_id: &str,
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
                decision_id: Some(decision_id.into()),
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

fn make_fill_received_without_decision(
    fill_id: &str,
    order_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some(instrument.into()),
            Linkage {
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
                decision_id: None,
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
            "decision-lineage",
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

fn store_with_events(name: &str, events: Vec<StoredEvent>) -> (JsonlEventStore, PathBuf) {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    store.append_events(&events).unwrap();
    (store, path)
}

#[test]
fn supported_decision_reports_signal_and_hypothesis_lineage() {
    let (store, path) = store_with_events(
        "supported",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_signal_confirmed("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Supported);
    assert_eq!(report.upstream_refs.signal_ids, vec!["sig-1".to_string()]);
    assert_eq!(
        report.upstream_refs.hypothesis_ids,
        vec!["hyp-1".to_string()]
    );
    assert!(report.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::UpstreamSignalSupported { signal_id } if signal_id == "sig-1"
        )
    }));
    assert!(report.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::HypothesisTraced { hypothesis_id } if hypothesis_id == "hyp-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn decision_with_unconfirmed_signal_is_weak() {
    let (store, path) = store_with_events(
        "weak-unconfirmed",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Weak);
    assert!(report.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::UpstreamSignalGeneratedUnconfirmed { signal_id }
            if signal_id == "sig-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn decision_with_vetoed_upstream_is_blocked() {
    let (store, path) = store_with_events(
        "blocked-upstream",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_signal_confirmed("sig-1", Some("hyp-1")),
            make_signal_veto("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Blocked);
    assert_eq!(report.upstream_refs.signal_vetoes.len(), 1);
    assert!(report.reasons.iter().any(|reason| {
        matches!(
            reason,
            DecisionLineageReason::UpstreamSignalBlocked { signal_id } if signal_id == "sig-1"
        )
    }));
    cleanup(&path);
}

#[test]
fn decision_without_upstream_signal_reference_is_weak() {
    let (store, path) = store_with_events(
        "weak-missing-upstream",
        vec![make_decision_formed(None, "dec-1", None, "BTCUSDT")],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Weak);
    assert!(report
        .reasons
        .contains(&DecisionLineageReason::MissingUpstreamSignalReference));
    cleanup(&path);
}

#[test]
fn conflicting_decision_signal_references_are_inconsistent() {
    let path = temp_store_path("inconsistent-conflicting-signal");
    let store = JsonlEventStore::new(&path).unwrap();
    let first = make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT");
    let second = make_decision_formed(Some("sig-2"), "dec-1", Some("hyp-1"), "BTCUSDT");

    let payload_a = serde_json::to_string(&first).unwrap();
    let payload_b = serde_json::to_string(&second).unwrap();
    std::fs::write(&path, format!("{payload_a}\n{payload_b}\n")).unwrap();

    let service = QueryService::new(&store);
    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        DecisionLineageReason::ConflictingSignalReferences { values }
        if values == &vec!["sig-1".to_string(), "sig-2".to_string()]
    )));
    cleanup(&path);
}

#[test]
fn direct_decision_veto_is_reported() {
    let (store, path) = store_with_events(
        "blocked-direct-veto",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_signal_confirmed("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_decision_veto("dec-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Blocked);
    assert_eq!(report.upstream_refs.decision_vetoes.len(), 1);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        DecisionLineageReason::DirectDecisionVetoObserved { .. }
    )));
    cleanup(&path);
}

#[test]
fn downstream_fill_is_included_without_creating_order_lifecycle() {
    let (store, path) = store_with_events(
        "downstream-fill",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_signal_confirmed("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_fill_received("fill-1", "dec-1", "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Supported);
    assert_eq!(report.downstream_refs.fill_ids, vec!["fill-1".to_string()]);
    assert_eq!(report.downstream_refs.order_ids, vec!["ord-1".to_string()]);
    assert!(report.downstream_refs.local_order_ids.is_empty());
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        DecisionLineageReason::DownstreamFillObserved { fill_id, order_id }
        if fill_id == "fill-1" && order_id == "ord-1"
    )));
    assert!(report
        .notes
        .iter()
        .any(|note| note.contains("order lifecycle remains out of scope")));
    cleanup(&path);
}

#[test]
fn fill_instrument_mismatch_makes_lineage_inconsistent() {
    let (store, path) = store_with_events(
        "fill-mismatch",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_signal_confirmed("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_fill_received("fill-1", "dec-1", "ord-1", "ETHUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Inconsistent);
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        DecisionLineageReason::FillInstrumentMismatch { fill_id, fill_instrument }
        if fill_id == "fill-1" && fill_instrument == "ETHUSDT"
    )));
    cleanup(&path);
}

#[test]
fn local_order_registration_enriches_decision_lineage() {
    let (store, path) = store_with_events(
        "local-order-lineage",
        vec![
            make_signal_generated("sig-1", Some("hyp-1")),
            make_signal_confirmed("sig-1", Some("hyp-1")),
            make_decision_formed(Some("sig-1"), "dec-1", Some("hyp-1"), "BTCUSDT"),
            make_order_registered("ord-1", Some("dec-1"), "BTCUSDT"),
            make_fill_received_without_decision("fill-1", "ord-1", "BTCUSDT"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_lineage("dec-1").unwrap().unwrap();

    assert_eq!(report.status, DecisionLineageStatus::Supported);
    assert_eq!(report.downstream_refs.fill_ids, vec!["fill-1".to_string()]);
    assert_eq!(report.downstream_refs.order_ids, vec!["ord-1".to_string()]);
    assert_eq!(
        report.downstream_refs.local_order_ids,
        vec!["ord-1".to_string()]
    );
    assert!(report.reasons.iter().any(|reason| matches!(
        reason,
        DecisionLineageReason::LocalOrderRegistered { order_id, venue }
        if order_id == "ord-1" && venue == "binance"
    )));
    assert!(report
        .notes
        .iter()
        .any(|note| note.contains("local contractual support")));
    cleanup(&path);
}
