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

fn assert_close_json(value: &Value, expected: f64) {
    let actual = value.as_f64().expect("expected numeric JSON value");
    assert!(
        (actual - expected).abs() < 0.00000001,
        "expected {actual} to be close to {expected}"
    );
}

fn temp_aux_path(name: &str, suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-runtime-cli-{name}-{nanos}.{suffix}"))
}

fn temp_dir_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-runtime-cli-{name}-{nanos}"))
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

fn write_snapshot(path: &Path, minutes_after: i64, price: f64) {
    let mut snapshot = market_domain::MarketSnapshot::new(
        "market-1",
        market_domain::MarketSource::Synthetic,
        "Example market",
        market_domain::MarketStatus::Open,
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap()
            + chrono::Duration::minutes(minutes_after),
    )
    .unwrap();
    snapshot.last_price = Some(price);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    serde_json::to_writer(&mut file, &snapshot).unwrap();
    use std::io::Write;
    file.write_all(b"\n").unwrap();
}

fn write_snapshot_for_market(
    path: &Path,
    market_id: &str,
    base_time: chrono::DateTime<Utc>,
    minutes_after: i64,
    price: f64,
) {
    let mut snapshot = market_domain::MarketSnapshot::new(
        market_id,
        market_domain::MarketSource::Synthetic,
        "Example market",
        market_domain::MarketStatus::Open,
        base_time + chrono::Duration::minutes(minutes_after),
    )
    .unwrap();
    snapshot.last_price = Some(price);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    serde_json::to_writer(&mut file, &snapshot).unwrap();
    use std::io::Write;
    file.write_all(b"\n").unwrap();
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
    assert!(stdout.contains("confirm-signals"));
    assert!(stdout.contains("measure-confirmation-outcomes"));
    assert!(stdout.contains("evaluate-confirmation-policy"));
    assert!(stdout.contains("walkforward-confirmation-policy"));
    assert!(stdout.contains("propose-confirmation-policy"));
    assert!(stdout.contains("materialize-confirmation-readiness"));
    assert!(stdout.contains("simulate-paper-fill"));
    assert!(stdout.contains("run-paper-decisions"));
    assert!(stdout.contains("run-paper-pipeline"));
    assert!(stdout.contains("serve-dashboard"));
    assert!(stdout.contains("show-paper-ledger"));
    assert!(stdout.contains("materialize decisions"));
    assert!(stdout.contains("materialize orders"));
    assert!(stdout.contains("submit orders"));
    assert!(stdout.contains("observe fill --fill-id"));
    assert!(stdout.contains("run batch --research-signals"));
}

#[test]
fn cli_measure_confirmation_outcomes_json_reports_favorable_signal() {
    let path = temp_store_path("measure-outcomes-json");
    let snapshots_path = temp_aux_path("measure-outcomes-snapshots", "jsonl");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            {
                let mut event = EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("market-1".into()),
                    signal_linkage(),
                    provenance(),
                    SignalGenerated {
                        signal_id: "sig-1".into(),
                        hypothesis_id: Some("hyp-1".into()),
                        instrument: "market-1".into(),
                        timeframe: "1h".into(),
                        side: SignalSide::Long,
                        strength: 0.8,
                        rationale: Some("runtime smoke".into()),
                    },
                )
                .unwrap();
                event.occurred_at =
                    chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();
                stored_event(event)
            },
            {
                let mut event = EventEnvelope::new_signal_confirmed(
                    "confirmation-agent-v1",
                    Some("market-1".into()),
                    signal_linkage(),
                    provenance(),
                    SignalConfirmed {
                        signal_id: "sig-1".into(),
                        confirmed_by: "confirmation-agent-v1".into(),
                        confirmation_reason: None,
                        confirmation_score: Some(0.8),
                    },
                )
                .unwrap();
                event.occurred_at =
                    chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();
                stored_event(event)
            },
        ])
        .unwrap();
    write_snapshot(&snapshots_path, 0, 0.40);
    write_snapshot(&snapshots_path, 60, 0.46);

    let output = run_cli_raw(&[
        "measure-confirmation-outcomes",
        "--store",
        path.to_str().unwrap(),
        "--snapshots",
        snapshots_path.to_str().unwrap(),
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "measure_confirmation_outcomes");
    assert_eq!(parsed["total_confirmed_signals_processed"], 1);
    assert_eq!(parsed["favorable"], 1);
    assert_eq!(parsed["scorecard"]["favorable_rate"], 1.0);
    assert_eq!(parsed["outcomes"][0]["outcome_label"], "favorable");

    cleanup(&path);
    cleanup(&snapshots_path);
}

