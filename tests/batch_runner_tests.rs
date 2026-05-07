use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};
use twoexcamim::runtime;

fn temp_path(name: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "twoexcamim-batch-runner-{name}-{nanos}.{extension}"
    ))
}

fn python_interpreter() -> String {
    if let Ok(py) = std::env::var("PYTHON") {
        if !py.trim().is_empty() {
            return py;
        }
    }

    let venv_python = Path::new("research_prediction_markets/.venv/bin/python");
    if venv_python.exists() {
        return venv_python.display().to_string();
    }

    let sibling_repo_venv =
        Path::new("/root/2excamim/research_prediction_markets/.venv/bin/python");
    if sibling_repo_venv.exists() {
        return sibling_repo_venv.display().to_string();
    }

    "python3".to_string()
}

fn write_research_parquet(path: &Path, rows: &[serde_json::Value]) {
    let payload = serde_json::to_string(rows).unwrap();
    let interpreter = python_interpreter();
    let output = Command::new(&interpreter)
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
        "python_interpreter={} stderr={}",
        interpreter,
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

fn batch_args(input_path: &Path, store_path: &Path) -> Vec<String> {
    vec![
        "twoexcamim".into(),
        "run".into(),
        "batch".into(),
        "--research-signals".into(),
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

fn valid_row(signal_name: &str, market_id: &str) -> Value {
    json!({
        "market_id": market_id,
        "timestamp": "2026-04-03T17:57:41.047Z",
        "signal_name": signal_name,
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
fn batch_dry_run_reports_all_three_phases() {
    let input_path = temp_path("dry-run-input", "parquet");
    let store_path = temp_path("dry-run-store", "jsonl");
    write_research_parquet(&input_path, &[valid_row("vwap_reversion", "market-1")]);

    let mut args = batch_args(&input_path, &store_path);
    args.push("--dry-run".into());

    let (code, stdout, stderr) = run_runtime(&args);

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Batch Run"));
    assert!(stdout.contains("dry_run: true"));
    assert!(stdout.contains("ingest:"));
    assert!(stdout.contains("materialization:"));
    assert!(stdout.contains("final_summary:"));
    assert!(stdout.contains("rows_valid: 1"));
    assert!(stdout.contains("decisions_materialized: 0"));
    assert!(stdout.contains("total_events: 0"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn batch_invalid_input_path_fails_with_ingest_phase_error() {
    let input_path = temp_path("missing-input", "parquet");
    let store_path = temp_path("missing-store", "jsonl");

    let (code, _stdout, stderr) = run_runtime(&batch_args(&input_path, &store_path));

    assert_eq!(code, 1);
    assert!(stderr.contains("batch phase ingest failed"));
    assert!(stderr.contains("does not exist"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn batch_ingests_then_materializes_then_summarizes() {
    let input_path = temp_path("persist-input", "parquet");
    let store_path = temp_path("persist-store", "jsonl");
    write_research_parquet(&input_path, &[valid_row("vwap_reversion", "market-1")]);

    let (code, stdout, stderr) = run_runtime(&batch_args(&input_path, &store_path));

    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("rows_valid: 1"));
    assert!(stdout.contains("events_written: 1"));
    assert!(stdout.contains("signals_inspected: 1"));
    assert!(stdout.contains("decisions_materialized: 0"));
    assert!(stdout.contains("total_events: 1"));

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn batch_json_output_is_reasonable() {
    let input_path = temp_path("json-input", "parquet");
    let store_path = temp_path("json-store", "jsonl");
    write_research_parquet(&input_path, &[valid_row("vwap_reversion", "market-1")]);

    let mut args = batch_args(&input_path, &store_path);
    args.push("--dry-run".into());
    args.push("--json".into());

    let (code, stdout, stderr) = run_runtime(&args);
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(code, 0, "{stderr}");
    assert_eq!(parsed["kind"], "batch_run");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["ingest"]["kind"], "ingest_research_signals");
    assert_eq!(parsed["materialization"]["kind"], "materialize_decisions");
    assert_eq!(parsed["final_summary"]["kind"], "summary");

    cleanup(&[&input_path, &store_path]);
}

#[test]
fn batch_json_error_reports_failed_phase() {
    let input_path = temp_path("json-missing-input", "parquet");
    let store_path = temp_path("json-missing-store", "jsonl");

    let mut args = batch_args(&input_path, &store_path);
    args.push("--json".into());

    let (code, _stdout, stderr) = run_runtime(&args);
    let parsed: Value = serde_json::from_str(&stderr).unwrap();

    assert_eq!(code, 1);
    assert_eq!(parsed["kind"], "error");
    assert_eq!(parsed["exit_code"], 1);
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("batch phase ingest failed"));

    cleanup(&[&input_path, &store_path]);
}
