use chrono::Utc;
use twoexcamim::events::{
    EventEnvelope, EventError, FillReceived, FillSide, HypothesisGenerated, Linkage, Provenance,
    SignalGenerated, SignalSide, SourceKind, VetoRaised, VetoScope,
};
use uuid::Uuid;

fn linkage() -> Linkage {
    Linkage::default()
}

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: None,
        producer_run_id: Some("run-1".into()),
        actor: None,
        trace_id: Some("trace-1".into()),
        notes: None,
    }
}

#[test]
fn valid_events_pass_validation() {
    let hypothesis = EventEnvelope::new_hypothesis_generated(
        "research",
        None,
        linkage(),
        provenance(),
        HypothesisGenerated {
            hypothesis_id: "hyp-1".into(),
            instrument: "ETHUSDT".into(),
            timeframe: "15m".into(),
            thesis: "Mean reversion".into(),
            direction_hint: None,
            confidence: Some(0.5),
        },
    )
    .unwrap();

    let signal = EventEnvelope::new_signal_generated(
        "runtime",
        None,
        linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "ETHUSDT".into(),
            timeframe: "15m".into(),
            side: SignalSide::Long,
            strength: 0.8,
            rationale: Some("confirmed divergence".into()),
        },
    )
    .unwrap();

    assert!(hypothesis.validate().is_ok());
    assert!(signal.validate().is_ok());
}

#[test]
fn signal_generated_rejects_strength_above_one() {
    let err = EventEnvelope::new_signal_generated(
        "runtime",
        None,
        linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: None,
            instrument: "ETHUSDT".into(),
            timeframe: "15m".into(),
            side: SignalSide::Short,
            strength: 1.1,
            rationale: None,
        },
    )
    .unwrap_err();

    assert!(matches!(err, EventError::ValidationError(message) if message.contains("strength")));
}

#[test]
fn hypothesis_generated_rejects_negative_confidence() {
    let err = EventEnvelope::new_hypothesis_generated(
        "research",
        None,
        linkage(),
        provenance(),
        HypothesisGenerated {
            hypothesis_id: "hyp-1".into(),
            instrument: "ETHUSDT".into(),
            timeframe: "15m".into(),
            thesis: "Mean reversion".into(),
            direction_hint: None,
            confidence: Some(-0.1),
        },
    )
    .unwrap_err();

    assert!(matches!(err, EventError::ValidationError(message) if message.contains("confidence")));
}

#[test]
fn veto_raised_rejects_empty_reason_code() {
    let err = EventEnvelope::new_veto_raised(
        "risk",
        None,
        linkage(),
        provenance(),
        VetoRaised {
            veto_id: "veto-1".into(),
            scope: VetoScope::Signal,
            target_id: "sig-1".into(),
            reason_code: "".into(),
            reason_text: None,
            raised_by: "risk-engine".into(),
        },
    )
    .unwrap_err();

    assert!(matches!(err, EventError::ValidationError(message) if message.contains("reason_code")));
}

#[test]
fn fill_received_rejects_non_positive_quantity_and_price() {
    let quantity_err = EventEnvelope::new_fill_received(
        "execution",
        None,
        linkage(),
        provenance(),
        FillReceived {
            fill_id: "fill-1".into(),
            decision_id: None,
            order_id: "ord-1".into(),
            instrument: "ETHUSDT".into(),
            side: FillSide::Buy,
            quantity: 0.0,
            price: 100.0,
            venue: "binance".into(),
            executed_at: Utc::now(),
        },
    )
    .unwrap_err();

    let price_err = EventEnvelope::new_fill_received(
        "execution",
        None,
        linkage(),
        provenance(),
        FillReceived {
            fill_id: "fill-1".into(),
            decision_id: None,
            order_id: "ord-1".into(),
            instrument: "ETHUSDT".into(),
            side: FillSide::Buy,
            quantity: 1.0,
            price: 0.0,
            venue: "binance".into(),
            executed_at: Utc::now(),
        },
    )
    .unwrap_err();

    assert!(
        matches!(quantity_err, EventError::ValidationError(message) if message.contains("quantity"))
    );
    assert!(matches!(price_err, EventError::ValidationError(message) if message.contains("price")));
}

#[test]
fn envelope_rejects_blank_optional_fields_when_present() {
    let err = EventEnvelope::new_hypothesis_generated(
        "research",
        Some("   ".into()),
        Linkage {
            correlation_id: Some("".into()),
            ..linkage()
        },
        provenance(),
        HypothesisGenerated {
            hypothesis_id: "hyp-1".into(),
            instrument: "ETHUSDT".into(),
            timeframe: "15m".into(),
            thesis: "Mean reversion".into(),
            direction_hint: None,
            confidence: Some(0.5),
        },
    )
    .unwrap_err();

    assert!(
        matches!(err, EventError::ValidationError(message) if message.contains("aggregate_key") || message.contains("linkage.correlation_id"))
    );
}

#[test]
fn envelope_rejects_tampered_payload_idempotency_key() {
    let mut event = EventEnvelope::new_signal_generated(
        "runtime",
        None,
        linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "ETHUSDT".into(),
            timeframe: "15m".into(),
            side: SignalSide::Long,
            strength: 0.8,
            rationale: Some("confirmed divergence".into()),
        },
    )
    .unwrap();

    event.event_id = Uuid::new_v4().to_string();
    event.idempotency_key = "signal.generated:v1:other".into();

    let err = event.validate().unwrap_err();
    assert!(
        matches!(err, EventError::InvariantError(message) if message.contains("idempotency_key"))
    );
}