#[test]
fn cli_measure_confirmation_outcomes_text_is_compact_and_deterministic() {
    let path = temp_store_path("measure-outcomes-text");
    let snapshots_path = temp_aux_path("measure-outcomes-text-snapshots", "jsonl");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            {
                let mut event = EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("market-1".into()),
                    signal_linkage(),
                    provenance(),
                    SignalGenerated {
                        signal_id: "sig-1".into(),
                        hypothesis_id: Some("hyp-1".into()),
                        instrument: "market-1".into(),
                        timeframe: "1h".into(),
                        side: SignalSide::Long,
                        strength: 0.8,
                        rationale: Some("runtime smoke".into()),
                    },
                )
                .unwrap();
                event.occurred_at =
                    chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();
                stored_event(event)
            },
            {
                let mut event = EventEnvelope::new_signal_confirmed(
                    "confirmation-agent-v1",
                    Some("market-1".into()),
                    signal_linkage(),
                    provenance(),
                    SignalConfirmed {
                        signal_id: "sig-1".into(),
                        confirmed_by: "confirmation-agent-v1".into(),
                        confirmation_reason: None,
                        confirmation_score: Some(0.8),
                    },
                )
                .unwrap();
                event.occurred_at =
                    chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();
                stored_event(event)
            },
        ])
        .unwrap();
    write_snapshot(&snapshots_path, 0, 0.40);
    write_snapshot(&snapshots_path, 60, 0.41);

    let output = run_cli_raw(&[
        "measure-confirmation-outcomes",
        "--store",
        path.to_str().unwrap(),
        "--snapshots",
        snapshots_path.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Confirmation Outcomes"));
    assert!(stdout.contains("total_confirmed_signals_processed: 1"));
    assert!(stdout.contains("neutral: 1"));
    assert!(stdout.contains("neutral_rate: 1.0000"));

    cleanup(&path);
    cleanup(&snapshots_path);
}

#[test]
fn cli_evaluate_confirmation_policy_json_reports_comparison_and_sweep() {
    let path = temp_store_path("evaluate-confirmation-policy");
    let snapshots_path = temp_aux_path("evaluate-confirmation-policy-snapshots", "jsonl");
    let store = JsonlEventStore::new(&path).unwrap();
    let generated_at = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();

    let mut generated = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some("market-1".into()),
        signal_linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "market-1".into(),
            timeframe: "odds_jump".into(),
            side: SignalSide::Long,
            strength: 0.8,
            rationale: Some("runtime smoke".into()),
        },
    )
    .unwrap();
    generated.occurred_at = generated_at;
    let mut confirmed = EventEnvelope::new_signal_confirmed(
        "confirmation-agent-v1",
        Some("market-1".into()),
        signal_linkage(),
        provenance(),
        SignalConfirmed {
            signal_id: "sig-1".into(),
            confirmed_by: "confirmation-agent-v1".into(),
            confirmation_reason: None,
            confirmation_score: Some(0.8),
        },
    )
    .unwrap();
    confirmed.occurred_at = generated_at;
    store
        .append_events(&[stored_event(generated), stored_event(confirmed)])
        .unwrap();
    write_snapshot(&snapshots_path, 0, 0.40);
    write_snapshot(&snapshots_path, 60, 0.46);

    let output = run_cli_raw(&[
        "evaluate-confirmation-policy",
        "--store",
        path.to_str().unwrap(),
        "--snapshots",
        snapshots_path.to_str().unwrap(),
        "--horizons",
        "3600",
        "--confidence-thresholds",
        "0.6,0.8",
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "evaluate_confirmation_policy");
    assert_eq!(parsed["comparison"]["favorable_rate_confirmed"], 1.0);
    assert_eq!(parsed["comparison"]["uplift_favorable_rate"], 0.0);
    assert_eq!(parsed["sweep"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["advisory"][0]["classification"], "review");

    cleanup(&path);
    cleanup(&snapshots_path);
}

#[test]
fn cli_walkforward_confirmation_policy_json_reports_steps_and_summary() {
    let path = temp_store_path("walkforward-confirmation-policy");
    let snapshots_path = temp_aux_path("walkforward-confirmation-policy-snapshots", "jsonl");
    let store = JsonlEventStore::new(&path).unwrap();
    let base_time = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();

    for (signal_id, market_id, strength, minutes_after) in [
        ("sig-1", "market-1", 0.4, 0),
        ("sig-2", "market-2", 0.9, 1),
        ("sig-3", "market-3", 0.4, 120),
        ("sig-4", "market-4", 0.9, 121),
    ] {
        let mut generated = EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(market_id.into()),
            Linkage {
                signal_id: Some(signal_id.into()),
                ..signal_linkage()
            },
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: market_id.into(),
                timeframe: "odds_jump".into(),
                side: SignalSide::Long,
                strength,
                rationale: Some("walkforward".into()),
            },
        )
        .unwrap();
        generated.occurred_at = base_time + chrono::Duration::minutes(minutes_after);
        store.append_event(&stored_event(generated)).unwrap();
    }

    for (market_id, minutes_after, price) in [
        ("market-1", 0, 0.40),
        ("market-1", 60, 0.35),
        ("market-2", 1, 0.40),
        ("market-2", 61, 0.48),
        ("market-3", 120, 0.40),
        ("market-3", 180, 0.35),
        ("market-4", 121, 0.40),
        ("market-4", 181, 0.48),
    ] {
        let mut snapshot = market_domain::MarketSnapshot::new(
            market_id,
            market_domain::MarketSource::Synthetic,
            "Example market",
            market_domain::MarketStatus::Open,
            base_time + chrono::Duration::minutes(minutes_after),
        )
        .unwrap();
        snapshot.last_price = Some(price);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&snapshots_path)
            .unwrap();
        serde_json::to_writer(&mut file, &snapshot).unwrap();
        use std::io::Write;
        file.write_all(b"\n").unwrap();
    }

    let output = run_cli_raw(&[
        "walkforward-confirmation-policy",
        "--store",
        path.to_str().unwrap(),
        "--snapshots",
        snapshots_path.to_str().unwrap(),
        "--eras",
        "2",
        "--horizons",
        "3600",
        "--confidence-thresholds",
        "0.5",
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "walkforward_confirmation_policy");
    assert_eq!(parsed["steps"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["steps"][0]["train_era_id"], "era-001");
    assert_eq!(parsed["steps"][0]["validation_era_id"], "era-002");
    assert_eq!(parsed["steps"][0]["confirmed_sample_count_validation"], 1);
    assert_eq!(parsed["summary"]["average_validation_favorable_rate"], 1.0);
    assert_eq!(parsed["summary"]["average_validation_uplift"], 0.5);

    cleanup(&path);
    cleanup(&snapshots_path);
}

#[test]
fn cli_propose_confirmation_policy_json_exports_reviewable_policy() {
    let path = temp_store_path("propose-confirmation-policy");
    let snapshots_path = temp_aux_path("propose-confirmation-policy-snapshots", "jsonl");
    let output_path = temp_aux_path("proposed-confirmation-policy", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    let base_time = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();

    for (signal_id, market_id, strength, minutes_after) in [
        ("sig-1", "market-1", 0.9, 0),
        ("sig-2", "market-2", 0.9, 1),
        ("sig-3", "market-3", 0.9, 2),
        ("sig-4", "market-4", 0.9, 120),
        ("sig-5", "market-5", 0.9, 121),
        ("sig-6", "market-6", 0.9, 122),
        ("sig-7", "market-7", 0.9, 240),
        ("sig-8", "market-8", 0.9, 241),
        ("sig-9", "market-9", 0.9, 242),
    ] {
        let mut generated = EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(market_id.into()),
            Linkage {
                signal_id: Some(signal_id.into()),
                ..signal_linkage()
            },
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: market_id.into(),
                timeframe: "odds_jump".into(),
                side: SignalSide::Long,
                strength,
                rationale: Some("proposal".into()),
            },
        )
        .unwrap();
        generated.occurred_at = base_time + chrono::Duration::minutes(minutes_after);
        store.append_event(&stored_event(generated)).unwrap();
    }

    for (market_id, minutes_after, price) in [
        ("market-1", 0, 0.40),
        ("market-1", 60, 0.48),
        ("market-2", 1, 0.40),
        ("market-2", 61, 0.49),
        ("market-3", 2, 0.40),
        ("market-3", 62, 0.48),
        ("market-4", 120, 0.40),
        ("market-4", 180, 0.49),
        ("market-5", 121, 0.40),
        ("market-5", 181, 0.48),
        ("market-6", 122, 0.40),
        ("market-6", 182, 0.49),
        ("market-7", 240, 0.40),
        ("market-7", 300, 0.48),
        ("market-8", 241, 0.40),
        ("market-8", 301, 0.49),
        ("market-9", 242, 0.40),
        ("market-9", 302, 0.48),
    ] {
        write_snapshot_for_market(&snapshots_path, market_id, base_time, minutes_after, price);
    }

    let output = run_cli_raw(&[
        "propose-confirmation-policy",
        "--store",
        path.to_str().unwrap(),
        "--snapshots",
        snapshots_path.to_str().unwrap(),
        "--eras",
        "3",
        "--horizons",
        "3600",
        "--confidence-thresholds",
        "0.5",
        "--output",
        output_path.to_str().unwrap(),
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    let exported = twoexcamim::ConfirmationPolicy::from_file(&output_path).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "propose_confirmation_policy");
    assert_eq!(parsed["summary"]["promoted_rules"], 1);
    assert_eq!(parsed["policy"]["rules"][0]["signal_name"], "odds_jump");
    assert_eq!(parsed["policy"]["rules"][0]["status"], "promoted");
    assert_eq!(parsed["policy"]["rules"][0]["confidence_threshold"], 0.5);
    assert_eq!(parsed["policy"]["rules"][0]["horizon_seconds"], 3600);
    assert_eq!(exported.rules[0].signal_name, "odds_jump");
    assert_eq!(exported.rules[0].confidence_threshold, Some(0.5));
    assert_eq!(exported.rules[0].horizon_seconds, Some(3600));

    cleanup(&path);
    cleanup(&snapshots_path);
    cleanup(&output_path);
}

#[test]
fn cli_materialize_confirmation_readiness_json_exports_reloadable_state() {
    let path = temp_store_path("materialize-confirmation-readiness");
    let snapshots_path = temp_aux_path("materialize-confirmation-readiness-snapshots", "jsonl");
    let output_path = temp_aux_path("confirmation-readiness", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    let base_time = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 13, 12, 0, 0).unwrap();

    for (signal_id, market_id, minutes_after) in [
        ("sig-1", "market-1", 0),
        ("sig-2", "market-2", 1),
        ("sig-3", "market-3", 2),
        ("sig-4", "market-4", 120),
        ("sig-5", "market-5", 121),
        ("sig-6", "market-6", 122),
        ("sig-7", "market-7", 240),
        ("sig-8", "market-8", 241),
        ("sig-9", "market-9", 242),
    ] {
        let mut generated = EventEnvelope::new_signal_generated(
            "signal-engine",
            Some(market_id.into()),
            Linkage {
                signal_id: Some(signal_id.into()),
                ..signal_linkage()
            },
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: market_id.into(),
                timeframe: "odds_jump".into(),
                side: SignalSide::Long,
                strength: 0.9,
                rationale: Some("readiness".into()),
            },
        )
        .unwrap();
        generated.occurred_at = base_time + chrono::Duration::minutes(minutes_after);
        store.append_event(&stored_event(generated)).unwrap();
    }

    for (market_id, minutes_after, price) in [
        ("market-1", 0, 0.40),
        ("market-1", 60, 0.48),
        ("market-2", 1, 0.40),
        ("market-2", 61, 0.49),
        ("market-3", 2, 0.40),
        ("market-3", 62, 0.48),
        ("market-4", 120, 0.40),
        ("market-4", 180, 0.49),
        ("market-5", 121, 0.40),
        ("market-5", 181, 0.48),
        ("market-6", 122, 0.40),
        ("market-6", 182, 0.49),
        ("market-7", 240, 0.40),
        ("market-7", 300, 0.48),
        ("market-8", 241, 0.40),
        ("market-8", 301, 0.49),
        ("market-9", 242, 0.40),
        ("market-9", 302, 0.48),
    ] {
        write_snapshot_for_market(&snapshots_path, market_id, base_time, minutes_after, price);
    }

    let output = run_cli_raw(&[
        "materialize-confirmation-readiness",
        "--store",
        path.to_str().unwrap(),
        "--snapshots",
        snapshots_path.to_str().unwrap(),
        "--eras",
        "3",
        "--horizons",
        "3600",
        "--confidence-thresholds",
        "0.5",
        "--output",
        output_path.to_str().unwrap(),
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    let exported = twoexcamim::read_confirmation_readiness_report(&output_path).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "materialize_confirmation_readiness");
    assert_eq!(parsed["output_path"], output_path.display().to_string());
    assert_eq!(parsed["readiness"]["summary"]["promoted"], 1);
    assert_eq!(
        parsed["readiness"]["states"][0]["readiness_status"],
        "promoted"
    );
    assert_eq!(
        parsed["readiness"]["states"][0]["evidence"]["proposed_policy_status"],
        "promoted"
    );
    assert_eq!(exported.summary.promoted, 1);
    assert_eq!(exported.states[0].signal_name, "odds_jump");

    cleanup(&path);
    cleanup(&snapshots_path);
    cleanup(&output_path);
}

#[test]
fn cli_confirm_signals_persists_confirmed_events_and_reports_summary() {
    let path = temp_store_path("confirm-signals");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_event(&stored_event(
            EventEnvelope::new_signal_generated(
                "signal-engine",
                Some("market-1".into()),
                signal_linkage(),
                provenance(),
                SignalGenerated {
                    signal_id: "sig-1".into(),
                    hypothesis_id: Some("hyp-1".into()),
                    instrument: "market-1".into(),
                    timeframe: "1h".into(),
                    side: SignalSide::Long,
                    strength: 0.8,
                    rationale: Some("runtime smoke".into()),
                },
            )
            .unwrap(),
        ))
        .unwrap();

    let output = run_cli(&path, &["confirm-signals"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let events = store.read_all().unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Signal Confirmation"));
    assert!(stdout.contains("total_signals_processed: 1"));
    assert!(stdout.contains("accepted: 1"));
    assert!(stdout.contains("rejected: 0"));
    assert!(stdout.contains("rejected_low_confidence: 0"));
    assert!(stdout.contains("rejected_stale: 0"));
    assert!(stdout.contains("skipped_frozen: 0"));
    assert!(stdout.contains("skipped_already_confirmed: 0"));
    assert!(stdout.contains("policy_overrides_used: 0"));
    assert!(stdout.contains("acceptance_rate: 1.0000"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type.as_str() == "signal.confirmed")
            .count(),
        1
    );

    cleanup(&path);
}

#[test]
fn cli_confirm_signals_json_reports_counts() {
    let path = temp_store_path("confirm-signals-json");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_event(&stored_event(
            EventEnvelope::new_signal_generated(
                "signal-engine",
                Some("market-1".into()),
                signal_linkage(),
                provenance(),
                SignalGenerated {
                    signal_id: "sig-1".into(),
                    hypothesis_id: Some("hyp-1".into()),
                    instrument: "market-1".into(),
                    timeframe: "1h".into(),
                    side: SignalSide::Long,
                    strength: 0.8,
                    rationale: Some("runtime smoke".into()),
                },
            )
            .unwrap(),
        ))
        .unwrap();

    let output = run_cli(&path, &["confirm-signals", "--json"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "confirm_signals");
    assert_eq!(parsed["total_signals_processed"], 1);
    assert_eq!(parsed["accepted"], 1);
    assert_eq!(parsed["rejected"], 0);
    assert_eq!(parsed["rejected_low_confidence"], 0);
    assert_eq!(parsed["rejected_stale"], 0);
    assert_eq!(parsed["skipped_frozen"], 0);
    assert_eq!(parsed["skipped_already_confirmed"], 0);
    assert_eq!(parsed["policy_overrides_used"], 0);
    assert_eq!(parsed["scorecard"]["acceptance_rate"], 1.0);

    cleanup(&path);
}

#[test]
fn cli_confirm_signals_json_is_deterministic_for_same_store_state() {
    let path = temp_store_path("confirm-signals-json-deterministic");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_event(&stored_event(
            EventEnvelope::new_signal_generated(
                "signal-engine",
                Some("market-1".into()),
                signal_linkage(),
                provenance(),
                SignalGenerated {
                    signal_id: "sig-1".into(),
                    hypothesis_id: Some("hyp-1".into()),
                    instrument: "market-1".into(),
                    timeframe: "1h".into(),
                    side: SignalSide::Long,
                    strength: 0.8,
                    rationale: Some("runtime smoke".into()),
                },
            )
            .unwrap(),
        ))
        .unwrap();

    let first = run_cli(&path, &["confirm-signals", "--json"]);
    let second = run_cli(&path, &["confirm-signals", "--json"]);
    let first_json: Value = serde_json::from_slice(&first.stdout).unwrap();
    let second_json: Value = serde_json::from_slice(&second.stdout).unwrap();

    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(first_json["accepted"], 1);
    assert_eq!(second_json["accepted"], 0);
    assert_eq!(second_json["skipped_already_confirmed"], 1);
    assert_eq!(second_json["scorecard"]["acceptance_rate"], 0.0);
    assert_eq!(
        second_json["items"][0]["disposition"],
        "skipped_already_confirmed"
    );
    assert_eq!(
        first_json["items"][0]["signal_id"],
        second_json["items"][0]["signal_id"]
    );

    cleanup(&path);
}

#[test]
fn cli_confirm_signals_with_policy_file_reports_freeze_and_override_counts() {
    let path = temp_store_path("confirm-signals-policy");
    let policy_path = temp_aux_path("confirm-signals-policy", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            stored_event(
                EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("market-1".into()),
                    Linkage {
                        signal_id: Some("sig-frozen".into()),
                        ..signal_linkage()
                    },
                    provenance(),
                    SignalGenerated {
                        signal_id: "sig-frozen".into(),
                        hypothesis_id: Some("hyp-1".into()),
                        instrument: "market-1".into(),
                        timeframe: "odds_jump".into(),
                        side: SignalSide::Short,
                        strength: 0.95,
                        rationale: Some("policy freeze".into()),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("market-2".into()),
                    Linkage {
                        signal_id: Some("sig-promoted".into()),
                        ..signal_linkage()
                    },
                    provenance(),
                    SignalGenerated {
                        signal_id: "sig-promoted".into(),
                        hypothesis_id: Some("hyp-1".into()),
                        instrument: "market-2".into(),
                        timeframe: "activity_spike".into(),
                        side: SignalSide::Long,
                        strength: 0.55,
                        rationale: Some("policy promote".into()),
                    },
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    std::fs::write(
        &policy_path,
        r#"{
  "rules": [
    {
      "signal_name": "odds_jump",
      "direction": "No",
      "status": "frozen"
    },
    {
      "signal_name": "activity_spike",
      "status": "promoted",
      "confidence_threshold": 0.5
    }
  ]
}"#,
    )
    .unwrap();

    let output = run_cli_raw(&[
        "confirm-signals",
        "--store",
        path.to_str().unwrap(),
        "--policy-file",
        policy_path.to_str().unwrap(),
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["accepted"], 1);
    assert_eq!(parsed["skipped_frozen"], 1);
    assert_eq!(parsed["policy_overrides_used"], 1);
    assert_eq!(parsed["items"][0]["disposition"], "skipped_frozen");
    assert_eq!(parsed["items"][1]["applied_confidence_threshold"], 0.5);

    cleanup(&path);
    cleanup(&policy_path);
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
fn cli_simulate_paper_fill_maps_fixture_trade_to_canonical_fill_event() {
    let path = temp_store_path("simulate-paper-fill");
    let trades_path = temp_aux_path("polymarket-paper-trades", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            stored_event(
                EventEnvelope::new_order_registered(
                    "execution-planner",
                    Some("will-bitcoin-hit-100k".into()),
                    order_linkage(),
                    provenance(),
                    OrderRegistered {
                        order_id: "ord-1".into(),
                        decision_id: Some("dec-1".into()),
                        instrument: "will-bitcoin-hit-100k".into(),
                        venue: "polymarket-paper".into(),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_order_submitted(
                    "execution-planner",
                    Some("will-bitcoin-hit-100k".into()),
                    order_linkage(),
                    provenance(),
                    OrderSubmitted {
                        order_id: "ord-1".into(),
                        decision_id: Some("dec-1".into()),
                        instrument: "will-bitcoin-hit-100k".into(),
                        venue: "polymarket-paper".into(),
                    },
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    std::fs::write(
        &trades_path,
        r#"{
  "trades": [
    {
      "id": "trade-1",
      "market": "will-bitcoin-hit-100k",
      "outcome": "yes",
      "side": "Buy",
      "quantity": 200.0,
      "price": 0.5,
      "fee": 0.02,
      "slippage_bps": 12.0,
      "created_at": "2026-04-14T12:00:00Z"
    }
  ]
}"#,
    )
    .unwrap();

    let output = run_cli_raw(&[
        "simulate-paper-fill",
        "--store",
        path.to_str().unwrap(),
        "--order-id",
        "ord-1",
        "--decision-id",
        "dec-1",
        "--market",
        "will-bitcoin-hit-100k",
        "--outcome",
        "yes",
        "--side",
        "buy",
        "--amount-usd",
        "100",
        "--backend-account",
        "paper-main",
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    let events = store.read_all().unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "simulate_paper_fill");
    assert_eq!(parsed["paper_execution"]["disposition"], "Imported");
    assert_eq!(
        parsed["paper_execution"]["fill_result"]["fill_id"],
        "pm-paper-paper-main-trade-1"
    );
    assert_eq!(parsed["fill_observation"]["disposition"], "Observed");
    assert_eq!(parsed["fill_observation"]["persisted"], true);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );
    let second = run_cli_raw(&[
        "simulate-paper-fill",
        "--store",
        path.to_str().unwrap(),
        "--order-id",
        "ord-1",
        "--decision-id",
        "dec-1",
        "--market",
        "will-bitcoin-hit-100k",
        "--outcome",
        "yes",
        "--side",
        "buy",
        "--amount-usd",
        "100",
        "--backend-account",
        "paper-main",
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--json",
    ]);
    let second_stdout = String::from_utf8(second.stdout).unwrap();
    let second_parsed: Value = serde_json::from_str(&second_stdout).unwrap();
    let events_after_second = store.read_all().unwrap();

    assert!(second.status.success());
    assert_eq!(second_parsed["fill_observation"]["duplicate"], true);
    assert_eq!(
        events_after_second
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );

    cleanup(&path);
    cleanup(&trades_path);
}

#[test]
fn cli_run_paper_decisions_fixture_flow_persists_canonical_fill_once() {
    let path = temp_store_path("run-paper-decisions");
    let trades_path = temp_aux_path("paper-decision-trades", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    let linkage = Linkage {
        signal_id: Some("sig-paper".into()),
        correlation_id: Some("corr-paper".into()),
        ..signal_linkage()
    };
    store
        .append_events(&[
            stored_event(
                EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("will-bitcoin-hit-100k".into()),
                    linkage.clone(),
                    provenance(),
                    SignalGenerated {
                        signal_id: "sig-paper".into(),
                        hypothesis_id: Some("hyp-1".into()),
                        instrument: "will-bitcoin-hit-100k".into(),
                        timeframe: "odds_jump".into(),
                        side: SignalSide::Long,
                        strength: 0.9,
                        rationale: Some("paper runner".into()),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_signal_confirmed(
                    "confirmation-agent-v1",
                    Some("will-bitcoin-hit-100k".into()),
                    linkage,
                    provenance(),
                    SignalConfirmed {
                        signal_id: "sig-paper".into(),
                        confirmed_by: "confirmation-agent-v1".into(),
                        confirmation_reason: None,
                        confirmation_score: Some(0.9),
                    },
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    std::fs::write(
        &trades_path,
        r#"{
  "trades": [
    {
      "id": "trade-paper-1",
      "market": "will-bitcoin-hit-100k",
      "outcome": "yes",
      "side": "buy",
      "quantity": 200.0,
      "price": 0.5,
      "fee": 0.02,
      "slippage_bps": 12.0,
      "created_at": "2026-04-14T12:00:00Z"
    }
  ]
}"#,
    )
    .unwrap();

    let output = run_cli_raw(&[
        "run-paper-decisions",
        "--store",
        path.to_str().unwrap(),
        "--backend-account",
        "paper-main",
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--usd-size",
        "100",
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    let events = store.read_all().unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "run_paper_decisions");
    assert_eq!(parsed["report"]["confirmed_signals_seen"], 1);
    assert_eq!(parsed["report"]["execution_requests_sent"], 1);
    assert_eq!(parsed["report"]["fills_persisted"], 1);
    assert_eq!(parsed["report"]["items"][0]["outcome"], "yes");
    assert_eq!(
        parsed["report"]["items"][0]["order_id"],
        "pm-paper-order-yes-sig-paper"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );
    let second = run_cli_raw(&[
        "run-paper-decisions",
        "--store",
        path.to_str().unwrap(),
        "--backend-account",
        "paper-main",
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--usd-size",
        "100",
        "--json",
    ]);
    let second_stdout = String::from_utf8(second.stdout).unwrap();
    let second_parsed: Value = serde_json::from_str(&second_stdout).unwrap();
    let events_after_second = store.read_all().unwrap();

    assert!(second.status.success());
    assert_eq!(second_parsed["report"]["execution_requests_sent"], 0);
    assert_eq!(second_parsed["report"]["skipped_already_executed"], 1);
    assert_eq!(
        events_after_second
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );

    cleanup(&path);
    cleanup(&trades_path);
}

#[test]
fn cli_run_paper_decisions_reports_risk_blocks() {
    let path = temp_store_path("run-paper-decisions-risk");
    let trades_path = temp_aux_path("paper-decision-risk-trades", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    let linkage = Linkage {
        signal_id: Some("sig-risk".into()),
        correlation_id: Some("corr-risk".into()),
        ..signal_linkage()
    };
    store
        .append_events(&[
            stored_event(
                EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("risk-market".into()),
                    linkage.clone(),
                    provenance(),
                    SignalGenerated {
                        signal_id: "sig-risk".into(),
                        hypothesis_id: Some("hyp-1".into()),
                        instrument: "risk-market".into(),
                        timeframe: "odds_jump".into(),
                        side: SignalSide::Long,
                        strength: 0.9,
                        rationale: Some("risk block".into()),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_signal_confirmed(
                    "confirmation-agent-v1",
                    Some("risk-market".into()),
                    linkage,
                    provenance(),
                    SignalConfirmed {
                        signal_id: "sig-risk".into(),
                        confirmed_by: "confirmation-agent-v1".into(),
                        confirmation_reason: None,
                        confirmation_score: Some(0.9),
                    },
                )
                .unwrap(),
            ),
        ])
        .unwrap();
    std::fs::write(
        &trades_path,
        r#"{"trades":[{"id":"trade-risk-1","market":"risk-market","outcome":"yes","side":"buy","quantity":200.0,"price":0.5,"created_at":"2026-04-14T12:00:00Z"}]}"#,
    )
    .unwrap();

    let json_output = run_cli_raw(&[
        "run-paper-decisions",
        "--store",
        path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--max-open-positions",
        "0",
        "--json",
    ]);
    let parsed: Value = serde_json::from_slice(&json_output.stdout).unwrap();

    assert!(json_output.status.success());
    assert_eq!(parsed["report"]["execution_requests_sent"], 0);
    assert_eq!(parsed["report"]["blocked_by_risk"], 1);
    assert_eq!(parsed["report"]["blocked_max_open_positions"], 1);
    assert_eq!(parsed["report"]["items"][0]["disposition"], "BlockedByRisk");

    let text_output = run_cli_raw(&[
        "run-paper-decisions",
        "--store",
        path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--max-open-positions",
        "0",
    ]);
    let stdout = String::from_utf8(text_output.stdout).unwrap();

    assert!(text_output.status.success());
    assert!(stdout.contains("blocked_by_risk: 1"));
    assert!(stdout.contains("blocked_max_open_positions: 1"));

    cleanup(&path);
    cleanup(&trades_path);
}

#[test]
fn cli_run_paper_pipeline_json_reports_ordered_stages_and_persists_fill_once() {
    let dir = temp_dir_path("run-paper-pipeline");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("events.jsonl");
    let trades_path = dir.join("trades.json");
    let store = JsonlEventStore::new(&path).unwrap();
    append_pipeline_signal(
        &store,
        "sig-pipeline",
        "market-pipeline",
        SignalSide::Long,
        0.9,
    );
    write_pipeline_trades(&trades_path, "market-pipeline", "yes");

    let output = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--backend-account",
        "paper-main",
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--usd-size",
        "100",
        "--json",
    ]);
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    let events = store.read_all().unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "run_paper_pipeline");
    assert_eq!(
        parsed["report"]["stages"],
        serde_json::json!([
            "load_existing_signals",
            "confirm_signals",
            "run_paper_decisions",
            "project_paper_ledger"
        ])
    );
    assert_eq!(parsed["report"]["signals_seen"], 1);
    assert_eq!(parsed["report"]["signals_generated"], 0);
    assert_eq!(parsed["report"]["signals_confirmed"], 1);
    assert_eq!(parsed["report"]["execution_requests_sent"], 1);
    assert_eq!(parsed["report"]["fills_persisted"], 1);
    assert_eq!(parsed["report"]["open_positions"], 1);
    assert_eq!(parsed["report"]["total_notional_spent"], 100.0);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );
    let latest_summary = path
        .parent()
        .unwrap()
        .join("operations/latest_summary.json");
    let summary: Value = serde_json::from_slice(&std::fs::read(&latest_summary).unwrap()).unwrap();
    assert_eq!(summary["pipeline"]["fills_persisted"], 1);
    assert_eq!(summary["paper"]["open_positions"], 1);

    let second = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--backend-account",
        "paper-main",
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--usd-size",
        "100",
        "--json",
    ]);
    let second_parsed: Value = serde_json::from_slice(&second.stdout).unwrap();
    let events_after_second = store.read_all().unwrap();

    assert!(second.status.success());
    assert_eq!(second_parsed["report"]["signals_confirmed"], 0);
    assert_eq!(second_parsed["report"]["execution_requests_sent"], 0);
    assert_eq!(second_parsed["report"]["fills_persisted"], 0);
    assert_eq!(second_parsed["report"]["open_positions"], 1);
    assert_eq!(
        events_after_second
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );

    cleanup(&path);
    cleanup(&trades_path);
    cleanup(&latest_summary);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_run_paper_pipeline_propagates_policy_file_freeze() {
    let path = temp_store_path("run-paper-pipeline-policy");
    let policy_path = temp_aux_path("paper-pipeline-policy", "json");
    let trades_path = temp_aux_path("paper-pipeline-policy-trades", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    append_pipeline_signal(&store, "sig-policy", "market-policy", SignalSide::Long, 0.9);
    write_pipeline_trades(&trades_path, "market-policy", "yes");
    std::fs::write(
        &policy_path,
        r#"{"rules":[{"signal_name":"odds_jump","direction":"Yes","status":"frozen"}]}"#,
    )
    .unwrap();

    let output = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--policy-file",
        policy_path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--json",
    ]);
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["report"]["signals_seen"], 1);
    assert_eq!(parsed["report"]["signals_confirmed"], 0);
    assert_eq!(parsed["report"]["execution_requests_sent"], 0);
    assert_eq!(parsed["report"]["fills_persisted"], 0);

    cleanup(&path);
    cleanup(&policy_path);
    cleanup(&trades_path);
}

#[test]
fn cli_run_paper_pipeline_propagates_risk_guard_and_text_report() {
    let path = temp_store_path("run-paper-pipeline-risk");
    let trades_path = temp_aux_path("paper-pipeline-risk-trades", "json");
    let store = JsonlEventStore::new(&path).unwrap();
    append_pipeline_signal(
        &store,
        "sig-risk-pipeline",
        "market-risk-pipeline",
        SignalSide::Long,
        0.9,
    );
    write_pipeline_trades(&trades_path, "market-risk-pipeline", "yes");

    let output = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--max-open-positions",
        "0",
        "--json",
    ]);
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["report"]["signals_confirmed"], 1);
    assert_eq!(parsed["report"]["execution_requests_sent"], 0);
    assert_eq!(parsed["report"]["blocked_by_risk"], 1);
    assert_eq!(parsed["report"]["fills_persisted"], 0);

    let text_output = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--max-open-positions",
        "0",
    ]);
    let stdout = String::from_utf8(text_output.stdout).unwrap();

    assert!(text_output.status.success());
    assert!(stdout.contains("Paper Pipeline"));
    assert!(stdout.contains("signals_seen: 1"));
    assert!(stdout.contains("blocked_by_risk: 1"));
    assert!(stdout.contains("- load_existing_signals"));
    assert!(stdout.contains("- project_paper_ledger"));

    cleanup(&path);
    cleanup(&trades_path);
}

#[test]
fn cli_serve_dashboard_writes_static_control_room() {
    let path = temp_store_path("serve-dashboard");
    let trades_path = temp_aux_path("serve-dashboard-trades", "json");
    let output_path = temp_aux_path("control-room", "html");
    let store = JsonlEventStore::new(&path).unwrap();
    append_pipeline_signal(
        &store,
        "sig-dashboard",
        "market-dashboard",
        SignalSide::Long,
        0.9,
    );
    write_pipeline_trades(&trades_path, "market-dashboard", "yes");

    let pipeline = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(pipeline.status.success());

    let dashboard = run_cli_raw(&[
        "serve-dashboard",
        "--store",
        path.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
        "--json",
    ]);
    let parsed: Value = serde_json::from_slice(&dashboard.stdout).unwrap();
    let html = std::fs::read_to_string(&output_path).unwrap();

    assert!(dashboard.status.success());
    assert_eq!(parsed["kind"], "serve_dashboard");
    assert_eq!(parsed["output_path"], output_path.display().to_string());
    assert!(html.contains("2EXCAMIM Control Room"));
    assert!(html.contains("Pipeline Summary"));
    assert!(html.contains("Paper Trading State"));
    assert!(html.contains("realized_pnl_total"));
    assert!(html.contains("Governance / Readiness"));
    assert!(html.contains("Recent Activity"));
    assert!(html.contains("market-dashboard"));

    cleanup(&path);
    cleanup(&trades_path);
    cleanup(&output_path);
    let latest = path
        .parent()
        .unwrap()
        .join("dashboard/latest_pipeline.json");
    cleanup(&latest);
    let latest_summary = path
        .parent()
        .unwrap()
        .join("operations/latest_summary.json");
    cleanup(&latest_summary);
}

#[test]
fn cli_generate_operational_summary_writes_json_and_markdown() {
    let dir = temp_dir_path("operational-summary");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("events.jsonl");
    let trades_path = dir.join("trades.json");
    let json_output_path = dir.join("latest_summary.json");
    let markdown_output_path = dir.join("latest_summary.md");
    let store = JsonlEventStore::new(&path).unwrap();
    append_pipeline_signal(
        &store,
        "sig-operational-summary",
        "market-operational-summary",
        SignalSide::Long,
        0.9,
    );
    write_pipeline_trades(&trades_path, "market-operational-summary", "yes");

    let pipeline = run_cli_raw(&[
        "run-paper-pipeline",
        "--store",
        path.to_str().unwrap(),
        "--backend-trades-json",
        trades_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(pipeline.status.success());

    let json_summary = run_cli_raw(&[
        "generate-operational-summary",
        "--store",
        path.to_str().unwrap(),
        "--output",
        json_output_path.to_str().unwrap(),
        "--format",
        "json",
        "--json",
    ]);
    let response: Value = serde_json::from_slice(&json_summary.stdout).unwrap();
    let summary: Value =
        serde_json::from_slice(&std::fs::read(&json_output_path).unwrap()).unwrap();

    assert!(json_summary.status.success());
    assert_eq!(response["kind"], "generate_operational_summary");
    assert_eq!(response["format"], "json");
    assert_eq!(summary["pipeline"]["signals_seen"], 1);
    assert_eq!(summary["pipeline"]["signals_confirmed"], 1);
    assert_eq!(summary["pipeline"]["execution_requests_sent"], 1);
    assert_eq!(summary["pipeline"]["fills_persisted"], 1);
    assert_eq!(summary["pipeline"]["blocked_by_risk"], 0);
    assert_eq!(summary["paper"]["open_positions"], 1);
    assert_eq!(summary["paper"]["closed_positions"], 0);
    assert_eq!(summary["governance"]["policy_available"], false);
    assert_eq!(
        summary["recent_activity"]["fills"][0]["instrument"],
        "market-operational-summary"
    );
    assert_eq!(
        summary["recent_activity"]["orders"][0]["instrument"],
        "market-operational-summary"
    );

    let markdown_summary = run_cli_raw(&[
        "generate-operational-summary",
        "--store",
        path.to_str().unwrap(),
        "--output",
        markdown_output_path.to_str().unwrap(),
        "--format",
        "markdown",
    ]);
    let markdown = std::fs::read_to_string(&markdown_output_path).unwrap();

    assert!(markdown_summary.status.success());
    assert!(markdown.contains("# 2EXCAMIM Operational Summary"));
    assert!(markdown.contains("## Pipeline Recap"));
    assert!(markdown.contains("- signals_seen: 1"));
    assert!(markdown.contains("## Recent Activity"));

    let _ = std::fs::remove_dir_all(&dir);
}

fn append_pipeline_signal(
    store: &JsonlEventStore,
    signal_id: &str,
    market_id: &str,
    side: SignalSide,
    strength: f64,
) {
    let linkage = Linkage {
        signal_id: Some(signal_id.into()),
        correlation_id: Some(format!("corr-{signal_id}")),
        ..signal_linkage()
    };
    store
        .append_event(&stored_event(
            EventEnvelope::new_signal_generated(
                "signal-engine",
                Some(market_id.into()),
                linkage,
                provenance(),
                SignalGenerated {
                    signal_id: signal_id.into(),
                    hypothesis_id: Some("hyp-1".into()),
                    instrument: market_id.into(),
                    timeframe: "odds_jump".into(),
                    side,
                    strength,
                    rationale: Some("paper pipeline".into()),
                },
            )
            .unwrap(),
        ))
        .unwrap();
}

fn write_pipeline_trades(path: &Path, market_id: &str, outcome: &str) {
    std::fs::write(
        path,
        format!(
            r#"{{
  "trades": [
    {{
      "id": "trade-{market_id}",
      "market": "{market_id}",
      "outcome": "{outcome}",
      "side": "buy",
      "quantity": 200.0,
      "price": 0.5,
      "fee": 0.02,
      "slippage_bps": 12.0,
      "created_at": "2026-04-14T12:00:00Z"
    }}
  ]
}}"#
        ),
    )
    .unwrap();
}

#[test]
fn cli_show_paper_ledger_json_reports_projection_summary() {
    let path = temp_store_path("show-paper-ledger");
    let store = JsonlEventStore::new(&path).unwrap();
    let linkage = Linkage {
        signal_id: Some("sig-ledger".into()),
        decision_id: Some("dec-ledger".into()),
        order_id: Some("pm-paper-order-sig-ledger".into()),
        correlation_id: Some("corr-ledger".into()),
        ..Linkage::default()
    };
    store
        .append_events(&[
            stored_event(
                EventEnvelope::new_order_registered(
                    "paper-runner",
                    Some("market-ledger".into()),
                    linkage.clone(),
                    provenance(),
                    OrderRegistered {
                        order_id: "pm-paper-order-sig-ledger".into(),
                        decision_id: Some("dec-ledger".into()),
                        instrument: "market-ledger".into(),
                        venue: "polymarket-paper".into(),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_order_submitted(
                    "paper-runner",
                    Some("market-ledger".into()),
                    linkage.clone(),
                    provenance(),
                    OrderSubmitted {
                        order_id: "pm-paper-order-sig-ledger".into(),
                        decision_id: Some("dec-ledger".into()),
                        instrument: "market-ledger".into(),
                        venue: "polymarket-paper".into(),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_fill_received(
                    "paper-runner",
                    Some("market-ledger".into()),
                    linkage.clone(),
                    provenance(),
                    FillReceived {
                        fill_id: "pm-paper-paper-main-trade-ledger-1".into(),
                        decision_id: Some("dec-ledger".into()),
                        order_id: "pm-paper-order-sig-ledger".into(),
                        instrument: "market-ledger".into(),
                        side: FillSide::Buy,
                        quantity: 10.0,
                        price: 0.4,
                        venue: "polymarket-paper".into(),
                        executed_at: chrono::TimeZone::with_ymd_and_hms(
                            &Utc, 2026, 4, 14, 12, 0, 0,
                        )
                        .unwrap(),
                    },
                )
                .unwrap(),
            ),
            stored_event(
                EventEnvelope::new_fill_received(
                    "paper-runner",
                    Some("market-ledger".into()),
                    linkage,
                    provenance(),
                    FillReceived {
                        fill_id: "pm-paper-paper-main-trade-ledger-2".into(),
                        decision_id: Some("dec-ledger".into()),
                        order_id: "pm-paper-order-sig-ledger".into(),
                        instrument: "market-ledger".into(),
                        side: FillSide::Sell,
                        quantity: 4.0,
                        price: 0.6,
                        venue: "polymarket-paper".into(),
                        executed_at: chrono::TimeZone::with_ymd_and_hms(
                            &Utc, 2026, 4, 14, 13, 0, 0,
                        )
                        .unwrap(),
                    },
                )
                .unwrap(),
            ),
        ])
        .unwrap();

    let output = run_cli_raw(&[
        "show-paper-ledger",
        "--store",
        path.to_str().unwrap(),
        "--json",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(parsed["kind"], "show_paper_ledger");
    assert_eq!(parsed["ledger"]["summary"]["total_orders"], 1);
    assert_eq!(parsed["ledger"]["summary"]["total_fills"], 2);
    assert_eq!(parsed["ledger"]["summary"]["open_positions"], 1);
    assert_eq!(parsed["ledger"]["summary"]["closed_positions"], 0);
    assert_eq!(parsed["ledger"]["summary"]["total_notional_spent"], 4.0);
    assert_eq!(parsed["ledger"]["summary"]["total_notional_received"], 2.4);
    assert_close_json(&parsed["ledger"]["summary"]["realized_pnl_total"], 0.8);
    assert_close_json(&parsed["ledger"]["summary"]["unrealized_pnl_total"], 1.2);
    assert_eq!(
        parsed["ledger"]["open_positions"][0]["lifecycle"],
        "PartiallyClosed"
    );
    assert_eq!(parsed["ledger"]["open_positions"][0]["net_shares"], 6.0);
    assert_close_json(&parsed["ledger"]["open_positions"][0]["realized_pnl"], 0.8);
    assert_close_json(
        &parsed["ledger"]["open_positions"][0]["unrealized_pnl"],
        1.2,
    );

    cleanup(&path);
}

#[test]
fn cli_show_paper_ledger_text_is_compact() {
    let path = temp_store_path("show-paper-ledger-text");
    let store = JsonlEventStore::new(&path).unwrap();
    let linkage = Linkage {
        signal_id: Some("sig-ledger-text".into()),
        decision_id: Some("dec-ledger-text".into()),
        order_id: Some("pm-paper-order-sig-ledger-text".into()),
        ..Linkage::default()
    };
    store
        .append_event(&stored_event(
            EventEnvelope::new_fill_received(
                "paper-runner",
                Some("market-ledger-text".into()),
                linkage,
                provenance(),
                FillReceived {
                    fill_id: "pm-paper-paper-main-trade-ledger-text".into(),
                    decision_id: Some("dec-ledger-text".into()),
                    order_id: "pm-paper-order-sig-ledger-text".into(),
                    instrument: "market-ledger-text".into(),
                    side: FillSide::Buy,
                    quantity: 5.0,
                    price: 0.2,
                    venue: "polymarket-paper".into(),
                    executed_at: chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 4, 14, 12, 0, 0)
                        .unwrap(),
                },
            )
            .unwrap(),
        ))
        .unwrap();

    let output = run_cli_raw(&["show-paper-ledger", "--store", path.to_str().unwrap()]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Paper Ledger"));
    assert!(stdout.contains("total_fills: 1"));
    assert!(stdout.contains("open_positions: 1"));
    assert!(stdout.contains("closed_positions: 0"));
    assert!(stdout.contains("total_notional_spent: 1.00000000"));
    assert!(stdout.contains("realized_pnl_total: 0.00000000"));
    assert!(stdout.contains("unrealized_pnl_total: 0"));

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
