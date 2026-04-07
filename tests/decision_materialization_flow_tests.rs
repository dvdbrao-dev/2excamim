use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use twoexcamim::{
    materialize_decisions, runtime, DecisionMaterializationOptions, EventEnvelope, JsonlEventStore,
    Linkage, Provenance, QueryService, SignalConfirmed, SignalGenerated, SignalSide, SourceKind,
    StoredEvent, VetoRaised, VetoScope,
};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "twoexcamim-decision-materialization-{name}-{nanos}.jsonl"
    ))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Research,
        source_ref: Some("research-signals://tests".into()),
        producer_run_id: Some("run-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-1".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some(format!("hyp-{signal_id}")),
        signal_id: Some(signal_id.into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(format!("corr-{signal_id}")),
    }
}

fn make_signal_generated(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "research-handoff",
            Some(format!("instrument-{signal_id}")),
            signal_linkage(signal_id),
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some(format!("hyp-{signal_id}")),
                instrument: format!("instrument-{signal_id}"),
                timeframe: "research_snapshot".into(),
                side: SignalSide::Long,
                strength: 0.8,
                rationale: Some("generated for tests".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "analyst",
            Some(format!("instrument-{signal_id}")),
            signal_linkage(signal_id),
            provenance(),
            SignalConfirmed {
                signal_id: signal_id.into(),
                confirmed_by: "ops".into(),
                confirmation_reason: Some("approved".into()),
                confirmation_score: Some(0.9),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_veto(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "governance",
            None,
            signal_linkage(signal_id),
            provenance(),
            VetoRaised {
                veto_id: format!("veto-{signal_id}"),
                scope: VetoScope::Signal,
                target_id: signal_id.into(),
                reason_code: "risk".into(),
                reason_text: Some("blocked".into()),
                raised_by: "ops".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn dry_run_classifies_signals_by_materialization_readiness() {
    let path = temp_store_path("dry-run");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-eligible"),
            make_signal_confirmed("sig-eligible"),
            make_signal_generated("sig-weak"),
            make_signal_generated("sig-blocked"),
            make_signal_confirmed("sig-blocked"),
            make_signal_veto("sig-blocked"),
            make_signal_confirmed("sig-inconsistent"),
        ])
        .unwrap();

    let report = materialize_decisions(
        &QueryService::new(&store),
        DecisionMaterializationOptions { dry_run: true },
    )
    .unwrap();

    assert_eq!(report.signals_inspected, 4);
    assert_eq!(report.eligible, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.blocked, 1);
    assert_eq!(report.inconsistent, 1);
    assert_eq!(report.decisions_materialized, 0);

    let eligible_item = report
        .items
        .iter()
        .find(|item| item.signal_id == "sig-eligible")
        .unwrap();
    assert_eq!(eligible_item.policy_status, "Eligible");
    assert_eq!(format!("{:?}", eligible_item.disposition), "Eligible");
    assert_eq!(
        eligible_item.candidate_decision_id.as_deref(),
        Some("decision-sig-eligible")
    );

    cleanup(&path);
}

#[test]
fn materializes_decision_for_clearly_eligible_signal() {
    let path = temp_store_path("persist");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
        ])
        .unwrap();

    let report = materialize_decisions(
        &QueryService::new(&store),
        DecisionMaterializationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.eligible, 1);
    assert_eq!(report.decisions_materialized, 1);
    assert_eq!(report.duplicates, 0);

    let events = store.read_all().unwrap();
    let decision = events
        .iter()
        .find(|event| event.event_type.as_str() == "decision.formed")
        .unwrap();
    assert_eq!(decision.linkage.signal_id.as_deref(), Some("sig-1"));
    assert_eq!(
        decision.linkage.decision_id.as_deref(),
        Some("decision-sig-1")
    );
    assert_eq!(decision.produced_by, "runtime.decision_materialization");

    cleanup(&path);
}

#[test]
fn rerun_does_not_duplicate_materialized_decision() {
    let path = temp_store_path("dedupe");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
        ])
        .unwrap();

    let first = materialize_decisions(
        &QueryService::new(&store),
        DecisionMaterializationOptions { dry_run: false },
    )
    .unwrap();
    let second = materialize_decisions(
        &QueryService::new(&store),
        DecisionMaterializationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(first.decisions_materialized, 1);
    assert_eq!(second.decisions_materialized, 0);
    assert_eq!(second.skipped, 1);

    let decisions = store
        .read_all()
        .unwrap()
        .into_iter()
        .filter(|event| event.event_type.as_str() == "decision.formed")
        .count();
    assert_eq!(decisions, 1);

    cleanup(&path);
}

#[test]
fn runtime_cli_materialize_decisions_json_report_is_reasonable() {
    let path = temp_store_path("cli");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
        ])
        .unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = runtime::run(
        vec![
            "twoexcamim".to_string(),
            "materialize".to_string(),
            "decisions".to_string(),
            "--store".to_string(),
            path.display().to_string(),
            "--dry-run".to_string(),
            "--json".to_string(),
        ],
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 0, "{}", String::from_utf8_lossy(&stderr));
    let parsed: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(parsed["kind"], "materialize_decisions");
    assert_eq!(parsed["signals_inspected"], 1);
    assert_eq!(parsed["eligible"], 1);
    assert_eq!(parsed["decisions_materialized"], 0);

    cleanup(&path);
}
