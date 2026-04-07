use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{TimeZone, Utc};
use twoexcamim::{
    observe_fill, DecisionAction, DecisionFormed, EventEnvelope, FillObservationDisposition,
    FillObservationOptions, FillObservationRequest, JsonlEventStore, Linkage, OrderLifecycleStatus,
    OrderRegistered, OrderSubmitted, Provenance, QueryService, SignalConfirmed, SignalGenerated,
    SignalSide, SourceKind, StoredEvent,
};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-fill-observation-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("execution-observation://tests".into()),
        producer_run_id: Some("run-fill-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-fill-1".into()),
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
                strength: 0.81,
                rationale: Some("fixture".into()),
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
                confirmation_score: Some(0.93),
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
                rationale: Some("fixture".into()),
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

fn fill_request(order_id: &str, decision_id: &str, instrument: &str) -> FillObservationRequest {
    FillObservationRequest {
        fill_id: format!("fill-{order_id}"),
        order_id: order_id.into(),
        decision_id: Some(decision_id.into()),
        instrument: Some(instrument.into()),
        side: twoexcamim::FillSide::Buy,
        quantity: 2.0,
        price: 0.61,
        venue: Some("paper".into()),
        executed_at: Utc.with_ymd_and_hms(2026, 4, 7, 0, 0, 0).unwrap(),
    }
}

#[test]
fn dry_run_marks_submitted_order_fill_as_eligible() {
    let path = temp_store_path("dry-run");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed("sig-1", "dec-1"),
            make_order_registered("sig-1", "dec-1", "ord-1"),
            make_order_submitted("sig-1", "dec-1", "ord-1"),
        ])
        .unwrap();

    let report = observe_fill(
        &QueryService::new(&store),
        &fill_request("ord-1", "dec-1", "instrument-sig-1"),
        FillObservationOptions { dry_run: true },
    )
    .unwrap();

    assert_eq!(report.disposition, FillObservationDisposition::Eligible);
    assert!(!report.persisted);
    assert!(!report.duplicate);
    assert_eq!(report.resolved_decision_id.as_deref(), Some("dec-1"));

    let events = store.read_all().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        0
    );

    cleanup(&path);
}

#[test]
fn registered_order_is_skipped_until_submission_is_observed() {
    let path = temp_store_path("registered-only");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-2"),
            make_signal_confirmed("sig-2"),
            make_decision_formed("sig-2", "dec-2"),
            make_order_registered("sig-2", "dec-2", "ord-2"),
        ])
        .unwrap();

    let report = observe_fill(
        &QueryService::new(&store),
        &fill_request("ord-2", "dec-2", "instrument-sig-2"),
        FillObservationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.disposition, FillObservationDisposition::Skipped);
    assert!(!report.persisted);
    assert!(report
        .reasons
        .iter()
        .any(|reason| reason == "OrderNotSubmittedLocally"));

    let lifecycle = QueryService::new(&store)
        .order_lifecycle("ord-2")
        .unwrap()
        .unwrap();
    assert_eq!(lifecycle.status, OrderLifecycleStatus::Registered);

    cleanup(&path);
}

#[test]
fn mismatched_fill_relation_is_reported_as_inconsistent() {
    let path = temp_store_path("mismatch");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-3"),
            make_signal_confirmed("sig-3"),
            make_decision_formed("sig-3", "dec-3"),
            make_order_registered("sig-3", "dec-3", "ord-3"),
            make_order_submitted("sig-3", "dec-3", "ord-3"),
        ])
        .unwrap();

    let report = observe_fill(
        &QueryService::new(&store),
        &fill_request("ord-3", "dec-3", "wrong-instrument"),
        FillObservationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.disposition, FillObservationDisposition::Inconsistent);
    assert!(!report.persisted);
    assert!(report
        .reasons
        .iter()
        .any(|reason| reason.contains("InstrumentMismatch")));

    cleanup(&path);
}

#[test]
fn persists_fill_and_updates_order_lifecycle() {
    let path = temp_store_path("persist");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-4"),
            make_signal_confirmed("sig-4"),
            make_decision_formed("sig-4", "dec-4"),
            make_order_registered("sig-4", "dec-4", "ord-4"),
            make_order_submitted("sig-4", "dec-4", "ord-4"),
        ])
        .unwrap();

    let report = observe_fill(
        &QueryService::new(&store),
        &fill_request("ord-4", "dec-4", "instrument-sig-4"),
        FillObservationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(report.disposition, FillObservationDisposition::Observed);
    assert!(report.persisted);

    let lifecycle = QueryService::new(&store)
        .order_lifecycle("ord-4")
        .unwrap()
        .unwrap();
    assert_eq!(lifecycle.status, OrderLifecycleStatus::ObservedWithFills);
    assert!(lifecycle
        .observed_fill_ids
        .iter()
        .any(|fill_id| fill_id == "fill-ord-4"));

    cleanup(&path);
}

#[test]
fn duplicate_fill_observation_is_idempotent() {
    let path = temp_store_path("duplicate");
    let store = JsonlEventStore::new(&path).unwrap();
    store
        .append_events(&[
            make_signal_generated("sig-5"),
            make_signal_confirmed("sig-5"),
            make_decision_formed("sig-5", "dec-5"),
            make_order_registered("sig-5", "dec-5", "ord-5"),
            make_order_submitted("sig-5", "dec-5", "ord-5"),
        ])
        .unwrap();

    let request = fill_request("ord-5", "dec-5", "instrument-sig-5");
    let first = observe_fill(
        &QueryService::new(&store),
        &request,
        FillObservationOptions { dry_run: false },
    )
    .unwrap();
    let second = observe_fill(
        &QueryService::new(&store),
        &request,
        FillObservationOptions { dry_run: false },
    )
    .unwrap();

    assert_eq!(first.disposition, FillObservationDisposition::Observed);
    assert_eq!(second.disposition, FillObservationDisposition::Skipped);
    assert!(second.duplicate);

    let events = store.read_all().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type.as_str() == "fill.received")
            .count(),
        1
    );

    cleanup(&path);
}
