use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use pretty_assertions::assert_eq;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, EventType, FillReceived, FillSide, Linkage,
    Provenance, SignalConfirmed, SignalGenerated, SignalSide, SourceKind, VetoRaised, VetoScope,
};
use twoexcamim::queries::QueryService;
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("query://tests".into()),
        producer_run_id: Some("run-query-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-query-1".into()),
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
    signal_id: &str,
    decision_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        decision_id: Some(decision_id.into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn fill_linkage(
    signal_id: &str,
    decision_id: &str,
    order_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
) -> Linkage {
    Linkage {
        hypothesis_id: Some(hypothesis_id.into()),
        signal_id: Some(signal_id.into()),
        decision_id: Some(decision_id.into()),
        order_id: Some(order_id.into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn make_signal_generated(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
    timeframe: &str,
    side: SignalSide,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, correlation_id),
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some(hypothesis_id.into()),
                instrument: instrument.into(),
                timeframe: timeframe.into(),
                side,
                strength: 0.8,
                rationale: Some("generated in query test".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, correlation_id),
            provenance(),
            SignalConfirmed {
                signal_id: signal_id.into(),
                confirmed_by: "risk-check".into(),
                confirmation_reason: Some("validated".into()),
                confirmation_score: Some(0.91),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_veto(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some(instrument.into()),
            signal_linkage(signal_id, hypothesis_id, correlation_id),
            provenance(),
            VetoRaised {
                veto_id: format!("veto-{signal_id}"),
                scope: VetoScope::Signal,
                target_id: signal_id.into(),
                reason_code: "risk_limit".into(),
                reason_text: Some("rejected".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_formed(
    signal_id: &str,
    decision_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
    side: SignalSide,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(instrument.into()),
            decision_linkage(signal_id, decision_id, hypothesis_id, correlation_id),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: instrument.into(),
                action: DecisionAction::Enter,
                side: Some(side),
                size_hint: Some(1.0),
                rationale: Some("follow signal".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill_received(
    signal_id: &str,
    decision_id: &str,
    order_id: &str,
    fill_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some(instrument.into()),
            fill_linkage(
                signal_id,
                decision_id,
                order_id,
                hypothesis_id,
                correlation_id,
            ),
            provenance(),
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: Some(decision_id.into()),
                order_id: order_id.into(),
                instrument: instrument.into(),
                side: FillSide::Buy,
                quantity: 2.0,
                price: 100.0,
                venue: "binance".into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn seeded_store(test_name: &str) -> (JsonlEventStore, PathBuf) {
    let path = temp_store_path(test_name);
    let store = JsonlEventStore::new(&path).unwrap();
    let events = vec![
        make_signal_generated(
            "sig-1",
            "hyp-1",
            "corr-1",
            "BTCUSDT",
            "1h",
            SignalSide::Long,
        ),
        make_signal_confirmed("sig-1", "hyp-1", "corr-1", "BTCUSDT"),
        make_decision_formed(
            "sig-1",
            "dec-1",
            "hyp-1",
            "corr-1",
            "BTCUSDT",
            SignalSide::Long,
        ),
        make_fill_received(
            "sig-1", "dec-1", "ord-1", "fill-1", "hyp-1", "corr-1", "BTCUSDT",
        ),
        make_signal_generated(
            "sig-2",
            "hyp-2",
            "corr-2",
            "ETHUSDT",
            "4h",
            SignalSide::Short,
        ),
        make_signal_veto("sig-2", "hyp-2", "corr-2", "ETHUSDT"),
        make_decision_formed(
            "sig-2",
            "dec-2",
            "hyp-2",
            "corr-2",
            "ETHUSDT",
            SignalSide::Short,
        ),
    ];

    store.append_events(&events).unwrap();

    (store, path)
}

#[test]
fn gets_signal_projection_by_id() {
    let (store, path) = seeded_store("signal-projection");
    let service = QueryService::new(&store);

    let projection = service.signal_projection("sig-1").unwrap().unwrap();

    assert_eq!(projection.signal_id, "sig-1");
    assert!(projection.generated);
    assert!(projection.confirmed);
    assert!(!projection.vetoed);
    assert_eq!(projection.decision_ids, vec!["dec-1".to_string()]);
    assert_eq!(projection.last_event_type, EventType::DecisionFormed);

    cleanup(&path);
}

#[test]
fn gets_decision_projection_by_id() {
    let (store, path) = seeded_store("decision-projection");
    let service = QueryService::new(&store);

    let projection = service.decision_projection("dec-1").unwrap().unwrap();

    assert_eq!(projection.decision_id, "dec-1");
    assert!(projection.formed);
    assert_eq!(projection.fills_count, 1);
    assert_eq!(projection.filled_quantity, 2.0);
    assert_eq!(projection.average_fill_price, Some(100.0));
    assert_eq!(projection.order_ids, vec!["ord-1".to_string()]);
    assert_eq!(projection.last_event_type, EventType::FillReceived);

    cleanup(&path);
}

#[test]
fn gets_timeline_for_signal() {
    let (store, path) = seeded_store("timeline-signal");
    let service = QueryService::new(&store);

    let timeline = service.timeline_for_signal("sig-1").unwrap();

    assert_eq!(timeline.len(), 4);
    assert_eq!(timeline[0].event_type, EventType::SignalGenerated);
    assert_eq!(timeline[1].event_type, EventType::SignalConfirmed);
    assert_eq!(timeline[2].event_type, EventType::DecisionFormed);
    assert_eq!(timeline[3].event_type, EventType::FillReceived);

    cleanup(&path);
}

#[test]
fn gets_timeline_for_decision() {
    let (store, path) = seeded_store("timeline-decision");
    let service = QueryService::new(&store);

    let timeline = service.timeline_for_decision("dec-1").unwrap();

    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0].event_type, EventType::DecisionFormed);
    assert_eq!(timeline[1].event_type, EventType::FillReceived);

    cleanup(&path);
}

#[test]
fn gets_timeline_for_correlation() {
    let (store, path) = seeded_store("timeline-correlation");
    let service = QueryService::new(&store);

    let timeline = service.timeline_for_correlation("corr-1").unwrap();

    assert_eq!(timeline.len(), 4);
    assert_eq!(timeline[0].event_type, EventType::SignalGenerated);
    assert_eq!(timeline[1].event_type, EventType::SignalConfirmed);
    assert_eq!(timeline[2].event_type, EventType::DecisionFormed);
    assert_eq!(timeline[3].event_type, EventType::FillReceived);

    cleanup(&path);
}

#[test]
fn lists_confirmed_signals() {
    let (store, path) = seeded_store("confirmed-signals");
    let service = QueryService::new(&store);

    let projections = service.confirmed_signals().unwrap();

    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].signal_id, "sig-1");
    assert!(projections[0].confirmed);

    cleanup(&path);
}

#[test]
fn lists_vetoed_signals() {
    let (store, path) = seeded_store("vetoed-signals");
    let service = QueryService::new(&store);

    let projections = service.vetoed_signals().unwrap();

    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].signal_id, "sig-2");
    assert!(projections[0].vetoed);

    cleanup(&path);
}

#[test]
fn lists_decisions_with_fills() {
    let (store, path) = seeded_store("decisions-with-fills");
    let service = QueryService::new(&store);

    let projections = service.decisions_with_fills().unwrap();

    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].decision_id, "dec-1");
    assert!(projections[0].fills_count > 0);

    cleanup(&path);
}

#[test]
fn lists_decisions_without_fills() {
    let (store, path) = seeded_store("decisions-without-fills");
    let service = QueryService::new(&store);

    let projections = service.decisions_without_fills().unwrap();

    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].decision_id, "dec-2");
    assert_eq!(projections[0].fills_count, 0);

    cleanup(&path);
}
