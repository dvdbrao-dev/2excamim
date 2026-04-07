use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use twoexcamim::{
    materialize_orders, runtime, submit_orders, DecisionAction, DecisionFormed, EventEnvelope,
    JsonlEventStore, Linkage, OrderMaterializationOptions, OrderRegistered, OrderSubmissionOptions,
    OrderSubmitted, Provenance, QueryService, SignalConfirmed, SignalGenerated, SignalSide,
    SourceKind, StoredEvent, VetoRaised, VetoScope,
};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-order-submission-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("order-submission://tests".into()),
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

fn decision_linkage(signal_id: &str, decision_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some(format!("hyp-{signal_id}")),
        signal_id: Some(signal_id.into()),
        decision_id: Some(decision_id.into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(format!("corr-{decision_id}")),
    }
}

fn order_linkage(signal_id: &str, decision_id: &str, order_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: Some(format!("hyp-{signal_id}")),
        signal_id: Some(signal_id.into()),
        decision_id: Some(decision_id.into()),
        order_id: Some(order_id.into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(format!("corr-{decision_id}")),
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

fn make_decision_formed(signal_id: &str, decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(format!("instrument-{signal_id}")),
            decision_linkage(signal_id, decision_id),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: format!("instrument-{signal_id}"),
                action: DecisionAction::Enter,
                side: Some(SignalSide::Long),
                size_hint: Some(1.0),
                rationale: Some("generated for tests".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_veto(signal_id: &str, decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "governance",
            None,
            decision_linkage(signal_id, decision_id),
            provenance(),
            VetoRaised {
                veto_id: format!("veto-{decision_id}"),
                scope: VetoScope::Decision,
                target_id: decision_id.into(),
                reason_code: "risk".into(),
                reason_text: Some("blocked".into()),
                raised_by: "ops".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_registered(signal_id: &str, decision_id: &str, order_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "runtime.order_materialization",
            Some(format!("instrument-{signal_id}")),
            order_linkage(signal_id, decision_id, order_id),
            provenance(),
            OrderRegistered {
                order_id: order_id.into(),
                decision_id: Some(decision_id.into()),
                instrument: format!("instrument-{signal_id}"),
                venue: "paper".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_submitted(signal_id: &str, decision_id: &str, order_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_submitted(
            "runtime.order_submission",
            Some(format!("instrument-{signal_id}")),
            order_linkage(signal_id, decision_id, order_id),
            provenance(),
            OrderSubmitted {
                order_id: order_id.into(),
                decision_id: Some(decision_id.into()),
                instrument: format!("instrument-{signal_id}"),
                venue: "paper".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn dry_run_classifies_orders_by_submission_readiness() {
    let path = temp_store_path("dry-run");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-eligible"),
            make_signal_confirmed("sig-eligible"),
            make_decision_formed("sig-eligible", "dec-eligible"),
            make_order_registered("sig-eligible", "dec-eligible", "ord-eligible"),
            make_signal_generated("sig-skipped"),
            make_signal_confirmed("sig-skipped"),
            make_decision_formed("sig-skipped", "dec-skipped"),
            make_order_registered("sig-skipped", "dec-skipped", "ord-skipped"),
            make_order_submitted("sig-skipped", "dec-skipped", "ord-skipped"),
            make_signal_generated("sig-blocked"),
            make_signal_confirmed("sig-blocked"),
            make_decision_formed("sig-blocked", "dec-blocked"),
            make_decision_veto("sig-blocked", "dec-blocked"),
            make_order_registered("sig-blocked", "dec-blocked", "ord-blocked"),
            make_order_submitted("sig-missing", "dec-missing", "ord-inconsistent"),
        ])
        .unwrap();

    let report = submit_orders(
        &QueryService::new(&store),
        OrderSubmissionOptions { dry_run: true },
    )
    .unwrap();

    assert_eq!(report.orders_inspected, 4);
    assert_eq!(report.eligible, 1);
    assert_eq!(report.submitted, 0);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.blocked, 1);
    assert_eq!(report.inconsistent, 1);
    assert_eq!(report.duplicates, 0);

    cleanup(&path);
}

#[test]
fn persists_order_submitted_for_clearly_eligible_order() {
    let path = temp_store_path("persist");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed("sig-1", "dec-1"),
            make_order_registered("sig-1", "dec-1", "ord-1"),
        ])
        .unwrap();

    let report = submit_orders(
        &QueryService::new(&store),
        OrderSubmissionOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.eligible, 1);
    assert_eq!(report.submitted, 1);

    let events = store.read_all().unwrap();
    let submitted = events
        .iter()
        .find(|event| event.event_type.as_str() == "order.submitted")
        .unwrap();
    assert_eq!(submitted.linkage.order_id.as_deref(), Some("ord-1"));
    assert_eq!(submitted.produced_by, "runtime.order_submission");
    assert_eq!(submitted.payload["venue"].as_str(), Some("paper"));

    cleanup(&path);
}

#[test]
fn rerun_does_not_duplicate_submitted_order() {
    let path = temp_store_path("dedupe");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed("sig-1", "dec-1"),
            make_order_registered("sig-1", "dec-1", "ord-1"),
        ])
        .unwrap();

    let first = submit_orders(
        &QueryService::new(&store),
        OrderSubmissionOptions { dry_run: false },
    )
    .unwrap();
    let second = submit_orders(
        &QueryService::new(&store),
        OrderSubmissionOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(first.submitted, 1);
    assert_eq!(second.submitted, 0);
    assert_eq!(second.skipped, 1);

    cleanup(&path);
}

#[test]
fn order_materialization_then_submission_forms_clean_runtime_path() {
    let path = temp_store_path("chain");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed("sig-1", "dec-1"),
        ])
        .unwrap();

    let materialization = materialize_orders(
        &QueryService::new(&store),
        OrderMaterializationOptions { dry_run: false },
    )
    .unwrap();
    let submission = submit_orders(
        &QueryService::new(&store),
        OrderSubmissionOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(materialization.orders_registered, 1);
    assert_eq!(submission.submitted, 1);

    cleanup(&path);
}

#[test]
fn runtime_cli_submit_orders_json_report_is_reasonable() {
    let path = temp_store_path("cli");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed("sig-1", "dec-1"),
            make_order_registered("sig-1", "dec-1", "ord-1"),
        ])
        .unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = runtime::run(
        vec![
            "twoexcamim".to_string(),
            "submit".to_string(),
            "orders".to_string(),
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
    assert_eq!(parsed["kind"], "submit_orders");
    assert_eq!(parsed["orders_inspected"], 1);
    assert_eq!(parsed["eligible"], 1);
    assert_eq!(parsed["submitted"], 0);

    cleanup(&path);
}
