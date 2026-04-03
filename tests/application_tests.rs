use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use twoexcamim::application::{ApplicationError, EventAppService};
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, Linkage, Provenance, SignalGenerated,
    SignalSide, SourceKind,
};
use twoexcamim::scenarios::ScenarioError;
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
        source_ref: Some("application://tests".into()),
        producer_run_id: Some("run-app-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-app-1".into()),
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

fn make_signal_generated_envelope(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> EventEnvelope<SignalGenerated> {
    EventEnvelope::new_signal_generated(
        "signal-engine",
        Some(instrument.into()),
        signal_linkage(signal_id, hypothesis_id, correlation_id),
        provenance(),
        SignalGenerated {
            signal_id: signal_id.into(),
            hypothesis_id: Some(hypothesis_id.into()),
            instrument: instrument.into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength: 0.8,
            rationale: Some("service test".into()),
        },
    )
    .unwrap()
}

fn make_signal_generated_stored(
    signal_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(make_signal_generated_envelope(
        signal_id,
        hypothesis_id,
        correlation_id,
        instrument,
    ))
    .unwrap()
}

fn make_decision_formed_stored(
    signal_id: &str,
    decision_id: &str,
    hypothesis_id: &str,
    correlation_id: &str,
    instrument: &str,
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
                side: Some(SignalSide::Long),
                size_hint: Some(1.0),
                rationale: Some("service decision".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn append_envelope_persists_correctly() {
    let path = temp_store_path("application-append-envelope");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);
    let envelope = make_signal_generated_envelope("sig-1", "hyp-1", "corr-1", "BTCUSDT");

    let appended = service.append_envelope(&envelope).unwrap();
    let events = store.read_all().unwrap();

    assert!(appended);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.as_str(), "signal.generated");
    assert_eq!(events[0].linkage.signal_id.as_deref(), Some("sig-1"));

    cleanup(&path);
}

#[test]
fn append_stored_event_deduplicates() {
    let path = temp_store_path("application-dedupe");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);
    let event = make_signal_generated_stored("sig-1", "hyp-1", "corr-1", "BTCUSDT");

    let first = service.append_stored_event(&event).unwrap();
    let second = service.append_stored_event(&event).unwrap();

    assert!(first);
    assert!(!second);
    assert_eq!(store.read_all().unwrap().len(), 1);

    cleanup(&path);
}

#[test]
fn current_summary_works() {
    let path = temp_store_path("application-summary");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);
    let events = vec![
        make_signal_generated_stored("sig-1", "hyp-1", "corr-1", "BTCUSDT"),
        make_decision_formed_stored("sig-1", "dec-1", "hyp-1", "corr-1", "BTCUSDT"),
    ];

    service.append_stored_events(&events).unwrap();

    let summary = service.current_summary().unwrap();

    assert_eq!(summary.total_events, 2);
    assert_eq!(summary.total_signals, 1);
    assert_eq!(summary.total_decisions, 1);
    assert_eq!(summary.decisions_without_fills, 1);

    cleanup(&path);
}

#[test]
fn signal_projection_via_service() {
    let path = temp_store_path("application-signal-projection");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);

    service
        .append_stored_event(&make_signal_generated_stored(
            "sig-1", "hyp-1", "corr-1", "BTCUSDT",
        ))
        .unwrap();

    let projection = service.signal_projection("sig-1").unwrap().unwrap();

    assert_eq!(projection.signal_id, "sig-1");
    assert!(projection.generated);
    assert_eq!(projection.instrument.as_deref(), Some("BTCUSDT"));

    cleanup(&path);
}

#[test]
fn decision_projection_via_service() {
    let path = temp_store_path("application-decision-projection");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);

    service
        .append_stored_events(&[
            make_signal_generated_stored("sig-1", "hyp-1", "corr-1", "BTCUSDT"),
            make_decision_formed_stored("sig-1", "dec-1", "hyp-1", "corr-1", "BTCUSDT"),
        ])
        .unwrap();

    let projection = service.decision_projection("dec-1").unwrap().unwrap();

    assert_eq!(projection.decision_id, "dec-1");
    assert!(projection.formed);
    assert_eq!(projection.instrument.as_deref(), Some("BTCUSDT"));

    cleanup(&path);
}

#[test]
fn replay_fixture_by_name_works() {
    let path = temp_store_path("application-replay");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);

    let result = service
        .replay_fixture_by_name("confirmed_then_filled_signal")
        .unwrap();

    assert_eq!(result.total_events, 4);
    assert_eq!(result.confirmed_signals_count, 1);
    assert_eq!(result.decisions_with_fills_count, 1);

    cleanup(&path);
}

#[test]
fn replay_fixture_by_name_fails_for_unknown_fixture() {
    let path = temp_store_path("application-replay-missing");
    let store = JsonlEventStore::new(&path).unwrap();
    let service = EventAppService::new(&store);

    let error = service
        .replay_fixture_by_name("missing-fixture")
        .unwrap_err();

    assert!(matches!(
        error,
        ApplicationError::Scenario(ScenarioError::UnknownFixture(name)) if name == "missing-fixture"
    ));

    cleanup(&path);
}
