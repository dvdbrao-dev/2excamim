use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use serde_json::{json, Value};
use twoexcamim::{
    events::{
        DecisionAction, EventEnvelope, FillReceived, FillSide, Linkage, Provenance,
        SignalConfirmed, SignalGenerated, SignalSide, SourceKind, VetoRaised, VetoScope,
    },
    runtime,
    store::{JsonlEventStore, StoredEvent},
};

fn temp_path(name: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-handoff-{name}-{nanos}.{extension}"))
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn python_interpreter() -> &'static str {
    "research_prediction_markets/.venv/bin/python"
}

fn run_python_script(script: &str, args: &[String]) -> (i32, String, String) {
    let output = Command::new("python3")
        .arg(repo_path(script))
        .args(args)
        .current_dir(repo_path("."))
        .output()
        .unwrap();

    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn write_research_parquet(path: &Path, rows: &[serde_json::Value]) {
    let payload = serde_json::to_string(rows).unwrap();
    let output = Command::new(python_interpreter())
        .arg("-c")
        .arg(
            r#"
import json
import sys
from pathlib import Path
import pandas as pd

rows = json.loads(sys.argv[2])
df = pd.DataFrame(rows)
if "timestamp" in df.columns:
    df["timestamp"] = pd.to_datetime(df["timestamp"], utc=True, errors="coerce")
Path(sys.argv[1]).parent.mkdir(parents=True, exist_ok=True)
df.to_parquet(sys.argv[1], index=False)
"#,
        )
        .arg(path)
        .arg(payload)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_runtime(args: &[String]) -> (i32, String, String) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = runtime::run(args.to_vec(), &mut stdout, &mut stderr);
    (
        code,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

fn ingest_args(input_path: &Path, store_path: &Path) -> Vec<String> {
    vec![
        "twoexcamim".into(),
        "ingest".into(),
        "research-signals".into(),
        input_path.display().to_string(),
        "--store".into(),
        store_path.display().to_string(),
    ]
}

fn cleanup(paths: &[&Path]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn signal_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("test".into()),
        producer_run_id: Some("run-confirmation".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-confirmation".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str, market_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: None,
        signal_id: Some(signal_id.into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: Some("evt-parent".into()),
        correlation_id: Some(market_id.into()),
    }
}

fn make_signal_event(signal_id: &str, market_id: &str, strength: f64) -> StoredEvent {
    let envelope = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some(market_id.into()),
        signal_linkage(signal_id, market_id),
        signal_provenance(),
        SignalGenerated {
            signal_id: signal_id.into(),
            hypothesis_id: None,
            instrument: market_id.into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength,
            rationale: Some("eligible".into()),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn make_signal_confirmed(signal_id: &str, market_id: &str, confirmation_score: f64) -> StoredEvent {
    let envelope = EventEnvelope::new_signal_confirmed(
        "confirmation-agent-v1",
        Some(market_id.into()),
        signal_linkage(signal_id, market_id),
        signal_provenance(),
        SignalConfirmed {
            signal_id: signal_id.into(),
            confirmed_by: "confirmation-agent-v1".into(),
            confirmation_reason: Some("approved".into()),
            confirmation_score: Some(confirmation_score),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn make_signal_veto(signal_id: &str, market_id: &str) -> StoredEvent {
    let envelope = EventEnvelope::new_veto_raised(
        "veto-agent-v1",
        Some(market_id.into()),
        signal_linkage(signal_id, market_id),
        signal_provenance(),
        VetoRaised {
            veto_id: format!("veto-{signal_id}"),
            scope: VetoScope::Signal,
            target_id: signal_id.into(),
            reason_code: "risk".into(),
            reason_text: Some("blocked before sizing".into()),
            raised_by: "veto-agent-v1".into(),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn make_decision_formed(decision_id: &str, signal_id: &str, market_id: &str) -> StoredEvent {
    let envelope = EventEnvelope::new_decision_formed(
        "sizing-agent-v1",
        Some(market_id.into()),
        Linkage {
            hypothesis_id: None,
            signal_id: Some(signal_id.into()),
            decision_id: Some(decision_id.into()),
            order_id: None,
            position_id: None,
            parent_event_id: Some("evt-parent".into()),
            correlation_id: Some(market_id.into()),
        },
        Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: Some("test".into()),
            producer_run_id: Some("run-exit".into()),
            actor: Some("tests".into()),
            trace_id: Some("trace-exit".into()),
            notes: None,
        },
        twoexcamim::events::DecisionFormed {
            decision_id: decision_id.into(),
            instrument: market_id.into(),
            action: DecisionAction::Enter,
            side: Some(SignalSide::Long),
            size_hint: Some(100.0),
            rationale: Some("gap_expected=0.10".into()),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn make_fill_received(
    decision_id: &str,
    order_id: &str,
    instrument: &str,
    fill_id: &str,
    side: FillSide,
    quantity: f64,
    price: f64,
    executed_at: &str,
) -> StoredEvent {
    let envelope = EventEnvelope::new_fill_received(
        "paper-execution",
        Some(instrument.into()),
        Linkage {
            hypothesis_id: None,
            signal_id: Some("signal-1".into()),
            decision_id: Some(decision_id.into()),
            order_id: Some(order_id.into()),
            position_id: None,
            parent_event_id: Some("evt-parent".into()),
            correlation_id: Some(instrument.into()),
        },
        Provenance {
            source_kind: SourceKind::ExecutionVenue,
            source_ref: Some("test-venue".into()),
            producer_run_id: Some("run-exit".into()),
            actor: Some("tests".into()),
            trace_id: Some("trace-exit".into()),
            notes: None,
        },
        FillReceived {
            fill_id: fill_id.into(),
            decision_id: Some(decision_id.into()),
            order_id: order_id.into(),
            instrument: instrument.into(),
            side,
            quantity,
            price,
            venue: "paper".into(),
            executed_at: chrono::DateTime::parse_from_rfc3339(executed_at)
                .unwrap()
                .with_timezone(&Utc),
        },
    )
    .unwrap();

    StoredEvent::try_from(envelope).unwrap()
}

fn write_jsonl(path: &Path, records: &[Value]) {
    let mut contents = String::new();
    for record in records {
        contents.push_str(&serde_json::to_string(record).unwrap());
        contents.push('\n');
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn valid_row() -> Value {
    json!({
        "market_id": "market-1",
        "timestamp": "2026-04-03T17:57:41.047Z",
        "signal_name": "vwap_reversion",
        "strength": 1.0,
        "direction": "long_yes",
        "probability": 0.25,
        "spread_tight": 0.99,
        "volume_spike_24h": 1.0,
        "price_deviation_vwap_1h": -12.5,
        "source": "polymarket",
        "metadata": "{\"source\":\"polymarket\"}"
    })
}

#[test]
fn ingests_valid_research_parquet_into_signal_generated_events() {
    let input_path = temp_path("valid", "parquet");
    let store_path = temp_path("valid-store", "jsonl");

    write_research_parquet(&input_path, &[valid_row()]);

    let (code, stdout, stderr) = run_runtime(&ingest_args(&input_path, &store_path));

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Research Signal Ingest"));
    assert!(stdout.contains("rows_valid: 1"));
    assert!(stdout.contains("events_written: 1"));

    let store = JsonlEventStore::new(&store_path).unwrap();
    let events = store.read_all().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.as_str(), "signal.generated");
    assert_eq!(
        events[0].aggregate_key.as_deref(),
        Some("polymarket:market-1")
    );
    assert_eq!(
        events[0].linkage.signal_id,
        events[0].linkage.correlation_id
    );
    assert_eq!(
        events[0].provenance.actor.as_deref(),
        Some("research_prediction_markets")
    );
    assert!(events[0].provenance.producer_run_id.is_some());
    assert!(events[0]
        .provenance
        .notes
        .as_deref()
        .unwrap()
        .contains("research-signals.v1"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn dry_run_validates_and_translates_without_persisting() {
    let input_path = temp_path("dry-run", "parquet");
    let store_path = temp_path("dry-run-store", "jsonl");

    write_research_parquet(&input_path, &[valid_row()]);

    let mut args = ingest_args(&input_path, &store_path);
    args.push("--dry-run".into());
    args.push("--json".into());

    let (code, stdout, stderr) = run_runtime(&args);
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["rows_valid"], 1);
    assert_eq!(parsed["events_written"], 0);
    assert_eq!(parsed["duplicates"], 0);

    let store = JsonlEventStore::new(&store_path).unwrap();
    assert!(store.read_all().unwrap().is_empty());

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn rejects_invalid_records_but_keeps_valid_ones() {
    let input_path = temp_path("invalid-row", "parquet");
    let store_path = temp_path("invalid-row-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[
            valid_row(),
            json!({
                "market_id": "market-2",
                "timestamp": "2026-04-03T17:57:41.047Z",
                "signal_name": "vwap_reversion",
                "strength": 0.8,
                "direction": "sideways",
                "probability": 0.25,
                "spread_tight": 0.99,
                "volume_spike_24h": 1.0,
                "price_deviation_vwap_1h": -12.5,
                "source": "polymarket",
                "metadata": "{\"source\":\"polymarket\"}"
            }),
        ],
    );

    let (code, stdout, stderr) = run_runtime(&ingest_args(&input_path, &store_path));

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("rows_valid: 1"));
    assert!(stdout.contains("rows_invalid: 1"));

    let store = JsonlEventStore::new(&store_path).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 1);

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn deduplicates_reingest_of_same_research_file() {
    let input_path = temp_path("dedupe", "parquet");
    let store_path = temp_path("dedupe-store", "jsonl");

    write_research_parquet(&input_path, &[valid_row()]);

    let args = ingest_args(&input_path, &store_path);
    let (first_code, first_stdout, first_stderr) = run_runtime(&args);
    let (second_code, second_stdout, second_stderr) = run_runtime(&args);

    assert_eq!(first_code, 0, "{first_stderr}");
    assert_eq!(second_code, 0, "{second_stderr}");
    assert!(first_stdout.contains("events_written: 1"));
    assert!(second_stdout.contains("events_written: 0"));
    assert!(second_stdout.contains("duplicates: 1"));

    let store = JsonlEventStore::new(&store_path).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 1);

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn reports_invalid_timestamp() {
    let input_path = temp_path("invalid-timestamp", "parquet");
    let store_path = temp_path("invalid-timestamp-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[json!({
            "market_id": "market-1",
            "timestamp": "not-a-timestamp",
            "signal_name": "vwap_reversion",
            "strength": 0.8,
            "direction": "long_yes",
            "source": "polymarket",
            "metadata": "{\"source\":\"polymarket\"}"
        })],
    );

    let (code, stdout, stderr) = run_runtime(&ingest_args(&input_path, &store_path));

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("rows_invalid: 1"));
    assert!(stdout.contains("missing timestamp"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn reports_invalid_metadata() {
    let input_path = temp_path("invalid-metadata", "parquet");
    let store_path = temp_path("invalid-metadata-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[json!({
            "market_id": "market-1",
            "timestamp": "2026-04-03T17:57:41.047Z",
            "signal_name": "vwap_reversion",
            "strength": 0.8,
            "direction": "long_yes",
            "source": "polymarket",
            "metadata": "[]"
        })],
    );

    let (code, stdout, stderr) = run_runtime(&ingest_args(&input_path, &store_path));

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("metadata must be a JSON object string"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn reports_partial_or_blank_row() {
    let input_path = temp_path("partial-row", "parquet");
    let store_path = temp_path("partial-row-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[json!({
            "market_id": "",
            "timestamp": "2026-04-03T17:57:41.047Z",
            "signal_name": "",
            "strength": 0.8,
            "direction": "long_yes",
            "source": "polymarket",
            "metadata": "{\"source\":\"polymarket\"}"
        })],
    );

    let (code, stdout, stderr) = run_runtime(&ingest_args(&input_path, &store_path));

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("market_id cannot be blank"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn empty_file_reports_zero_rows_cleanly() {
    let input_path = temp_path("empty", "parquet");
    let store_path = temp_path("empty-store", "jsonl");

    write_research_parquet(&input_path, &[]);

    let mut args = ingest_args(&input_path, &store_path);
    args.push("--json".into());
    let (code, stdout, stderr) = run_runtime(&args);
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(parsed["rows_read"], 0);
    assert_eq!(parsed["rows_valid"], 0);
    assert_eq!(parsed["rows_invalid"], 0);
    assert_eq!(parsed["events_written"], 0);

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn reports_reasonable_error_for_missing_input_file() {
    let missing_path = temp_path("missing", "parquet");
    let store_path = temp_path("missing-store", "jsonl");

    let (code, _stdout, stderr) = run_runtime(&ingest_args(&missing_path, &store_path));

    assert_eq!(code, 1);
    assert!(stderr.contains("does not exist"));

    cleanup(&[&missing_path, &store_path]);
}

#[test]
fn scoring_agent_emits_market_scored_and_confirmation_requires_it() {
    let watch_dir = temp_path("watch-dir", "tmp");
    let store_path = temp_path("score-store", "jsonl");
    let snapshots_path = watch_dir.join("snapshots.jsonl");
    let raw_path = watch_dir.join("raw/polymarket-discovery.jsonl");

    fs::create_dir_all(&watch_dir).unwrap();
    write_jsonl(
        &snapshots_path,
        &[json!({
            "market_id": "market-1",
            "source": "Polymarket",
            "title": "Will the test market resolve?",
            "status": "Open",
            "best_bid": 0.58,
            "best_ask": 0.60,
            "last_price": 0.59,
            "volume": 60000.0,
            "observed_at": "2026-04-14T12:00:00Z"
        })],
    );
    write_jsonl(
        &raw_path,
        &[json!({
            "source": "Polymarket",
            "source_event_id": "evt-1",
            "payload_kind": "polymarket.discovery",
            "payload": serde_json::to_vec(&json!({
                "markets": [
                    {
                        "conditionId": "market-1",
                        "question": "Will the test market resolve?",
                        "resolutionDate": "2026-04-14T20:00:00Z"
                    }
                ]
            }))
            .unwrap(),
            "captured_at": "2026-04-14T12:00:00Z"
        })],
    );

    let scoring_args = vec![
        "--store".into(),
        store_path.display().to_string(),
        "--watch-dir".into(),
        watch_dir.display().to_string(),
    ];
    let (score_code, score_stdout, score_stderr) =
        run_python_script("agents/scoring_agent.py", &scoring_args);
    assert_eq!(score_code, 0, "{score_stderr}");
    assert!(score_stdout.contains("\"market_scored_this_run\":1"));

    let store = JsonlEventStore::new(&store_path).unwrap();
    let scored_events = store.read_all().unwrap();
    assert_eq!(
        scored_events
            .iter()
            .filter(|event| event.event_type.as_str() == "market.scored")
            .count(),
        1
    );

    store
        .append_events(&[
            make_signal_event("signal-1", "market-1", 0.92),
            make_signal_event("signal-2", "market-2", 0.93),
        ])
        .unwrap();

    let confirm_args = vec!["--store".into(), store_path.display().to_string()];
    let (confirm_code, confirm_stdout, confirm_stderr) =
        run_python_script("agents/confirmation_agent.py", &confirm_args);
    assert_eq!(confirm_code, 0, "{confirm_stderr}");
    assert!(confirm_stdout.contains("\"eligible_candidates\":1"));

    let events = store.read_all().unwrap();
    let confirmed_signals: Vec<_> = events
        .iter()
        .filter(|event| event.event_type.as_str() == "signal.confirmed")
        .collect();

    assert_eq!(confirmed_signals.len(), 1);
    assert_eq!(confirmed_signals[0].payload["signal_id"], "signal-1");

    cleanup(&[&store_path]);
    let _ = fs::remove_dir_all(&watch_dir);
}

#[test]
fn sizing_agent_applies_kelly_and_is_idempotent() {
    let watch_dir = temp_path("sizing-watch", "tmp");
    let store_path = temp_path("sizing-store", "jsonl");
    let snapshots_path = watch_dir.join("snapshots.jsonl");

    fs::create_dir_all(&watch_dir).unwrap();
    write_jsonl(
        &snapshots_path,
        &[
            json!({
                "market_id": "market-1",
                "source": "Polymarket",
                "title": "Positive edge market",
                "status": "Open",
                "best_bid": 0.52,
                "best_ask": 0.58,
                "observed_at": "2026-04-14T12:00:00Z"
            }),
            json!({
                "market_id": "market-2",
                "source": "Polymarket",
                "title": "Negative edge market",
                "status": "Open",
                "best_bid": 0.48,
                "best_ask": 0.52,
                "observed_at": "2026-04-14T12:05:00Z"
            }),
            json!({
                "market_id": "market-3",
                "source": "Polymarket",
                "title": "Pre-vetoed market",
                "status": "Open",
                "best_bid": 0.44,
                "best_ask": 0.48,
                "observed_at": "2026-04-14T12:10:00Z"
            }),
        ],
    );

    let store = JsonlEventStore::new(&store_path).unwrap();
    store
        .append_events(&[
            make_signal_event("signal-1", "polymarket:market-1", 0.8),
            make_signal_confirmed("signal-1", "polymarket:market-1", 0.9),
            make_signal_event("signal-2", "polymarket:market-2", 0.8),
            make_signal_confirmed("signal-2", "polymarket:market-2", 0.4),
            make_signal_event("signal-3", "polymarket:market-3", 0.8),
            make_signal_confirmed("signal-3", "polymarket:market-3", 0.9),
            make_signal_veto("signal-3", "polymarket:market-3"),
        ])
        .unwrap();

    let sizing_args = vec![
        "--store".into(),
        store_path.display().to_string(),
        "--watch-dir".into(),
        watch_dir.display().to_string(),
        "--bankroll".into(),
        "1000.0".into(),
    ];

    let (first_code, first_stdout, first_stderr) =
        run_python_script("agents/sizing_agent.py", &sizing_args);
    assert_eq!(first_code, 0, "{first_stderr}");
    assert!(first_stdout.contains("\"decisions_written\":1"));
    assert!(first_stdout.contains("\"vetoes_written\":1"));

    let first_events = store.read_all().unwrap();
    let decision_count = first_events
        .iter()
        .filter(|event| event.event_type.as_str() == "decision.formed")
        .count();
    let veto_count = first_events
        .iter()
        .filter(|event| event.event_type.as_str() == "veto.raised")
        .count();

    assert_eq!(decision_count, 1);
    assert_eq!(veto_count, 2);

    let decision_event = first_events
        .iter()
        .find(|event| event.event_type.as_str() == "decision.formed")
        .unwrap();
    assert_eq!(decision_event.payload["decision_id"], "decision-signal-1");
    assert_eq!(decision_event.payload["size_hint"], 250.0);
    assert_eq!(decision_event.payload["action"], "Enter");
    assert_eq!(
        decision_event.provenance.actor.as_deref(),
        Some("sizing-agent-v1")
    );

    let veto_event = first_events
        .iter()
        .find(|event| {
            event.event_type.as_str() == "veto.raised" && event.payload["target_id"] == "signal-2"
        })
        .unwrap();
    assert_eq!(veto_event.payload["reason_code"], "negative_ev");
    assert_eq!(
        veto_event.provenance.actor.as_deref(),
        Some("sizing-agent-v1")
    );

    let (second_code, second_stdout, second_stderr) =
        run_python_script("agents/sizing_agent.py", &sizing_args);
    assert_eq!(second_code, 0, "{second_stderr}");
    assert!(second_stdout.contains("\"decisions_written\":0"));
    assert!(second_stdout.contains("\"vetoes_written\":0"));

    let second_events = store.read_all().unwrap();
    assert_eq!(second_events.len(), first_events.len());

    cleanup(&[&store_path]);
    let _ = fs::remove_dir_all(&watch_dir);
}

#[test]
fn exit_agent_raises_decision_veto_for_target_hit_and_is_idempotent() {
    let watch_dir = temp_path("exit-watch", "tmp");
    let store_path = temp_path("exit-store", "jsonl");
    let snapshots_path = watch_dir.join("snapshots.jsonl");

    fs::create_dir_all(&watch_dir).unwrap();
    write_jsonl(
        &snapshots_path,
        &[json!({
            "market_id": "market-1",
            "source": "Polymarket",
            "title": "Exit target hit market",
            "status": "Open",
            "best_bid": 0.49,
            "best_ask": 0.51,
            "last_price": 0.50,
            "volume": 1000.0,
            "observed_at": "2026-04-14T12:00:00Z"
        })],
    );

    let store = JsonlEventStore::new(&store_path).unwrap();
    store
        .append_events(&[
            make_decision_formed("decision-1", "signal-1", "polymarket:market-1"),
            make_fill_received(
                "decision-1",
                "pm-paper-order-yes-decision-1",
                "polymarket:market-1",
                "fill-1",
                FillSide::Buy,
                10.0,
                0.40,
                "2026-04-14T11:00:00Z",
            ),
        ])
        .unwrap();

    let exit_args = vec![
        "--store".into(),
        store_path.display().to_string(),
        "--watch-dir".into(),
        watch_dir.display().to_string(),
    ];

    let (first_code, first_stdout, first_stderr) =
        run_python_script("agents/exit_agent.py", &exit_args);
    assert_eq!(first_code, 0, "{first_stderr}");
    assert!(first_stdout.contains("\"vetoes_written\":1"));

    let first_events = store.read_all().unwrap();
    let vetoes: Vec<_> = first_events
        .iter()
        .filter(|event| event.event_type.as_str() == "veto.raised")
        .collect();
    assert_eq!(vetoes.len(), 1);
    assert_eq!(vetoes[0].payload["reason_code"], "TARGET_HIT");
    assert_eq!(vetoes[0].payload["scope"], "Decision");
    assert_eq!(vetoes[0].provenance.actor.as_deref(), Some("exit-agent-v1"));

    let (second_code, second_stdout, second_stderr) =
        run_python_script("agents/exit_agent.py", &exit_args);
    assert_eq!(second_code, 0, "{second_stderr}");
    assert!(second_stdout.contains("\"vetoes_written\":0"));

    let second_events = store.read_all().unwrap();
    assert_eq!(second_events.len(), first_events.len());

    cleanup(&[&store_path]);
    let _ = fs::remove_dir_all(&watch_dir);
}
