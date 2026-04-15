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

fn temp_aux_path(name: &str, suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-runtime-cli-{name}-{nanos}.{suffix}"))
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
