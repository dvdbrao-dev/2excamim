use pretty_assertions::assert_eq;
use twoexcamim::commands::{
    CommandError, ConfirmSignalCommand, FormDecisionCommand, GenerateSignalCommand,
    RegisterOrderCommand, SubmitOrderCommand,
};
use twoexcamim::events::{DecisionAction, EventType, Provenance, SignalSide, SourceKind};

fn provenance() -> Provenance {
    Provenance {
        source_kind: SourceKind::Derived,
        source_ref: Some("commands://tests".into()),
        producer_run_id: Some("run-cmd-1".into()),
        actor: Some("command-tests".into()),
        trace_id: Some("trace-cmd-1".into()),
        notes: None,
    }
}

#[test]
fn generate_signal_produces_valid_signal_generated_event() {
    let event = GenerateSignalCommand {
        produced_by: "signal-engine".into(),
        provenance: provenance(),
        signal_id: "sig-101".into(),
        hypothesis_id: Some("hyp-101".into()),
        instrument: "BTCUSDT".into(),
        timeframe: "1h".into(),
        side: SignalSide::Long,
        strength: 0.81,
        rationale: Some("momentum confirmed".into()),
        parent_event_id: Some("evt-parent-1".into()),
        correlation_id: Some("corr-101".into()),
    }
    .execute()
    .unwrap();

    assert_eq!(event.event_type, EventType::SignalGenerated);
    assert_eq!(event.produced_by, "signal-engine");
    assert_eq!(event.aggregate_key.as_deref(), Some("BTCUSDT"));
    assert_eq!(event.idempotency_key, "signal.generated:v1:sig-101");
    assert_eq!(event.linkage.hypothesis_id.as_deref(), Some("hyp-101"));
    assert_eq!(event.linkage.signal_id.as_deref(), Some("sig-101"));
    assert_eq!(event.payload.signal_id, "sig-101");
    assert_eq!(event.payload.hypothesis_id.as_deref(), Some("hyp-101"));
    assert_eq!(event.payload.instrument, "BTCUSDT");
    assert!(event.validate().is_ok());
}

#[test]
fn confirm_signal_produces_valid_signal_confirmed_event() {
    let event = ConfirmSignalCommand {
        produced_by: "confirmation-engine".into(),
        provenance: provenance(),
        aggregate_key: Some("BTCUSDT".into()),
        signal_id: "sig-201".into(),
        hypothesis_id: Some("hyp-201".into()),
        confirmed_by: "risk-engine".into(),
        confirmation_reason: Some("risk checks passed".into()),
        confirmation_score: Some(0.93),
        parent_event_id: Some("evt-signal-201".into()),
        correlation_id: Some("corr-201".into()),
    }
    .execute()
    .unwrap();

    assert_eq!(event.event_type, EventType::SignalConfirmed);
    assert_eq!(event.produced_by, "confirmation-engine");
    assert_eq!(event.aggregate_key.as_deref(), Some("BTCUSDT"));
    assert_eq!(
        event.idempotency_key,
        "signal.confirmed:v1:sig-201:risk-engine"
    );
    assert_eq!(event.linkage.signal_id.as_deref(), Some("sig-201"));
    assert_eq!(event.payload.confirmed_by, "risk-engine");
    assert_eq!(event.payload.confirmation_score, Some(0.93));
    assert!(event.validate().is_ok());
}

#[test]
fn form_decision_produces_valid_decision_formed_event() {
    let event = FormDecisionCommand {
        produced_by: "decision-engine".into(),
        provenance: provenance(),
        decision_id: "dec-301".into(),
        hypothesis_id: Some("hyp-301".into()),
        signal_id: Some("sig-301".into()),
        instrument: "ETHUSDT".into(),
        action: DecisionAction::Enter,
        side: Some(SignalSide::Short),
        size_hint: Some(2.5),
        rationale: Some("entry approved".into()),
        parent_event_id: Some("evt-confirmed-301".into()),
        correlation_id: Some("corr-301".into()),
    }
    .execute()
    .unwrap();

    assert_eq!(event.event_type, EventType::DecisionFormed);
    assert_eq!(event.produced_by, "decision-engine");
    assert_eq!(event.aggregate_key.as_deref(), Some("ETHUSDT"));
    assert_eq!(event.idempotency_key, "decision.formed:v1:dec-301");
    assert_eq!(event.linkage.decision_id.as_deref(), Some("dec-301"));
    assert_eq!(event.linkage.signal_id.as_deref(), Some("sig-301"));
    assert_eq!(event.payload.action, DecisionAction::Enter);
    assert_eq!(event.payload.side, Some(SignalSide::Short));
    assert!(event.validate().is_ok());
}

