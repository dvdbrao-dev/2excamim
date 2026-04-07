use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
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
    df["timestamp"] = pd.to_datetime(df["timestamp"], utc=True)
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

fn cleanup(paths: &[&Path]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

#[test]
fn ingests_valid_research_parquet_into_signal_generated_events() {
    let input_path = temp_path("valid", "parquet");
    let store_path = temp_path("valid-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[json!({
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
        })],
    );

    let (code, stdout, stderr) = run_runtime(&[
        "twoexcamim".into(),
        "ingest".into(),
        "research-signals".into(),
        input_path.display().to_string(),
        "--store".into(),
        store_path.display().to_string(),
    ]);

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Research Signal Ingest"));
    assert!(stdout.contains("accepted: 1"));

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

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn rejects_invalid_records_but_keeps_valid_ones() {
    let input_path = temp_path("invalid-row", "parquet");
    let store_path = temp_path("invalid-row-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[
            json!({
                "market_id": "market-1",
                "timestamp": "2026-04-03T17:57:41.047Z",
                "signal_name": "vwap_reversion",
                "strength": 0.8,
                "direction": "long_yes",
                "probability": 0.25,
                "spread_tight": 0.99,
                "volume_spike_24h": 1.0,
                "price_deviation_vwap_1h": -12.5,
                "source": "polymarket",
                "metadata": "{\"source\":\"polymarket\"}"
            }),
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

    let (code, stdout, stderr) = run_runtime(&[
        "twoexcamim".into(),
        "ingest".into(),
        "research-signals".into(),
        input_path.display().to_string(),
        "--store".into(),
        store_path.display().to_string(),
    ]);

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("accepted: 1"));
    assert!(stdout.contains("rejected: 1"));

    let store = JsonlEventStore::new(&store_path).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 1);

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn deduplicates_reingest_of_same_research_file() {
    let input_path = temp_path("dedupe", "parquet");
    let store_path = temp_path("dedupe-store", "jsonl");

    write_research_parquet(
        &input_path,
        &[json!({
            "market_id": "market-1",
            "timestamp": "2026-04-03T17:57:41.047Z",
            "signal_name": "vwap_reversion",
            "strength": 0.8,
            "direction": "long_yes",
            "probability": 0.25,
            "spread_tight": 0.99,
            "volume_spike_24h": 1.0,
            "price_deviation_vwap_1h": -12.5,
            "source": "polymarket",
            "metadata": "{\"source\":\"polymarket\"}"
        })],
    );

    let args = [
        "twoexcamim".into(),
        "ingest".into(),
        "research-signals".into(),
        input_path.display().to_string(),
        "--store".into(),
        store_path.display().to_string(),
    ];

    let (first_code, first_stdout, first_stderr) = run_runtime(&args);
    let (second_code, second_stdout, second_stderr) = run_runtime(&args);

    assert_eq!(first_code, 0, "{first_stderr}");
    assert_eq!(second_code, 0, "{second_stderr}");
    assert!(first_stdout.contains("accepted: 1"));
    assert!(second_stdout.contains("accepted: 0"));
    assert!(second_stdout.contains("deduplicated: 1"));

    let store = JsonlEventStore::new(&store_path).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 1);

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn reports_reasonable_error_for_missing_input_file() {
    let missing_path = temp_path("missing", "parquet");
    let store_path = temp_path("missing-store", "jsonl");

    let (code, _stdout, stderr) = run_runtime(&[
        "twoexcamim".into(),
        "ingest".into(),
        "research-signals".into(),
        missing_path.display().to_string(),
        "--store".into(),
        store_path.display().to_string(),
    ]);

    assert_eq!(code, 1);
    assert!(stderr.contains("does not exist"));

    cleanup(&[&missing_path, &store_path]);
}
