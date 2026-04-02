use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use twoexcamim::scenarios::{available_fixtures, load_fixture_named, ReplayHarness, ScenarioError};
use twoexcamim::store::JsonlEventStore;

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

#[test]
fn executes_confirmed_then_filled_signal_fixture() {
    let fixture = load_fixture_named("confirmed_then_filled_signal").unwrap();
    let path = temp_store_path("scenario-confirmed");
    let store = JsonlEventStore::new(&path).unwrap();
    let harness = ReplayHarness::new(&store);

    let result = harness.run_fixture(&fixture).unwrap();

    assert_eq!(result.total_events, 4);
    assert_eq!(result.signal_projections.len(), 1);
    assert_eq!(result.decision_projections.len(), 1);
    assert_eq!(result.confirmed_signals_count, 1);
    assert_eq!(result.vetoed_signals_count, 0);
    assert_eq!(result.decisions_with_fills_count, 1);
    assert_eq!(result.decisions_without_fills_count, 0);
    assert_eq!(result.signal_projections[0].signal_id, "sig-confirmed-1");
    assert!(result.signal_projections[0].confirmed);
    assert_eq!(
        result.decision_projections[0].decision_id,
        "dec-confirmed-1"
    );
    assert_eq!(result.decision_projections[0].fills_count, 1);

    cleanup(&path);
}

#[test]
fn executes_vetoed_signal_without_fill_fixture() {
    let fixture = load_fixture_named("vetoed_signal_without_fill").unwrap();
    let path = temp_store_path("scenario-vetoed");
    let store = JsonlEventStore::new(&path).unwrap();
    let harness = ReplayHarness::new(&store);

    let result = harness.run_fixture(&fixture).unwrap();

    assert_eq!(result.total_events, 3);
    assert_eq!(result.signal_projections.len(), 1);
    assert_eq!(result.decision_projections.len(), 1);
    assert_eq!(result.confirmed_signals_count, 0);
    assert_eq!(result.vetoed_signals_count, 1);
    assert_eq!(result.decisions_with_fills_count, 0);
    assert_eq!(result.decisions_without_fills_count, 1);
    assert_eq!(result.signal_projections[0].signal_id, "sig-vetoed-1");
    assert!(result.signal_projections[0].vetoed);
    assert_eq!(result.decision_projections[0].decision_id, "dec-vetoed-1");
    assert_eq!(result.decision_projections[0].fills_count, 0);

    cleanup(&path);
}

#[test]
fn executes_decision_with_multiple_fills_fixture() {
    let fixture = load_fixture_named("decision_with_multiple_fills").unwrap();
    let path = temp_store_path("scenario-multiple-fills");
    let store = JsonlEventStore::new(&path).unwrap();
    let harness = ReplayHarness::new(&store);

    let result = harness.run_fixture(&fixture).unwrap();

    assert_eq!(result.total_events, 4);
    assert_eq!(result.signal_projections.len(), 1);
    assert_eq!(result.decision_projections.len(), 1);
    assert_eq!(result.confirmed_signals_count, 0);
    assert_eq!(result.vetoed_signals_count, 0);
    assert_eq!(result.decisions_with_fills_count, 1);
    assert_eq!(result.decisions_without_fills_count, 0);
    assert_eq!(
        result.decision_projections[0].decision_id,
        "dec-multi-fill-1"
    );
    assert_eq!(result.decision_projections[0].fills_count, 2);
    assert_eq!(result.decision_projections[0].filled_quantity, 3.0);
    assert_eq!(
        result.decision_projections[0].average_fill_price,
        Some(102.0)
    );

    cleanup(&path);
}

#[test]
fn rerunning_fixture_does_not_duplicate_events() {
    let fixture = load_fixture_named("confirmed_then_filled_signal").unwrap();
    let path = temp_store_path("scenario-rerun");
    let store = JsonlEventStore::new(&path).unwrap();
    let harness = ReplayHarness::new(&store);

    let first = harness.run_fixture(&fixture).unwrap();
    let second = harness.run_fixture(&fixture).unwrap();

    assert_eq!(first.total_events, 4);
    assert_eq!(second.total_events, 4);
    assert_eq!(first, second);

    cleanup(&path);
}

#[test]
fn exposes_fixture_catalog_and_named_loading() {
    let names = available_fixtures();

    assert_eq!(
        names,
        vec![
            "confirmed_then_filled_signal",
            "vetoed_signal_without_fill",
            "decision_with_multiple_fills"
        ]
    );
    assert!(load_fixture_named("confirmed_then_filled_signal").is_ok());
}

#[test]
fn rejects_unknown_fixture_name() {
    let error = load_fixture_named("missing-fixture").unwrap_err();

    assert!(matches!(error, ScenarioError::UnknownFixture(name) if name == "missing-fixture"));
}
