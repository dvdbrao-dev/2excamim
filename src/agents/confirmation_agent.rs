use chrono::{DateTime, Duration, Utc};
use market_domain::MarketSignal;

use crate::events::{EventEnvelope, Linkage, Provenance, SignalConfirmed, SourceKind};

const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.6;
const DEFAULT_MAX_AGE_SECONDS: i64 = 300;
const DEFAULT_PRODUCED_BY: &str = "confirmation-agent-v1";
const DEFAULT_SOURCE_REF: &str = "agent://confirmation/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationAgentConfig {
    pub confidence_threshold: f64,
    pub max_age_seconds: i64,
    pub produced_by: String,
    pub confirmed_by: String,
    pub evaluation_time: Option<DateTime<Utc>>,
}

impl Default for ConfirmationAgentConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
            produced_by: DEFAULT_PRODUCED_BY.to_string(),
            confirmed_by: DEFAULT_PRODUCED_BY.to_string(),
            evaluation_time: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationAgent {
    config: ConfirmationAgentConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    SignalConfirmed(EventEnvelope<SignalConfirmed>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationDecision {
    Accepted,
    RejectedLowConfidence,
    RejectedStale,
}

impl ConfirmationAgent {
    pub fn new(config: ConfirmationAgentConfig) -> Self {
        assert!(
            (0.0..=1.0).contains(&config.confidence_threshold),
            "confidence_threshold must be in [0,1]"
        );
        assert!(config.max_age_seconds >= 0, "max_age_seconds must be >= 0");
        assert!(
            !config.produced_by.trim().is_empty(),
            "produced_by cannot be empty"
        );
        assert!(
            !config.confirmed_by.trim().is_empty(),
            "confirmed_by cannot be empty"
        );

        Self { config }
    }

    pub fn config(&self) -> &ConfirmationAgentConfig {
        &self.config
    }

    pub fn with_confidence_threshold(&self, confidence_threshold: f64) -> Self {
        let mut config = self.config.clone();
        config.confidence_threshold = confidence_threshold;
        Self::new(config)
    }

    pub fn evaluate(&self, signal: &MarketSignal) -> Option<CoreEvent> {
        self.evaluate_decision(signal).event
    }

    pub fn evaluate_decision(&self, signal: &MarketSignal) -> EvaluationOutcome {
        let evaluation_time = self.config.evaluation_time.unwrap_or_else(Utc::now);

        if signal.confidence < self.config.confidence_threshold {
            return EvaluationOutcome {
                decision: ConfirmationDecision::RejectedLowConfidence,
                event: None,
            };
        }

        let max_age = Duration::seconds(self.config.max_age_seconds);
        if evaluation_time.signed_duration_since(signal.generated_at) > max_age {
            return EvaluationOutcome {
                decision: ConfirmationDecision::RejectedStale,
                event: None,
            };
        }

        let payload = SignalConfirmed {
            signal_id: signal.signal_id.clone(),
            confirmed_by: self.config.confirmed_by.clone(),
            confirmation_reason: Some(self.build_rationale(signal, evaluation_time)),
            confirmation_score: Some(signal.confidence),
        };

        let event = EventEnvelope::new_signal_confirmed(
            self.config.produced_by.clone(),
            Some(signal.market_id.clone()),
            Linkage {
                signal_id: Some(signal.signal_id.clone()),
                ..Linkage::default()
            },
            Provenance {
                source_kind: SourceKind::Derived,
                source_ref: Some(DEFAULT_SOURCE_REF.to_string()),
                producer_run_id: None,
                actor: Some(self.config.produced_by.clone()),
                trace_id: None,
                notes: None,
            },
            payload,
        )
        .expect("confirmation agent builds valid signal.confirmed events");

        EvaluationOutcome {
            decision: ConfirmationDecision::Accepted,
            event: Some(CoreEvent::SignalConfirmed(event)),
        }
    }

    fn build_rationale(&self, signal: &MarketSignal, evaluation_time: DateTime<Utc>) -> String {
        let age_seconds = evaluation_time
            .signed_duration_since(signal.generated_at)
            .num_seconds()
            .max(0);

        match signal.rationale.as_deref() {
            Some(rationale) => format!(
                "accepted signal={} confidence={:.3} threshold={:.3} age_seconds={} max_age_seconds={} rationale={}",
                signal.signal_name,
                signal.confidence,
                self.config.confidence_threshold,
                age_seconds,
                self.config.max_age_seconds,
                rationale
            ),
            None => format!(
                "accepted signal={} confidence={:.3} threshold={:.3} age_seconds={} max_age_seconds={}",
                signal.signal_name,
                signal.confidence,
                self.config.confidence_threshold,
                age_seconds,
                self.config.max_age_seconds
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationOutcome {
    pub decision: ConfirmationDecision,
    pub event: Option<CoreEvent>,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use market_domain::{MarketSignal, MarketSignalDirection, MarketSource};

    use super::{ConfirmationAgent, ConfirmationAgentConfig, CoreEvent};

    fn signal(confidence: f64, generated_at: DateTime<Utc>) -> MarketSignal {
        let mut signal = MarketSignal::new(
            "signal-1",
            "market-1",
            MarketSource::Synthetic,
            "odds_jump",
            MarketSignalDirection::Yes,
            confidence,
            generated_at,
        )
        .unwrap();
        signal.rationale = Some("detector matched".into());
        signal
    }

    #[test]
    fn accepts_signal_that_meets_threshold_and_age_rules() {
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();
        let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
            evaluation_time: Some(evaluation_time),
            ..ConfirmationAgentConfig::default()
        });

        let event = agent.evaluate(&signal(
            0.81,
            evaluation_time - chrono::Duration::seconds(60),
        ));

        match event {
            Some(CoreEvent::SignalConfirmed(event)) => {
                assert_eq!(event.aggregate_key.as_deref(), Some("market-1"));
                assert_eq!(event.linkage.signal_id.as_deref(), Some("signal-1"));
                assert_eq!(event.payload.signal_id, "signal-1");
                assert_eq!(event.payload.confirmation_score, Some(0.81));
                assert_eq!(
                    event.idempotency_key,
                    "signal.confirmed:v1:signal-1:confirmation-agent-v1"
                );
                assert!(event
                    .payload
                    .confirmation_reason
                    .as_deref()
                    .unwrap()
                    .contains("accepted signal=odds_jump"));
            }
            None => panic!("expected signal.confirmed event"),
        }
    }

    #[test]
    fn rejects_signal_below_confidence_threshold() {
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();
        let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
            evaluation_time: Some(evaluation_time),
            ..ConfirmationAgentConfig::default()
        });

        let event = agent.evaluate(&signal(
            0.59,
            evaluation_time - chrono::Duration::seconds(30),
        ));

        assert!(event.is_none());
    }

    #[test]
    fn rejects_signal_older_than_max_age() {
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();
        let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
            max_age_seconds: 120,
            evaluation_time: Some(evaluation_time),
            ..ConfirmationAgentConfig::default()
        });

        let event = agent.evaluate(&signal(
            0.9,
            evaluation_time - chrono::Duration::seconds(121),
        ));

        assert!(event.is_none());
    }

    #[test]
    fn repeated_evaluation_is_deterministic_at_payload_level() {
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();
        let agent = ConfirmationAgent::new(ConfirmationAgentConfig {
            evaluation_time: Some(evaluation_time),
            ..ConfirmationAgentConfig::default()
        });
        let signal = signal(0.77, evaluation_time - chrono::Duration::seconds(45));

        let first = agent.evaluate(&signal);
        let second = agent.evaluate(&signal);

        match (first, second) {
            (Some(CoreEvent::SignalConfirmed(first)), Some(CoreEvent::SignalConfirmed(second))) => {
                assert_eq!(first.event_type, second.event_type);
                assert_eq!(first.aggregate_key, second.aggregate_key);
                assert_eq!(first.idempotency_key, second.idempotency_key);
                assert_eq!(first.linkage, second.linkage);
                assert_eq!(first.provenance, second.provenance);
                assert_eq!(first.payload, second.payload);
            }
            _ => panic!("expected deterministic signal.confirmed events"),
        }
    }
}
