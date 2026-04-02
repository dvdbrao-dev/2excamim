use chrono::Utc;
use pretty_assertions::assert_eq;
use twoexcamim::codecs::{CodecError, RehydratedEvent};
use twoexcamim::events::{
    DecisionAction, DecisionFormed, EventEnvelope, EventError, FillReceived, FillSide,
    HypothesisGenerated, Linkage, Provenance, SignalGenerated, SignalSide, SourceKind,
};
use twoexcamim::store::StoredEvent;

fn linkage() -> Linkage {
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

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("codec://tests".into()),
        producer_run_id: Some("run-codec-1".into()),
        actor: Some("tests".into()),
        trace_id: Some("trace-codec-1".into()),
        notes: Some("codec coverage".into()),
    }
}

#[test]
fn rehydrates_hypothesis_generated() {
    let envelope = EventEnvelope::new_hypothesis_generated(
        "research-engine",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
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
    let stored = StoredEvent::try_from(&envelope).unwrap();

    let rehydrated = RehydratedEvent::try_from(stored).unwrap();

    assert_eq!(rehydrated, RehydratedEvent::HypothesisGenerated(envelope));
}

#[test]
fn rehydrates_signal_generated() {
    let envelope = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "BTCUSDT".into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength: 0.81,
            rationale: Some("momentum".into()),
        },
    )
    .unwrap();
    let stored = StoredEvent::try_from(&envelope).unwrap();

    let rehydrated = RehydratedEvent::try_from(stored).unwrap();

    assert_eq!(rehydrated, RehydratedEvent::SignalGenerated(envelope));
}

#[test]
fn rehydrates_decision_formed() {
    let envelope = EventEnvelope::new_decision_formed(
        "decision-engine",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
        DecisionFormed {
            decision_id: "dec-1".into(),
            instrument: "BTCUSDT".into(),
            action: DecisionAction::Enter,
            side: Some(SignalSide::Long),
            size_hint: Some(1.25),
            rationale: Some("follow signal".into()),
        },
    )
    .unwrap();
    let stored = StoredEvent::try_from(&envelope).unwrap();

    let rehydrated = RehydratedEvent::try_from(stored).unwrap();

    assert_eq!(rehydrated, RehydratedEvent::DecisionFormed(envelope));
}

#[test]
fn fails_when_event_type_and_payload_do_not_match() {
    let envelope = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "BTCUSDT".into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength: 0.81,
            rationale: Some("momentum".into()),
        },
    )
    .unwrap();
    let mut stored = StoredEvent::try_from(envelope).unwrap();
    stored.event_type = twoexcamim::events::EventType::DecisionFormed;

    let err = RehydratedEvent::try_from(stored).unwrap_err();

    assert!(matches!(
        err,
        CodecError::PayloadDecode {
            event_type: twoexcamim::events::EventType::DecisionFormed,
            ..
        }
    ));
}

#[test]
fn roundtrip_envelope_to_stored_to_rehydrated() {
    let envelope = EventEnvelope::new_fill_received(
        "execution-gateway",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
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

    let stored = StoredEvent::try_from(&envelope).unwrap();
    let rehydrated = RehydratedEvent::try_from(stored.clone()).unwrap();
    let stored_again = StoredEvent::try_from(&rehydrated).unwrap();

    assert_eq!(stored_again, stored);
    assert_eq!(rehydrated, RehydratedEvent::FillReceived(envelope));
}

#[test]
fn preserves_linkage_provenance_and_idempotency_key() {
    let envelope = EventEnvelope::new_hypothesis_generated(
        "research-engine",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
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
    let stored = StoredEvent::try_from(&envelope).unwrap();

    let rehydrated = RehydratedEvent::try_from(stored).unwrap();

    match rehydrated {
        RehydratedEvent::HypothesisGenerated(event) => {
            assert_eq!(event.linkage, linkage());
            assert_eq!(event.provenance, provenance());
            assert_eq!(event.idempotency_key, "hypothesis.generated:v1:hyp-1");
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn fails_when_reconstructed_envelope_does_not_validate() {
    let envelope = EventEnvelope::new_signal_generated(
        "signal-engine",
        Some("BTCUSDT".into()),
        linkage(),
        provenance(),
        SignalGenerated {
            signal_id: "sig-1".into(),
            hypothesis_id: Some("hyp-1".into()),
            instrument: "BTCUSDT".into(),
            timeframe: "1h".into(),
            side: SignalSide::Long,
            strength: 0.81,
            rationale: Some("momentum".into()),
        },
    )
    .unwrap();
    let mut stored = StoredEvent::try_from(envelope).unwrap();
    stored.idempotency_key = "signal.generated:v1:other".into();

    let err = RehydratedEvent::try_from(stored).unwrap_err();

    assert!(matches!(
        err,
        CodecError::Validation(EventError::InvariantError(message))
        if message.contains("idempotency_key")
    ));
}
