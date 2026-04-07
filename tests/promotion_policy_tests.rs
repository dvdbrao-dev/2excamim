use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, FillReceived, FillSide, Linkage,
    OrderRegistered, OrderSubmitted, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
    SourceKind, VetoRaised, VetoScope,
};
use twoexcamim::queries::{PromotionNextStep, PromotionPolicyStatus, QueryService};
use twoexcamim::store::{JsonlEventStore, StoredEvent};

fn temp_store_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("twoexcamim-promotion-policy-{name}-{nanos}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn runtime_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("promotion-policy://runtime".into()),
        producer_run_id: Some("run-promotion-policy-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-promotion-policy-1".into()),
        notes: None,
    }
}

fn venue_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::ExecutionVenue,
        source_ref: Some("binance".into()),
        producer_run_id: Some("run-promotion-fill-1".into()),
        actor: Some("venue".into()),
        trace_id: Some("trace-promotion-fill-1".into()),
        notes: None,
    }
}

fn signal_linkage(signal_id: &str) -> Linkage {
    Linkage {
        signal_id: Some(signal_id.into()),
        hypothesis_id: Some("hyp-1".into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn decision_linkage(signal_id: Option<&str>, decision_id: &str) -> Linkage {
    Linkage {
        signal_id: signal_id.map(str::to_string),
        hypothesis_id: Some("hyp-1".into()),
        decision_id: Some(decision_id.into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn order_linkage(order_id: &str, decision_id: Option<&str>) -> Linkage {
    Linkage {
        order_id: Some(order_id.into()),
        decision_id: decision_id.map(str::to_string),
        signal_id: Some("sig-1".into()),
        hypothesis_id: Some("hyp-1".into()),
        correlation_id: Some("corr-1".into()),
        ..Linkage::default()
    }
}

fn make_signal_generated(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id),
            runtime_provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: "BTCUSDT".into(),
                timeframe: "1h".into(),
                side: SignalSide::Long,
                strength: 0.8,
                rationale: Some("generated".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed(signal_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id),
            runtime_provenance(),
            SignalConfirmed {
                signal_id: signal_id.into(),
                confirmed_by: "risk-check".into(),
                confirmation_reason: Some("ok".into()),
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
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(signal_id),
            runtime_provenance(),
            VetoRaised {
                veto_id: format!("veto-{signal_id}"),
                scope: VetoScope::Signal,
                target_id: signal_id.into(),
                reason_code: "risk_limit".into(),
                reason_text: Some("blocked".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_formed(signal_id: Option<&str>, decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some("BTCUSDT".into()),
            decision_linkage(signal_id, decision_id),
            runtime_provenance(),
            DecisionFormed {
                decision_id: decision_id.into(),
                instrument: "BTCUSDT".into(),
                action: DecisionAction::Enter,
                side: Some(SignalSide::Long),
                size_hint: Some(1.0),
                rationale: Some("follow".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_veto(decision_id: &str) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some("BTCUSDT".into()),
            Linkage {
                decision_id: Some(decision_id.into()),
                correlation_id: Some("corr-1".into()),
                ..Linkage::default()
            },
            runtime_provenance(),
            VetoRaised {
                veto_id: format!("veto-{decision_id}"),
                scope: VetoScope::Decision,
                target_id: decision_id.into(),
                reason_code: "manual_block".into(),
                reason_text: Some("blocked".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_registered(order_id: &str, decision_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_registered(
            "runtime",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            runtime_provenance(),
            OrderRegistered {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: "BTCUSDT".into(),
                venue: "binance".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_order_submitted(order_id: &str, decision_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_order_submitted(
            "runtime",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            runtime_provenance(),
            OrderSubmitted {
                order_id: order_id.into(),
                decision_id: decision_id.map(str::to_string),
                instrument: "BTCUSDT".into(),
                venue: "binance".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill_received(fill_id: &str, order_id: &str, decision_id: Option<&str>) -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some("BTCUSDT".into()),
            order_linkage(order_id, decision_id),
            venue_provenance(),
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: decision_id.map(str::to_string),
                order_id: order_id.into(),
                instrument: "BTCUSDT".into(),
                side: FillSide::Buy,
                quantity: 1.0,
                price: 100.0,
                venue: "binance".into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn store_with_events(name: &str, events: Vec<StoredEvent>) -> (JsonlEventStore, PathBuf) {
    let path = temp_store_path(name);
    let store = JsonlEventStore::new(&path).unwrap();
    store.append_events(&events).unwrap();
    (store, path)
}

#[test]
fn signal_confirmed_is_eligible_for_decision() {
    let (store, path) = store_with_events(
        "signal-eligible",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.signal_promotion_policy("sig-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Eligible);
    assert_eq!(report.next_step, Some(PromotionNextStep::FormDecision));
    cleanup(&path);
}

#[test]
fn signal_unconfirmed_is_weak() {
    let (store, path) = store_with_events("signal-weak", vec![make_signal_generated("sig-1")]);
    let service = QueryService::new(&store);

    let report = service.signal_promotion_policy("sig-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Weak);
    cleanup(&path);
}

#[test]
fn signal_vetoed_is_blocked() {
    let (store, path) = store_with_events(
        "signal-blocked",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_signal_veto("sig-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.signal_promotion_policy("sig-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Blocked);
    assert_eq!(report.next_step, None);
    cleanup(&path);
}

#[test]
fn signal_without_generation_is_inconsistent() {
    let (store, path) = store_with_events(
        "signal-inconsistent",
        vec![make_decision_formed(Some("sig-1"), "dec-1")],
    );
    let service = QueryService::new(&store);

    let report = service.signal_promotion_policy("sig-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Inconsistent);
    cleanup(&path);
}

#[test]
fn decision_with_sane_upstream_and_no_orders_is_eligible_for_order() {
    let (store, path) = store_with_events(
        "decision-eligible",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_promotion_policy("dec-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Eligible);
    assert_eq!(report.next_step, Some(PromotionNextStep::RegisterOrder));
    cleanup(&path);
}

#[test]
fn decision_with_registered_order_but_no_submission_is_frozen() {
    let (store, path) = store_with_events(
        "decision-frozen-registered-order",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_promotion_policy("dec-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Frozen);
    assert_eq!(report.next_step, Some(PromotionNextStep::SubmitOrder));
    cleanup(&path);
}

#[test]
fn decision_blocked_by_veto_is_blocked() {
    let (store, path) = store_with_events(
        "decision-blocked",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1"),
            make_decision_veto("dec-1"),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.decision_promotion_policy("dec-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Blocked);
    cleanup(&path);
}

#[test]
fn order_registered_only_is_frozen() {
    let (store, path) = store_with_events(
        "order-frozen",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.order_promotion_policy("ord-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Frozen);
    assert_eq!(report.next_step, Some(PromotionNextStep::SubmitOrder));
    cleanup(&path);
}

#[test]
fn order_submitted_is_eligible_for_execution_follow_up() {
    let (store, path) = store_with_events(
        "order-eligible",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
            make_order_submitted("ord-1", Some("dec-1")),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.order_promotion_policy("ord-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Eligible);
    assert_eq!(report.next_step, Some(PromotionNextStep::ObserveExecution));
    cleanup(&path);
}

#[test]
fn order_with_observed_fill_is_eligible_for_reconciliation_follow_up() {
    let (store, path) = store_with_events(
        "order-observed",
        vec![
            make_signal_generated("sig-1"),
            make_signal_confirmed("sig-1"),
            make_decision_formed(Some("sig-1"), "dec-1"),
            make_order_registered("ord-1", Some("dec-1")),
            make_order_submitted("ord-1", Some("dec-1")),
            make_fill_received("fill-1", "ord-1", Some("dec-1")),
        ],
    );
    let service = QueryService::new(&store);

    let report = service.order_promotion_policy("ord-1").unwrap().unwrap();
    let decision = service.decision_promotion_policy("dec-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Eligible);
    assert_eq!(
        report.next_step,
        Some(PromotionNextStep::ReconcileObservedExecution)
    );
    assert_eq!(decision.status, PromotionPolicyStatus::Frozen);
    cleanup(&path);
}

#[test]
fn order_submitted_without_registration_is_inconsistent() {
    let (store, path) = store_with_events(
        "order-inconsistent",
        vec![make_order_submitted("ord-1", Some("dec-1"))],
    );
    let service = QueryService::new(&store);

    let report = service.order_promotion_policy("ord-1").unwrap().unwrap();

    assert_eq!(report.status, PromotionPolicyStatus::Inconsistent);
    cleanup(&path);
}
