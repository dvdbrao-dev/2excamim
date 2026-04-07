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

fn build_store_without_fill(name: &str) -> PathBuf {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    let mut events = build_fixture_events();
    events.pop();
    store.append_events(&events).unwrap();
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

fn run_cli_raw(args: &[&str]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_twoexcamim");
    Command::new(binary).args(args).output().unwrap()
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
    assert!(stdout.contains("Execution Summary"));
    assert!(stdout.contains("ObservedWithFills"));
    assert!(stdout.contains("fully_filled"));
    assert!(stdout.contains("Submission Policy"));
    assert!(stdout.contains("Related Events"));

    cleanup(&path);
}

#[test]
fn cli_order_json_includes_execution_summary() {
    let path = build_store("order-json");
    let output = run_cli(&path, &["inspect", "order", "ord-1", "--json"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "order");
    assert_eq!(
        parsed["execution_summary"]["execution_status"],
        "fully_filled"
    );
    assert_eq!(parsed["execution_summary"]["fill_count"], 1);
    assert_eq!(parsed["execution_summary"]["filled_quantity"], 1.0);

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

#[test]
fn cli_help_returns_zero_and_shows_primary_verbs() {
    let output = run_cli_raw(&["--help"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("2EXCAMIM Runtime CLI"));
    assert!(stdout.contains("inspect <signal|decision|order|fill>"));
    assert!(stdout.contains("policy <signal|decision|order>"));
    assert!(stdout.contains("materialize decisions"));
    assert!(stdout.contains("materialize orders"));
    assert!(stdout.contains("submit orders"));
    assert!(stdout.contains("observe fill --fill-id"));
    assert!(stdout.contains("run batch --research-signals"));
}

#[test]
fn cli_invalid_command_returns_usage_exit_code() {
    let output = run_cli_raw(&["nonsense"]);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("invalid command"));
}

#[test]
fn cli_policy_signal_json_smoke_test() {
    let path = build_store("policy-signal");
    let output = run_cli(&path, &["policy", "signal", "sig-1", "--json"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "signal_policy");
    assert_eq!(parsed["signal_id"], "sig-1");
    assert_eq!(parsed["promotion_policy"]["status"], "Eligible");

    cleanup(&path);
}

#[test]
fn cli_invalid_store_path_returns_runtime_error() {
    let dir_path = std::env::temp_dir();
    let output = run_cli_raw(&["summary", "--store", dir_path.to_str().unwrap()]);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("store io error"));
}

#[test]
fn cli_not_found_json_error_is_structured() {
    let path = build_store("not-found-json");
    let output = run_cli(&path, &["inspect", "signal", "missing", "--json"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: Value = serde_json::from_str(&stderr).unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(parsed["kind"], "error");
    assert_eq!(parsed["exit_code"], 3);
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("signal missing not found"));

    cleanup(&path);
}

#[test]
fn cli_rejects_dry_run_for_inspect() {
    let path = build_store("dry-run-invalid");
    let output = run_cli(&path, &["inspect", "signal", "sig-1", "--dry-run"]);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("--dry-run is only supported"));

    cleanup(&path);
}

#[test]
fn cli_observe_fill_json_smoke_test() {
    let path = build_store_without_fill("observe-fill-json");
    let output = run_cli(
        &path,
        &[
            "observe",
            "fill",
            "--fill-id",
            "fill-cli-1",
            "--order-id",
            "ord-1",
            "--side",
            "buy",
            "--quantity",
            "1.25",
            "--price",
            "0.55",
            "--executed-at",
            "2026-04-07T00:00:00Z",
            "--json",
            "--dry-run",
        ],
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "observe_fill");
    assert_eq!(parsed["order_id"], "ord-1");
    assert_eq!(parsed["fill_id"], "fill-cli-1");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["disposition"], "Eligible");

    cleanup(&path);
}

#[test]
fn cli_observe_fill_rejects_invalid_timestamp() {
    let path = build_store_without_fill("observe-fill-invalid-ts");
    let output = run_cli(
        &path,
        &[
            "observe",
            "fill",
            "--fill-id",
            "fill-cli-2",
            "--order-id",
            "ord-1",
            "--side",
            "buy",
            "--quantity",
            "1.25",
            "--price",
            "0.55",
            "--executed-at",
            "not-a-timestamp",
        ],
    );
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("invalid RFC3339 timestamp"));

    cleanup(&path);
}
