use chrono::Utc;
use pretty_assertions::assert_eq;
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, EventType, FillReceived, FillSide, Linkage,
    Provenance, SignalConfirmed, SignalGenerated, SignalSide, SourceKind, VetoRaised, VetoScope,
};
use twoexcamim::projections::{
    build_decision_projections, build_signal_projections, timeline_for_correlation_id,
    timeline_for_decision_id, timeline_for_signal_id,
};
use twoexcamim::store::StoredEvent;

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("projection://tests".into()),
        producer_run_id: Some("run-proj-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-proj-1".into()),
        notes: None,
    }
}

fn signal_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: None,
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
    }
}

fn decision_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: None,
        position_id: None,
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
    }
}

fn fill_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: Some("ord-1".into()),
        position_id: None,
        parent_event_id: None,
        correlation_id: Some("corr-1".into()),
    }
}

fn make_signal_generated() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("BTCUSDT".into()),
            signal_linkage(),
            provenance(),
            SignalGenerated {
                signal_id: "sig-1".into(),
                hypothesis_id: Some("hyp-1".into()),
                instrument: "BTCUSDT".into(),
                timeframe: "1h".into(),
                side: SignalSide::Long,
                strength: 0.83,
                rationale: Some("momentum".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_signal_confirmed() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_signal_confirmed(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(),
            provenance(),
            SignalConfirmed {
                signal_id: "sig-1".into(),
                confirmed_by: "risk-check".into(),
                confirmation_reason: Some("validated".into()),
                confirmation_score: Some(0.91),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_veto_for_signal() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_veto_raised(
            "risk-engine",
            Some("BTCUSDT".into()),
            signal_linkage(),
            provenance(),
            VetoRaised {
                veto_id: "veto-1".into(),
                scope: VetoScope::Signal,
                target_id: "sig-1".into(),
                reason_code: "risk_limit".into(),
                reason_text: Some("too much exposure".into()),
                raised_by: "risk-engine".into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_decision_formed() -> StoredEvent {
    StoredEvent::try_from(
        EventEnvelope::new_decision_formed(
            "decision-engine",
            Some("BTCUSDT".into()),
            decision_linkage(),
            provenance(),
            DecisionFormed {
                decision_id: "dec-1".into(),
                instrument: "BTCUSDT".into(),
                action: DecisionAction::Enter,
                side: Some(SignalSide::Long),
                size_hint: Some(1.2),
                rationale: Some("follow signal".into()),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn make_fill(order_id: &str, fill_id: &str, quantity: f64, price: f64) -> StoredEvent {
    let mut linkage = fill_linkage();
    linkage.order_id = Some(order_id.into());

    StoredEvent::try_from(
        EventEnvelope::new_fill_received(
            "execution-gateway",
            Some("BTCUSDT".into()),
            linkage,
            provenance(),
            FillReceived {
                fill_id: fill_id.into(),
                decision_id: Some("dec-1".into()),
                order_id: order_id.into(),
                instrument: "BTCUSDT".into(),
                side: FillSide::Buy,
                quantity,
                price,
                venue: "binance".into(),
                executed_at: Utc::now(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn builds_signal_projection_from_signal_and_decision_events() {
    let events = vec![
        make_signal_generated(),
        make_signal_confirmed(),
        make_veto_for_signal(),
        make_decision_formed(),
    ];

    let projections = build_signal_projections(&events).unwrap();

    assert_eq!(projections.len(), 1);
    assert_eq!(
        projections[0],
        twoexcamim::projections::SignalProjection {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: Some("BTCUSDT".into()),
            timeframe: Some("1h".into()),
            generated: true,
            confirmed: true,
            vetoed: true,
            decision_ids: vec!["dec-1".into()],
            last_event_type: EventType::DecisionFormed,
            correlation_id: Some("corr-1".into()),
        }
    );
}

#[test]
fn builds_decision_projection_from_decision_and_fill_events() {
    let events = vec![
        make_decision_formed(),
        make_fill("ord-1", "fill-1", 2.0, 100.0),
    ];

    let projections = build_decision_projections(&events).unwrap();

    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].decision_id, "dec-1");
    assert_eq!(projections[0].instrument.as_deref(), Some("BTCUSDT"));
    assert_eq!(projections[0].action, Some(DecisionAction::Enter));
    assert_eq!(projections[0].side, Some(SignalSide::Long));
    assert!(projections[0].formed);
    assert!(!projections[0].vetoed);
    assert_eq!(projections[0].fills_count, 1);
    assert_eq!(projections[0].filled_quantity, 2.0);
    assert_eq!(projections[0].average_fill_price, Some(100.0));
    assert_eq!(projections[0].order_ids, vec!["ord-1".to_string()]);
    assert_eq!(projections[0].last_event_type, EventType::FillReceived);
    assert_eq!(projections[0].correlation_id.as_deref(), Some("corr-1"));
}

#[test]
fn calculates_weighted_average_fill_price() {
    let events = vec![
        make_decision_formed(),
        make_fill("ord-1", "fill-1", 2.0, 100.0),
        make_fill("ord-1", "fill-2", 1.0, 106.0),
    ];

    let projections = build_decision_projections(&events).unwrap();

    assert_eq!(projections[0].fills_count, 2);
    assert_eq!(projections[0].filled_quantity, 3.0);
    assert_eq!(projections[0].average_fill_price, Some(102.0));
}

#[test]
fn filters_timeline_by_correlation_id() {
    let events = vec![
        make_signal_generated(),
        make_signal_confirmed(),
        make_decision_formed(),
    ];

    let timeline = timeline_for_correlation_id(&events, "corr-1");

    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0].event_type, EventType::SignalGenerated);
    assert_eq!(timeline[1].event_type, EventType::SignalConfirmed);
    assert_eq!(timeline[2].event_type, EventType::DecisionFormed);
}

#[test]
fn filters_timeline_by_signal_id() {
    let events = vec![
        make_signal_generated(),
        make_signal_confirmed(),
        make_decision_formed(),
    ];

    let timeline = timeline_for_signal_id(&events, "sig-1");

    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0].event_type, EventType::SignalGenerated);
    assert_eq!(timeline[1].event_type, EventType::SignalConfirmed);
    assert_eq!(timeline[2].event_type, EventType::DecisionFormed);
}

#[test]
fn filters_timeline_by_decision_id() {
    let events = vec![
        make_signal_generated(),
        make_decision_formed(),
        make_fill("ord-1", "fill-1", 2.0, 100.0),
    ];

    let timeline = timeline_for_decision_id(&events, "dec-1");

    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0].event_type, EventType::DecisionFormed);
    assert_eq!(timeline[1].event_type, EventType::FillReceived);
}
