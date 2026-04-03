use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use pretty_assertions::assert_eq;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, EventType, FillReceived, FillSide,
    HypothesisGenerated, Linkage, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
    SourceKind,
};
use twoexcamim::observability::{
    build_observability_summary, summary_from_store, ObservabilitySummary,
};
use twoexcamim::scenarios::load_fixture_named;
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("observability://tests".into()),
        producer_run_id: Some("run-obs-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-obs-1".into()),
        notes: None,
    }
}

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

fn hypothesis_linkage(correlation_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: None,
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn signal_linkage(correlation_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn decision_linkage(correlation_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn fill_linkage(correlation_id: &str, order_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: Some(order_id.into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(correlation_id.into()),
    }
}

fn make_hypothesis_generated(correlation_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_hypothesis_generated(
            "research-engine",
            Some("BTCUSDT".into()),
            hypothesis_linkage(correlation_id),
            provenance(),
            HypothesisGenerated {
                hypothesis_id: "hyp-1".into(),
                instrument: "BTCUSDT".into(),
                timeframe: "1h".into(),
                thesis: "breakout".into(),
                direction_hint: Some("long".into()),
                confidence: Some(0.7),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_generated(correlation_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            signal_linkage(correlation_id),
            provenance(),
            SignalGenerated {
                signal_id: "sig-1".into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: "BTCUSDT".into(),
                timeframe: "1h".into(),
                side: SignalSide::Long,
                strength: 0.83,
                rationale: Some("momentum".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed(correlation_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(correlation_id),
            provenance(),
            SignalConfirmed {
                signal_id: "sig-1".into(),
                confirmed_by: "risk-check".into(),
                confirmation_reason: Some("validated".into()),
                confirmation_score: Some(0.91),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_formed(correlation_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some("BTCUSDT".into()),
            decision_linkage(correlation_id),
            provenance(),
            DecisionFormed {
                decision_id: "dec-1".into(),
                instrument: "BTCUSDT".into(),
                action: DecisionAction::Enter,
                side: Some(SignalSide::Long),
                size_hint: Some(1.2),
                rationale: Some("follow signal".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill(correlation_id: &str, order_id: &str, fill_id: &str, quantity: f64) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some("BTCUSDT".into()),
            fill_linkage(correlation_id, order_id),
            provenance(),
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: Some("dec-1".into()),
                order_id: order_id.into(),
                instrument: "BTCUSDT".into(),
                side: FillSide::Buy,
                quantity,
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
fn builds_summary_from_simple_event_vector() {
    let events = vec![
        make_hypothesis_generated("corr-1"),
        make_signal_generated("corr-1"),
        make_signal_confirmed("corr-1"),
        make_decision_formed("corr-1"),
        make_fill("corr-1", "ord-1", "fill-1", 2.5),
    ];

    let summary = build_observability_summary(&events).unwrap();

    assert_eq!(summary.total_events, 5);
    assert_eq!(summary.total_signals, 1);
    assert_eq!(summary.total_decisions, 1);
    assert_eq!(summary.confirmed_signals, 1);
    assert_eq!(summary.vetoed_signals, 0);
    assert_eq!(summary.decisions_with_fills, 1);
    assert_eq!(summary.decisions_without_fills, 0);
    assert_eq!(summary.total_fills, 1);
    assert_eq!(summary.total_filled_quantity, 2.5);
    assert_eq!(summary.unique_correlation_ids, 1);
}

#[test]
fn builds_summary_from_store() {
    let path = temp_store_path("observability-store");
    let store = JsonlEventStore::new(&path).unwrap();
    let events = vec![
        make_signal_generated("corr-store-1"),
        make_decision_formed("corr-store-1"),
        make_fill("corr-store-1", "ord-store-1", "fill-store-1", 1.0),
    ];

    store.append_events(&events).unwrap();

    let summary = summary_from_store(&store).unwrap();

    assert_eq!(summary.total_events, 3);
    assert_eq!(summary.total_signals, 1);
    assert_eq!(summary.total_decisions, 1);
    assert_eq!(summary.total_fills, 1);
    assert_eq!(summary.total_filled_quantity, 1.0);

    cleanup(&path);
}

#[test]
fn counts_fills_and_filled_quantity_correctly() {
    let events = vec![
        make_signal_generated("corr-qty-1"),
        make_decision_formed("corr-qty-1"),
        make_fill("corr-qty-1", "ord-qty-1", "fill-qty-1", 2.0),
        make_fill("corr-qty-1", "ord-qty-1", "fill-qty-2", 1.25),
    ];

    let summary = build_observability_summary(&events).unwrap();

    assert_eq!(summary.total_fills, 2);
    assert_eq!(summary.total_filled_quantity, 3.25);
    assert_eq!(summary.decisions_with_fills, 1);
    assert_eq!(summary.decisions_without_fills, 0);
}

#[test]
fn counts_unique_non_empty_correlation_ids_correctly() {
    let mut empty_correlation_event = make_signal_generated("corr-a");
    empty_correlation_event.linkage.correlation_id = None;

    let events = vec![
        make_hypothesis_generated("corr-a"),
        make_signal_generated("corr-a"),
        make_decision_formed("corr-b"),
        make_fill("corr-b", "ord-corr-1", "fill-corr-1", 1.0),
        empty_correlation_event,
    ];

    let summary = build_observability_summary(&events).unwrap();

    assert_eq!(summary.unique_correlation_ids, 2);
}

#[test]
fn counts_events_by_type_correctly() {
    let events = vec![
        make_hypothesis_generated("corr-map-1"),
        make_signal_generated("corr-map-1"),
        make_signal_confirmed("corr-map-1"),
        make_decision_formed("corr-map-1"),
        make_fill("corr-map-1", "ord-map-1", "fill-map-1", 1.0),
        make_fill("corr-map-1", "ord-map-1", "fill-map-2", 2.0),
    ];

    let summary = build_observability_summary(&events).unwrap();
    let expected = BTreeMap::from([
        (EventType::DecisionFormed.as_str().to_string(), 1usize),
        (EventType::FillReceived.as_str().to_string(), 2usize),
        (EventType::HypothesisGenerated.as_str().to_string(), 1usize),
        (EventType::SignalConfirmed.as_str().to_string(), 1usize),
        (EventType::SignalGenerated.as_str().to_string(), 1usize),
    ]);

    assert_eq!(summary.event_counts_by_type, expected);
}

#[test]
fn builds_complete_summary_from_scenario_fixture() {
    let fixture = load_fixture_named("confirmed_then_filled_signal").unwrap();
    let expected = ObservabilitySummary {
        total_events: 4,
        total_signals: 1,
        total_decisions: 1,
        confirmed_signals: 1,
        vetoed_signals: 0,
        decisions_with_fills: 1,
        decisions_without_fills: 0,
        total_fills: 1,
        total_filled_quantity: 1.5,
        unique_correlation_ids: 1,
        event_counts_by_type: BTreeMap::from([
            (EventType::DecisionFormed.as_str().to_string(), 1usize),
            (EventType::FillReceived.as_str().to_string(), 1usize),
            (EventType::SignalConfirmed.as_str().to_string(), 1usize),
            (EventType::SignalGenerated.as_str().to_string(), 1usize),
        ]),
    };

    let summary = build_observability_summary(&fixture.events).unwrap();

    assert_eq!(summary, expected);
}