#[test]
fn register_order_produces_valid_order_registered_event() {
    let event = RegisterOrderCommand {
        produced_by: "order-engine".into(),
        provenance: provenance(),
        order_id: "ord-401".into(),
        decision_id: Some("dec-401".into()),
        hypothesis_id: Some("hyp-401".into()),
        signal_id: Some("sig-401".into()),
        instrument: "BTCUSDT".into(),
        venue: "paper".into(),
        parent_event_id: Some("evt-decision-401".into()),
        correlation_id: Some("corr-401".into()),
    }
    .execute()
    .unwrap();

    assert_eq!(event.event_type, EventType::OrderRegistered);
    assert_eq!(event.idempotency_key, "order.registered:v1:paper:ord-401");
    assert_eq!(event.linkage.order_id.as_deref(), Some("ord-401"));
    assert_eq!(event.linkage.decision_id.as_deref(), Some("dec-401"));
    assert!(event.validate().is_ok());
}

#[test]
fn submit_order_produces_valid_order_submitted_event() {
    let event = SubmitOrderCommand {
        produced_by: "submission-engine".into(),
        provenance: provenance(),
        order_id: "ord-402".into(),
        decision_id: Some("dec-402".into()),
        hypothesis_id: Some("hyp-402".into()),
        signal_id: Some("sig-402".into()),
        instrument: "BTCUSDT".into(),
        venue: "paper".into(),
        parent_event_id: Some("evt-order-402".into()),
        correlation_id: Some("corr-402".into()),
    }
    .execute()
    .unwrap();

    assert_eq!(event.event_type, EventType::OrderSubmitted);
    assert_eq!(event.idempotency_key, "order.submitted:v1:paper:ord-402");
    assert_eq!(event.linkage.order_id.as_deref(), Some("ord-402"));
    assert_eq!(event.linkage.decision_id.as_deref(), Some("dec-402"));
    assert!(event.validate().is_ok());
}

#[test]
fn generate_signal_returns_validation_error_for_invalid_input() {
    let error = GenerateSignalCommand {
        produced_by: "signal-engine".into(),
        provenance: provenance(),
        signal_id: "".into(),
        hypothesis_id: None,
        instrument: "BTCUSDT".into(),
        timeframe: "1h".into(),
        side: SignalSide::Long,
        strength: 0.8,
        rationale: None,
        parent_event_id: None,
        correlation_id: None,
    }
    .execute()
    .unwrap_err();

    assert!(matches!(error, CommandError::Validation(message) if message.contains("signal_id")));
}

#[test]
fn confirm_signal_returns_validation_error_for_invalid_input() {
    let error = ConfirmSignalCommand {
        produced_by: "confirmation-engine".into(),
        provenance: provenance(),
        aggregate_key: Some("BTCUSDT".into()),
        signal_id: "sig-202".into(),
        hypothesis_id: None,
        confirmed_by: "".into(),
        confirmation_reason: None,
        confirmation_score: Some(0.5),
        parent_event_id: None,
        correlation_id: None,
    }
    .execute()
    .unwrap_err();

    assert!(matches!(error, CommandError::Validation(message) if message.contains("confirmed_by")));
}

#[test]
fn form_decision_returns_validation_error_for_invalid_input() {
    let error = FormDecisionCommand {
        produced_by: "decision-engine".into(),
        provenance: provenance(),
        decision_id: "dec-302".into(),
        hypothesis_id: None,
        signal_id: None,
        instrument: "ETHUSDT".into(),
        action: DecisionAction::Hold,
        side: None,
        size_hint: Some(0.0),
        rationale: None,
        parent_event_id: None,
        correlation_id: None,
    }
    .execute()
    .unwrap_err();

    assert!(matches!(error, CommandError::Validation(message) if message.contains("size_hint")));
}

#[test]
fn submit_order_returns_validation_error_for_invalid_input() {
    let error = SubmitOrderCommand {
        produced_by: "submission-engine".into(),
        provenance: provenance(),
        order_id: "ord-403".into(),
        decision_id: Some("dec-403".into()),
        hypothesis_id: None,
        signal_id: None,
        instrument: "BTCUSDT".into(),
        venue: "".into(),
        parent_event_id: None,
        correlation_id: None,
    }
    .execute()
    .unwrap_err();

    assert!(matches!(error, CommandError::Validation(message) if message.contains("venue")));
}
