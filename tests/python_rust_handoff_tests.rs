use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};
use twoexcamim::{runtime, store::JsonlEventStore};

fn temp_path(name: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-handoff-{name}-{nanos}.{extension}"))
}

fn python_interpreter() -> &'static str {
    "research_prediction_markets/.venv/bin/python"
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
