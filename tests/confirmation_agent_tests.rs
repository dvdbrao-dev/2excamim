use chrono::{Duration, TimeZone, Utc};
use market_domain::{MarketSignal, MarketSignalDirection, MarketSource};
use twoexcamim::{ConfirmationAgent, ConfirmationAgentConfig, CoreEvent};

fn signal(confidence: f64, generated_at: chrono::DateTime<Utc>) -> MarketSignal {
    let mut signal = MarketSignal::new(
        "signal-42",
        "market-42",
        MarketSource::Synthetic,
        "activity_spike",
        MarketSignalDirection::Yes,
        confidence,
        generated_at,
    )
    .unwrap();
    signal.rationale = Some("volume expansion".into());
    signal
}

#[test]
fn accept_valid_signal() {
    let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 15, 0, 0).unwrap();
    let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
        evaluation_time: Some(evaluation_time),
        ..ConfirmationAgentConfig::default()
    });

    let event = agent.evaluate(&signal(0.75, evaluation_time - Duration::seconds(90)));

    match event {
        Some(CoreEvent::SignalConfirmed(event)) => {
            assert_eq!(event.produced_by, "confirmation-agent-v1");
            assert_eq!(event.aggregate_key.as_deref(), Some("market-42"));
            assert_eq!(event.payload.signal_id, "signal-42");
            assert_eq!(event.payload.confirmed_by, "confirmation-agent-v1");
            assert_eq!(event.payload.confirmation_score, Some(0.75));
            assert!(event.validate().is_ok());
        }
        None => panic!("expected confirmed signal"),
    }
}

#[test]
fn reject_low_confidence() {
    let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 15, 0, 0).unwrap();
    let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
        confidence_threshold: 0.8,
        evaluation_time: Some(evaluation_time),
        ..ConfirmationAgentConfig::default()
    });

    let event = agent.evaluate(&signal(0.79, evaluation_time - Duration::seconds(30)));

    assert!(event.is_none());
}

#[test]
fn reject_old_signal() {
    let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 15, 0, 0).unwrap();
    let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
        max_age_seconds: 60,
        evaluation_time: Some(evaluation_time),
        ..ConfirmationAgentConfig::default()
    });

    let event = agent.evaluate(&signal(0.92, evaluation_time - Duration::seconds(61)));

    assert!(event.is_none());
}

#[test]
fn deterministic_behavior_for_same_signal_and_config() {
    let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 15, 0, 0).unwrap();
    let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
        evaluation_time: Some(evaluation_time),
        ..ConfirmationAgentConfig::default()
    });
    let signal = signal(0.92, evaluation_time - Duration::seconds(10));

    let first = agent.evaluate(&signal);
    let second = agent.evaluate(&signal);

    match (first, second) {
        (Some(CoreEvent::SignalConfirmed(first)), Some(CoreEvent::SignalConfirmed(second))) => {
            assert_eq!(first.event_type, second.event_type);
            assert_eq!(first.idempotency_key, second.idempotency_key);
            assert_eq!(first.aggregate_key, second.aggregate_key);
            assert_eq!(first.linkage, second.linkage);
            assert_eq!(first.payload, second.payload);
        }
        _ => panic!("expected confirmed events"),
    }
}
