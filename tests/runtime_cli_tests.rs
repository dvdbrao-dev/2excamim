use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use serde_json::Value;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, OrderSubmitted, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
    SourceKind,
};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-runtime-cli-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("runtime-cli://tests".into()),
        producer_run_id: Some("run-runtime-cli-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-runtime-cli-1".into()),
        notes: None,
    }
}

fn signal_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
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
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
    }
}

fn order_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: Some("ord-1".into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
    }
}

fn stored_event<T>(event: EventEnvelope<T>) -> StoredEvent
where
    T: serde::Serialize,
{
    StoredEvent::try_from(event).unwrap()
}

fn build_fixture_events() -> Vec<StoredEvent> {
    vec![
        stored_event(
            EventEnvelope::new_signal_generated(
                "signal-engine",
                Some("BTCUSDT".into()),
                signal_linkage(),
                provenance(),
                SignalGenerated {
                    signal_id: "sig-1".into(),
                    hypothesis_id: Some("hyp-1".into()),
                    instrument: "BTCUSDT".into(),
                    timeframe: "1h".into(),
                    side: SignalSide::Long,
                    strength: 0.8,
                    rationale: Some("runtime smoke".into()),
                },
            )
            .unwrap(),
        ),
        stored_event(
            EventEnvelope::new_signal_confirmed(
                "analyst",
                Some("BTCUSDT".into()),
                signal_linkage(),
                provenance(),
                SignalConfirmed {
                    signal_id: "sig-1".into(),
                    confirmed_by: "ops".into(),
                    confirmation_reason: Some("validated".into()),
                    confirmation_score: Some(0.9),
                },
            )
            .unwrap(),
        ),
        stored_event(
            EventEnvelope::new_decision_formed(
                "decision-engine",
                Some("BTCUSDT".into()),
                decision_linkage(),
                provenance(),
                DecisionFormed {
                    decision_id: "dec-1".into(),
                    instrument: "BTCUSDT".into(),
                    action: DecisionAction::Enter,
                    side: Some(SignalSide::Long),
                    size_hint: Some(1.0),
                    rationale: Some("runtime smoke".into()),
                },
            )
            .unwrap(),
        ),
        stored_event(
            EventEnvelope::new_order_registered(
                "execution-planner",
                Some("BTCUSDT".into()),
                order_linkage(),
                provenance(),
                OrderRegistered {
                    order_id: "ord-1".into(),
                    decision_id: Some("dec-1".into()),
                    instrument: "BTCUSDT".into(),
                    venue: "kalshi".into(),
                },
            )
            .unwrap(),
        ),
        stored_event(
            EventEnvelope::new_order_submitted(
                "execution-planner",
                Some("BTCUSDT".into()),
                order_linkage(),
                provenance(),
                OrderSubmitted {
                    order_id: "ord-1".into(),
                    decision_id: Some("dec-1".into()),
                    instrument: "BTCUSDT".into(),
                    venue: "kalshi".into(),
                },
            )
            .unwrap(),
        ),
        stored_event(
            EventEnvelope::new_fill_received(
                "venue-adapter",
                Some("BTCUSDT".into()),
                order_linkage(),
                provenance(),
                FillReceived {
                    fill_id: "fill-1".into(),
                    decision_id: Some("dec-1".into()),
                    order_id: "ord-1".into(),
                    instrument: "BTCUSDT".into(),
                    side: FillSide::Buy,
                    quantity: 1.0,
                    price: 42000.0,
                    venue: "kalshi".into(),
                    executed_at: Utc::now(),
                },
            )
            .unwrap(),
        ),
    ]
}

fn build_store(name: &str) -> PathBuf {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    store.append_events(&build_fixture_events()).unwrap();
    path
}

fn run_cli(path: &Path, args: &[&str]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_twoexcamim");
    Command::new(binary)
        .args(args)
        .arg("--store")
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn cli_summary_smoke_test() {
    let path = build_store("summary");
    let output = run_cli(&path, &[]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Runtime Summary"));
    assert!(stdout.contains("total_events: 6"));
    assert!(stdout.contains("total_fills: 1"));

    cleanup(&path);
}

#[test]
fn cli_signal_json_smoke_test() {
    let path = build_store("signal-json");
    let output = run_cli(&path, &["signal", "sig-1", "--json"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "signal");
    assert_eq!(parsed["signal_id"], "sig-1");
    assert_eq!(parsed["projection"]["confirmed"], true);
    assert_eq!(parsed["governance"]["status"], "Eligible");

    cleanup(&path);
}

#[test]
fn cli_order_smoke_test() {
    let path = build_store("order");
    let output = run_cli(&path, &["order", "ord-1"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Order ord-1"));
    assert!(stdout.contains("Lifecycle"));
    assert!(stdout.contains("ObservedWithFills"));
    assert!(stdout.contains("Related Events"));

    cleanup(&path);
}

#[test]
fn cli_fill_smoke_test() {
    let path = build_store("fill");
    let output = run_cli(&path, &["fill", "fill-1"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Fill fill-1"));
    assert!(stdout.contains("Execution Boundary"));
    assert!(stdout.contains("Matching Fill Events"));

    cleanup(&path);
}
