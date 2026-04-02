use chrono::Utc;
use twoexcamim::events::{
    EventEnvelope, FillReceived, FillSide, Linkage, Provenance, SignalConfirmed, SourceKind,
};

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Runtime,
        source_ref: Some("runtime://confirm".into()),
        producer_run_id: Some("run-9".into()),
        actor: Some("engine".into()),
        trace_id: Some("trace-9".into()),
        notes: None,
    }
}

#[test]
fn signal_confirmed_idempotency_is_deterministic() {
    let event_a = EventEnvelope::new_signal_confirmed(
        "runtime",
        None,
        Linkage::default(),
        provenance(),
        SignalConfirmed {
            signal_id: "sig-42".into(),
            confirmed_by: "risk-engine".into(),
            confirmation_reason: Some("threshold met".into()),
            confirmation_score: Some(0.91),
        },
    )
    .unwrap();

    let event_b = EventEnvelope::new_signal_confirmed(
        "runtime",
        None,
        Linkage::default(),
        provenance(),
        SignalConfirmed {
            signal_id: "sig-42".into(),
            confirmed_by: "risk-engine".into(),
            confirmation_reason: None,
            confirmation_score: Some(0.75),
        },
    )
    .unwrap();

    assert_eq!(
        event_a.idempotency_key,
        "signal.confirmed:v1:sig-42:risk-engine"
    );
    assert_eq!(event_a.idempotency_key, event_b.idempotency_key);
}

#[test]
fn fill_received_idempotency_is_deterministic() {
    let event = EventEnvelope::new_fill_received(
        "execution",
        Some("BTCUSDT".into()),
        Linkage {
            order_id: Some("ord-7".into()),
            ..Linkage::default()
        },
        Provenance {
            source_kind: SourceKind::ExecutionVenue,
            source_ref: Some("binance".into()),
            producer_run_id: Some("run-10".into()),
            actor: Some("venue".into()),
            trace_id: Some("trace-10".into()),
            notes: Some("websocket".into()),
        },
        FillReceived {
            fill_id: "fill-7".into(),
            decision_id: Some("dec-7".into()),
            order_id: "ord-7".into(),
            instrument: "BTCUSDT".into(),
            side: FillSide::Sell,
            quantity: 2.0,
            price: 61000.0,
            venue: "binance".into(),
            executed_at: Utc::now(),
        },
    )
    .unwrap();

    assert_eq!(
        event.idempotency_key,
        "fill.received:v1:binance:ord-7:fill-7"
    );
}
