use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use twoexcamim::{
    materialize_orders, runtime, DecisionAction, DecisionFormed, EventEnvelope, JsonlEventStore,
    Linkage, OrderMaterializationOptions, OrderRegistered, Provenance, QueryService,
    SignalConfirmed, SignalGenerated, SignalSide, SourceKind, StoredEvent, VetoRaised, VetoScope,
};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "twoexcamim-order-materialization-{name}-{nanos}.jsonl"
    ))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("order-materialization://tests".into()),
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

fn decision_linkage(signal_id: Option<&str>, decision_id: &str) -> Linkage {
    Linkage {
        hypothesis_id: signal_id.map(|value| format!("hyp-{value}")),
        signal_id: signal_id.map(str::to_string),
        decision_id: Some(decision_id.into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(format!("corr-{decision_id}")),
    }
}

fn decision_linkage_with_hypothesis(
    signal_id: Option<&str>,
    decision_id: &str,
    hypothesis_id: Option<&str>,
) -> Linkage {
    Linkage {
        hypothesis_id: hypothesis_id.map(str::to_string),
        signal_id: signal_id.map(str::to_string),
        decision_id: Some(decision_id.into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some(format!("corr-{decision_id}")),
    }
}

fn order_linkage(decision_id: &str, order_id: &str, signal_id: Option<&str>) -> Linkage {
    Linkage {
        hypothesis_id: signal_id.map(|value| format!("hyp-{value}")),
        signal_id: signal_id.map(str::to_string),
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

fn make_decision_formed(
    signal_id: Option<&str>,
    decision_id: &str,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(instrument.into()),
            decision_linkage(signal_id, decision_id),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: instrument.into(),
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

fn make_decision_formed_with_hypothesis(
    signal_id: Option<&str>,
    decision_id: &str,
    hypothesis_id: Option<&str>,
    instrument: &str,
) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some(instrument.into()),
            decision_linkage_with_hypothesis(signal_id, decision_id, hypothesis_id),
            provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: instrument.into(),
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

fn make_decision_veto(decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "governance",
            None,
            Linkage {
                decision_id: Some(decision_id.into()),
                correlation_id: Some(format!("corr-{decision_id}")),
                ..Linkage::default()
            },
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

fn make_order_registered(decision_id: &str, signal_id: Option<&str>) -> StoredEvent {
    let order_id = format!("order-{decision_id}");
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "runtime.order_materialization",
            Some(format!("instrument-{}", signal_id.unwrap_or("none"))),
            order_linkage(decision_id, &order_id, signal_id),
            provenance(),
            OrderRegistered {
                order_id,
                decision_id: Some(decision_id.into()),
                instrument: format!("instrument-{}", signal_id.unwrap_or("none")),
                venue: "paper".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn dry_run_classifies_decisions_by_order_materialization_readiness() {
    let path = temp_store_path("dry-run");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-eligible"),
            make_signal_confirmed("sig-eligible"),
            make_decision_formed(
                Some("sig-eligible"),
                "dec-eligible",
                "instrument-sig-eligible",
            ),
            make_signal_generated("sig-skipped"),
            make_signal_confirmed("sig-skipped"),
            make_decision_formed(Some("sig-skipped"), "dec-skipped", "instrument-sig-skipped"),
            make_order_registered("dec-skipped", Some("sig-skipped")),
            make_signal_generated("sig-blocked"),
            make_signal_confirmed("sig-blocked"),
            make_decision_formed(Some("sig-blocked"), "dec-blocked", "instrument-sig-blocked"),
            make_decision_veto("dec-blocked"),
            make_signal_generated("sig-inconsistent"),
            make_signal_confirmed("sig-inconsistent"),
            make_decision_formed_with_hypothesis(
                Some("sig-inconsistent"),
                "dec-inconsistent",
                Some("hyp-other"),
                "instrument-sig-inconsistent",
            ),
        ])
        .unwrap();

    let report = materialize_orders(
        &QueryService::new(&store),
        OrderMaterializationOptions { dry_run: true },
    )
    .unwrap();

    assert_eq!(report.decisions_inspected, 4);
    assert_eq!(report.eligible, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.blocked, 1);
    assert_eq!(report.inconsistent, 1);
    assert_eq!(report.orders_registered, 0);

    let eligible_item = report
        .items
        .iter()
        .find(|item| item.decision_id == "dec-eligible")
        .unwrap();
    assert_eq!(eligible_item.policy_status, "Eligible");
    assert_eq!(format!("{:?}", eligible_item.disposition), "Eligible");
    assert_eq!(
        eligible_item.candidate_order_id.as_deref(),
        Some("order-dec-eligible")
    );

    cleanup(&path);
}

#[test]
fn persists_order_registered_for_clearly_eligible_decision() {
    let path = temp_store_path("persist");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "instrument-sig-1"),
        ])
        .unwrap();

    let report = materialize_orders(
        &QueryService::new(&store),
        OrderMaterializationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.eligible, 1);
    assert_eq!(report.orders_registered, 1);
    assert_eq!(report.duplicates, 0);

    let events = store.read_all().unwrap();
    let order = events
        .iter()
        .find(|event| event.event_type.as_str() == "order.registered")
        .unwrap();
    assert_eq!(order.linkage.decision_id.as_deref(), Some("dec-1"));
    assert_eq!(order.linkage.order_id.as_deref(), Some("order-dec-1"));
    assert_eq!(order.produced_by, "runtime.order_materialization");
    assert_eq!(order.payload["venue"].as_str(), Some("paper"));

    cleanup(&path);
}

#[test]
fn skips_decision_with_existing_local_order() {
    let path = temp_store_path("existing-order");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "instrument-sig-1"),
            make_order_registered("dec-1", Some("sig-1")),
        ])
        .unwrap();

    let report = materialize_orders(
        &QueryService::new(&store),
        OrderMaterializationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.orders_registered, 0);
    assert_eq!(report.skipped, 1);
    assert!(report.items[0]
        .reasons
        .iter()
        .any(|reason| reason.contains("ExistingOrderIds")));

    cleanup(&path);
}

#[test]
fn rerun_does_not_duplicate_materialized_order() {
    let path = temp_store_path("dedupe");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "instrument-sig-1"),
        ])
        .unwrap();

    let first = materialize_orders(
        &QueryService::new(&store),
        OrderMaterializationOptions { dry_run: false },
    )
    .unwrap();
    let second = materialize_orders(
        &QueryService::new(&store),
        OrderMaterializationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(first.orders_registered, 1);
    assert_eq!(second.orders_registered, 0);
    assert_eq!(second.skipped, 1);

    let orders = store
        .read_all()
        .unwrap()
        .into_iter()
        .filter(|event| event.event_type.as_str() == "order.registered")
        .count();
    assert_eq!(orders, 1);

    cleanup(&path);
}

#[test]
fn runtime_cli_materialize_orders_json_report_is_reasonable() {
    let path = temp_store_path("cli");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1", "instrument-sig-1"),
        ])
        .unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = runtime::run(
        vec![
            "twoexcamim".to_string(),
            "materialize".to_string(),
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
    assert_eq!(parsed["kind"], "materialize_orders");
    assert_eq!(parsed["decisions_inspected"], 1);
    assert_eq!(parsed["eligible"], 1);
    assert_eq!(parsed["orders_registered"], 0);

    cleanup(&path);
}
