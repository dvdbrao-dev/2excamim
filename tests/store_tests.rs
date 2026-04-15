use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, EventType, Linkage, MarketScored,
    MarketScoredParameters, Provenance, SignalGenerated, SignalSide, SourceKind,
};
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

fn sample_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("unit-test".into()),
        producer_run_id: Some("run-store".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-store".into()),
        notes: None,
    }
}

fn signal_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: None,
        position_id: None,
        parent_event_id: Some("evt-parent".into()),
        correlation_id: Some("corr-1".into()),
    }
}

fn decision_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: None,
        position_id: None,
        parent_event_id: Some("evt-signal".into()),
        correlation_id: Some("corr-1".into()),
    }
}

fn make_signal_event() -> StoredEvent {
    let envelope = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some("BTCUSDT".into()),
        signal_linkage(),
        sample_provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "BTCUSDT".into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength: 0.81,
            rationale: Some("momentum".into()),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn make_decision_event() -> StoredEvent {
    let envelope = EventEnvelope::new_decision_formed(
        "decision-engine",
        Some("BTCUSDT".into()),
        decision_linkage(),
        sample_provenance(),
        DecisionFormed {
            decision_id: "dec-1".into(),
            instrument: "BTCUSDT".into(),
            action: DecisionAction::Enter,
            side: Some(SignalSide::Long),
            size_hint: Some(1.25),
            rationale: Some("follow signal".into()),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn make_market_scored_event() -> StoredEvent {
    let envelope = EventEnvelope::new_market_scored(
        "scoring-agent-v1",
        Some("market-1".into()),
        Linkage::default(),
        sample_provenance(),
        MarketScored {
            market_id: "market-1".into(),
            scored_on: "2026-04-14".into(),
            score: 0.82,
            market_price: 0.59,
            price_gap_to_half: 0.09,
            volume_usdc: 60_000.0,
            hours_to_resolution: 8.0,
            parameters: MarketScoredParameters {
                price_gap_limit: 0.07,
                min_volume_usdc: 50_000.0,
                min_resolution_hours: 4.0,
                max_resolution_hours: 168.0,
            },
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

#[test]
fn append_and_read_all_roundtrip() {
    let path = temp_store_path("append-read-all");
    let store = JsonlEventStore::new(&path).unwrap();
    let event = make_signal_event();

    let appended = store.append_event(&event).unwrap();
    let events = store.read_all().unwrap();

    assert!(appended);
    assert_eq!(events, vec![event]);

    cleanup(&path);
}

#[test]
fn append_and_read_all_roundtrip_market_scored() {
    let path = temp_store_path("append-read-market-scored");
    let store = JsonlEventStore::new(&path).unwrap();
    let event = make_market_scored_event();

    let appended = store.append_event(&event).unwrap();
    let events = store.read_all().unwrap();

    assert!(appended);
    assert_eq!(events, vec![event]);

    cleanup(&path);
}

#[test]
fn append_multiple_preserves_order() {
    let path = temp_store_path("append-order");
    let store = JsonlEventStore::new(&path).unwrap();
    let first = make_signal_event();
    let second = make_decision_event();

    let appended = store
        .append_events(&[first.clone(), second.clone()])
        .unwrap();
    let events = store.read_all().unwrap();

    assert_eq!(appended, 2);
    assert_eq!(events, vec![first, second]);

    cleanup(&path);
}

#[test]
fn does_not_duplicate_by_idempotency_key() {
    let path = temp_store_path("dedupe");
    let store = JsonlEventStore::new(&path).unwrap();
    let event = make_signal_event();

    let first_append = store.append_event(&event).unwrap();
    let second_append = store.append_event(&event).unwrap();
    let events = store.read_all().unwrap();

    assert!(first_append);
    assert!(!second_append);
    assert_eq!(events.len(), 1);
    assert!(store
        .exists_by_idempotency_key(&event.idempotency_key)
        .unwrap());

    cleanup(&path);
}

#[test]
fn finds_by_event_type() {
    let path = temp_store_path("find-event-type");
    let store = JsonlEventStore::new(&path).unwrap();
    let signal = make_signal_event();
    let decision = make_decision_event();

    store
        .append_events(&[signal.clone(), decision.clone()])
        .unwrap();

    let found = store
        .find_by_event_type(EventType::SignalGenerated)
        .unwrap();

    assert_eq!(found, vec![signal]);

    cleanup(&path);
}

#[test]
fn finds_by_correlation_id() {
    let path = temp_store_path("find-correlation");
    let store = JsonlEventStore::new(&path).unwrap();
    let signal = make_signal_event();
    let decision = make_decision_event();

    store
        .append_events(&[signal.clone(), decision.clone()])
        .unwrap();

    let found = store.find_by_correlation_id("corr-1").unwrap();

    assert_eq!(found, vec![signal, decision]);

    cleanup(&path);
}

#[test]
fn finds_by_signal_id_and_decision_id() {
    let path = temp_store_path("find-linkage");
    let store = JsonlEventStore::new(&path).unwrap();
    let signal = make_signal_event();
    let decision = make_decision_event();

    store
        .append_events(&[signal.clone(), decision.clone()])
        .unwrap();

    let by_signal = store.find_by_signal_id("sig-1").unwrap();
    let by_decision = store.find_by_decision_id("dec-1").unwrap();

    assert_eq!(by_signal, vec![signal.clone(), decision.clone()]);
    assert_eq!(by_decision, vec![signal, decision]);

    cleanup(&path);
}

#[test]
fn converts_event_envelope_to_stored_event() {
    let envelope = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some("BTCUSDT".into()),
        signal_linkage(),
        sample_provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "BTCUSDT".into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength: 0.81,
            rationale: Some("momentum".into()),
        },
    )
    .unwrap();

    let stored = StoredEvent::try_from(&envelope).unwrap();

    assert_eq!(stored.event_id, envelope.event_id);
    assert_eq!(stored.event_type, envelope.event_type);
    assert_eq!(stored.schema_version, envelope.schema_version);
    assert_eq!(stored.idempotency_key, envelope.idempotency_key);
    assert_eq!(stored.linkage, envelope.linkage);
    assert_eq!(stored.provenance, envelope.provenance);
    assert_eq!(stored.payload["signal_id"], "sig-1");
    assert_eq!(stored.payload["instrument"], "BTCUSDT");
}
