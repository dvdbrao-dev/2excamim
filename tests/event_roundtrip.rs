use chrono::Utc;
use twoexcamim::events::{
    EventEnvelope, FillReceived, FillSide, HypothesisGenerated, Linkage, OrderSubmitted,
    Provenance, SourceKind,
};

fn sample_linkage() -> Linkage {
    Linkage {
        hypothesis_id: Some("hyp-1".into()),
        signal_id: Some("sig-1".into()),
        decision_id: Some("dec-1".into()),
        order_id: Some("ord-1".into()),
        position_id: Some("pos-1".into()),
        parent_event_id: Some("evt-parent".into()),
        correlation_id: Some("corr-1".into()),
    }
}

fn sample_provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Research,
        source_ref: Some("report://alpha".into()),
        producer_run_id: Some("run-1".into()),
        actor: Some("researcher".into()),
        trace_id: Some("trace-1".into()),
        notes: Some("seed".into()),
    }
}

#[test]
fn hypothesis_roundtrip_json() {
    let event = EventEnvelope::new_hypothesis_generated(
        "research-engine",
        Some("BTCUSDT".into()),
        sample_linkage(),
        sample_provenance(),
        HypothesisGenerated {
            hypothesis_id: "hyp-1".into(),
            instrument: "BTCUSDT".into(),
            timeframe: "1h".into(),
            thesis: "Breakout continuation".into(),
            direction_hint: Some("up".into()),
            confidence: Some(0.72),
        },
    )
    .unwrap();

    let json = serde_json::to_string(&event).unwrap();
    let decoded: EventEnvelope<HypothesisGenerated> = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, event);
    assert!(json.contains("\"event_type\":\"hypothesis.generated\""));
    assert!(json.contains("\"linkage\""));
    assert!(json.contains("\"provenance\""));
}

#[test]
fn fill_roundtrip_json() {
    let event = EventEnvelope::new_fill_received(
        "execution-gateway",
        Some("BTCUSDT".into()),
        sample_linkage(),
        Provenance {
            source_kind: SourceKind::ExecutionVenue,
            source_ref: Some("binance".into()),
            producer_run_id: Some("run-2".into()),
            actor: Some("matching-engine".into()),
            trace_id: Some("trace-2".into()),
            notes: Some("fill callback".into()),
        },
        FillReceived {
            fill_id: "fill-1".into(),
            decision_id: Some("dec-1".into()),
            order_id: "ord-1".into(),
            instrument: "BTCUSDT".into(),
            side: FillSide::Buy,
            quantity: 1.5,
            price: 62000.5,
            venue: "binance".into(),
            executed_at: Utc::now(),
        },
    )
    .unwrap();

    let value = serde_json::to_value(&event).unwrap();
    let decoded: EventEnvelope<FillReceived> = serde_json::from_value(value.clone()).unwrap();

    assert_eq!(decoded, event);
    assert_eq!(value["event_type"], "fill.received");
    assert_eq!(value["provenance"]["source_kind"], "ExecutionVenue");
    assert_eq!(value["linkage"]["correlation_id"], "corr-1");
}

#[test]
fn order_submitted_roundtrip_json() {
    let event = EventEnvelope::new_order_submitted(
        "runtime",
        Some("BTCUSDT".into()),
        sample_linkage(),
        sample_provenance(),
        OrderSubmitted {
            order_id: "ord-1".into(),
            decision_id: Some("dec-1".into()),
            instrument: "BTCUSDT".into(),
            venue: "binance".into(),
        },
    )
    .unwrap();

    let value = serde_json::to_value(&event).unwrap();
    let decoded: EventEnvelope<OrderSubmitted> = serde_json::from_value(value.clone()).unwrap();

    assert_eq!(decoded, event);
    assert_eq!(value["event_type"], "order.submitted");
}
