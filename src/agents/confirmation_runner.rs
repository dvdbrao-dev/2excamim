use chrono::{DateTime, Utc};
use market_domain::{MarketSignal, MarketSignalDirection, MarketSource};

use crate::{
    codecs::RehydratedEvent,
    store::{JsonlEventStore, StoreError, StoredEvent},
    ConfirmationAgent, ConfirmationDecision, ConfirmationPolicy, CoreEvent, SignalPolicyStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationDisposition {
    Accepted,
    RejectedLowConfidence,
    RejectedStale,
    SkippedFrozen,
    SkippedAlreadyConfirmed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationRunItem {
    pub signal_id: String,
    pub disposition: ConfirmationDisposition,
    pub persisted: bool,
    pub market_id: String,
    pub direction: MarketSignalDirection,
    pub source: MarketSource,
    pub signal_name: String,
    pub applied_confidence_threshold: Option<f64>,
    pub policy_status: Option<SignalPolicyStatus>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationRunReport {
    pub total_signals_processed: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub rejected_low_confidence: usize,
    pub rejected_stale: usize,
    pub skipped_frozen: usize,
    pub skipped_already_confirmed: usize,
    pub policy_overrides_used: usize,
    pub persisted: usize,
    pub duplicates: usize,
    pub items: Vec<ConfirmationRunItem>,
    pub emitted_events: Vec<StoredEvent>,
}

#[derive(Debug)]
pub struct ConfirmationRunner<'a> {
    store: &'a JsonlEventStore,
    agent: ConfirmationAgent,
    policy: Option<ConfirmationPolicy>,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingConfirmationSignal {
    already_confirmed: bool,
    market_signal: MarketSignal,
}

impl<'a> ConfirmationRunner<'a> {
    pub fn new(store: &'a JsonlEventStore, agent: ConfirmationAgent) -> Self {
        Self {
            store,
            agent,
            policy: None,
        }
    }

    pub fn with_policy(mut self, policy: ConfirmationPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn run(&self) -> Result<ConfirmationRunReport, StoreError> {
        let signals = self.load_all_signals()?;
        let agent = self.agent_for_signals(&signals);
        self.run_signals_with_agent(&signals, &agent)
    }

    pub fn load_recent_signals(&self) -> Result<Vec<MarketSignal>, StoreError> {
        Ok(self
            .load_all_signals()?
            .into_iter()
            .filter(|signal| !signal.already_confirmed)
            .map(|signal| signal.market_signal)
            .collect())
    }

    fn load_all_signals(&self) -> Result<Vec<PendingConfirmationSignal>, StoreError> {
        let events = self.store.read_all()?;
        let confirmed_signal_ids = events
            .iter()
            .filter_map(|event| match RehydratedEvent::try_from(event) {
                Ok(RehydratedEvent::SignalConfirmed(event)) => Some(event.payload.signal_id),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();

        let mut signals = events
            .into_iter()
            .filter_map(|event| match RehydratedEvent::try_from(event) {
                Ok(RehydratedEvent::SignalGenerated(event)) => Some(PendingConfirmationSignal {
                    already_confirmed: confirmed_signal_ids.contains(&event.payload.signal_id),
                    market_signal: stored_signal_to_market_signal(
                        event.payload.signal_id,
                        event.aggregate_key,
                        event.payload.instrument,
                        event.payload.timeframe,
                        event.payload.side,
                        event.payload.strength,
                        event.payload.rationale,
                        event.occurred_at,
                    ),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();

        signals.sort_by(|left, right| {
            left.market_signal
                .generated_at
                .cmp(&right.market_signal.generated_at)
                .then_with(|| {
                    left.market_signal
                        .signal_id
                        .cmp(&right.market_signal.signal_id)
                })
        });

        Ok(signals)
    }

    pub fn run_signals(
        &self,
        signals: &[MarketSignal],
    ) -> Result<ConfirmationRunReport, StoreError> {
        let wrapped = signals
            .iter()
            .cloned()
            .map(|market_signal| PendingConfirmationSignal {
                already_confirmed: false,
                market_signal,
            })
            .collect::<Vec<_>>();
        let agent = self.agent_for_signals(&wrapped);
        self.run_signals_with_agent(&wrapped, &agent)
    }

    fn run_signals_with_agent(
        &self,
        signals: &[PendingConfirmationSignal],
        agent: &ConfirmationAgent,
    ) -> Result<ConfirmationRunReport, StoreError> {
        let mut items = Vec::with_capacity(signals.len());
        let mut emitted_events = Vec::new();
        let mut accepted = 0usize;
        let mut rejected_low_confidence = 0usize;
        let mut rejected_stale = 0usize;
        let mut skipped_frozen = 0usize;
        let mut skipped_already_confirmed = 0usize;
        let mut policy_overrides_used = 0usize;
        let mut persisted = 0usize;
        let mut duplicates = 0usize;

        for signal in signals {
            if signal.already_confirmed {
                skipped_already_confirmed += 1;
                items.push(ConfirmationRunItem {
                    signal_id: signal.market_signal.signal_id.clone(),
                    disposition: ConfirmationDisposition::SkippedAlreadyConfirmed,
                    persisted: false,
                    market_id: signal.market_signal.market_id.clone(),
                    direction: signal.market_signal.direction,
                    source: signal.market_signal.source,
                    signal_name: signal.market_signal.signal_name.clone(),
                    applied_confidence_threshold: None,
                    policy_status: None,
                });
                continue;
            }

            let matching_rule = self
                .policy
                .as_ref()
                .and_then(|policy| policy.matching_rule(&signal.market_signal));

            if matching_rule.is_some_and(|rule| rule.status == SignalPolicyStatus::Frozen) {
                skipped_frozen += 1;
                items.push(ConfirmationRunItem {
                    signal_id: signal.market_signal.signal_id.clone(),
                    disposition: ConfirmationDisposition::SkippedFrozen,
                    persisted: false,
                    market_id: signal.market_signal.market_id.clone(),
                    direction: signal.market_signal.direction,
                    source: signal.market_signal.source,
                    signal_name: signal.market_signal.signal_name.clone(),
                    applied_confidence_threshold: None,
                    policy_status: Some(SignalPolicyStatus::Frozen),
                });
                continue;
            }

            let applied_confidence_threshold =
                matching_rule.and_then(|rule| rule.confidence_threshold);
            let effective_agent = applied_confidence_threshold
                .map(|threshold| agent.with_confidence_threshold(threshold))
                .unwrap_or_else(|| agent.clone());
            if applied_confidence_threshold.is_some() {
                policy_overrides_used += 1;
            }

            let outcome = effective_agent.evaluate_decision(&signal.market_signal);
            match outcome.event {
                Some(CoreEvent::SignalConfirmed(event)) => {
                    accepted += 1;
                    let stored = StoredEvent::try_from(&event)?;
                    let was_persisted = self.store.append_event(&stored)?;
                    if was_persisted {
                        persisted += 1;
                    } else {
                        duplicates += 1;
                    }
                    emitted_events.push(stored);
                    items.push(ConfirmationRunItem {
                        signal_id: signal.market_signal.signal_id.clone(),
                        disposition: ConfirmationDisposition::Accepted,
                        persisted: was_persisted,
                        market_id: signal.market_signal.market_id.clone(),
                        direction: signal.market_signal.direction,
                        source: signal.market_signal.source,
                        signal_name: signal.market_signal.signal_name.clone(),
                        applied_confidence_threshold,
                        policy_status: matching_rule.map(|rule| rule.status),
                    });
                }
                None => {
                    let disposition = match outcome.decision {
                        ConfirmationDecision::Accepted => ConfirmationDisposition::Accepted,
                        ConfirmationDecision::RejectedLowConfidence => {
                            rejected_low_confidence += 1;
                            ConfirmationDisposition::RejectedLowConfidence
                        }
                        ConfirmationDecision::RejectedStale => {
                            rejected_stale += 1;
                            ConfirmationDisposition::RejectedStale
                        }
                    };

                    items.push(ConfirmationRunItem {
                        signal_id: signal.market_signal.signal_id.clone(),
                        disposition,
                        persisted: false,
                        market_id: signal.market_signal.market_id.clone(),
                        direction: signal.market_signal.direction,
                        source: signal.market_signal.source,
                        signal_name: signal.market_signal.signal_name.clone(),
                        applied_confidence_threshold,
                        policy_status: matching_rule.map(|rule| rule.status),
                    });
                }
            }
        }

        Ok(ConfirmationRunReport {
            total_signals_processed: signals.len(),
            accepted,
            rejected: rejected_low_confidence + rejected_stale,
            rejected_low_confidence,
            rejected_stale,
            skipped_frozen,
            skipped_already_confirmed,
            policy_overrides_used,
            persisted,
            duplicates,
            items,
            emitted_events,
        })
    }

    fn agent_for_signals(&self, signals: &[PendingConfirmationSignal]) -> ConfirmationAgent {
        if self.agent.config().evaluation_time.is_some() {
            return self.agent.clone();
        }

        let mut config = self.agent.config().clone();
        config.evaluation_time = signals
            .iter()
            .map(|signal| signal.market_signal.generated_at)
            .max();
        ConfirmationAgent::new(config)
    }
}

fn stored_signal_to_market_signal(
    signal_id: String,
    aggregate_key: Option<String>,
    instrument: String,
    timeframe: String,
    side: crate::events::SignalSide,
    strength: f64,
    rationale: Option<String>,
    generated_at: DateTime<Utc>,
) -> MarketSignal {
    MarketSignal {
        signal_id,
        market_id: aggregate_key.unwrap_or_else(|| instrument.clone()),
        source: MarketSource::Synthetic,
        signal_name: timeframe,
        direction: map_signal_side(side),
        confidence: strength,
        rationale,
        generated_at,
    }
}

fn map_signal_side(side: crate::events::SignalSide) -> MarketSignalDirection {
    match side {
        crate::events::SignalSide::Long => MarketSignalDirection::Yes,
        crate::events::SignalSide::Short => MarketSignalDirection::No,
        crate::events::SignalSide::Flat => MarketSignalDirection::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSource};

    use crate::{
        events::{EventEnvelope, Linkage, Provenance, SignalGenerated, SignalSide, SourceKind},
        store::JsonlEventStore,
        ConfirmationAgentConfig, ConfirmationDisposition, ConfirmationPolicy, SignalPolicyRule,
        SignalPolicyStatus,
    };

    use super::{stored_signal_to_market_signal, ConfirmationRunner};

    fn temp_store_path(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "twoexcamim-confirmation-runner-{name}-{nanos}.jsonl"
        ))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }

    fn provenance() -> Provenance {
        Provenance {
            source_kind: SourceKind::Derived,
            source_ref: Some("confirmation-runner://tests".into()),
            producer_run_id: Some("run-confirm-1".into()),
            actor: Some("tests".into()),
            trace_id: Some("trace-confirm-1".into()),
            notes: None,
        }
    }

    fn generated_signal(
        signal_id: &str,
        strength: f64,
        occurred_at: chrono::DateTime<Utc>,
    ) -> crate::store::StoredEvent {
        let mut event = EventEnvelope::new_signal_generated(
            "signal-engine",
            Some("market-1".into()),
            Linkage {
                signal_id: Some(signal_id.into()),
                correlation_id: Some(format!("corr-{signal_id}")),
                ..Linkage::default()
            },
            provenance(),
            SignalGenerated {
                signal_id: signal_id.into(),
                hypothesis_id: None,
                instrument: "market-1".into(),
                timeframe: "odds_jump".into(),
                side: SignalSide::Long,
                strength,
                rationale: Some("runner test".into()),
            },
        )
        .unwrap();
        event.occurred_at = occurred_at;
        crate::store::StoredEvent::try_from(event).unwrap()
    }

    #[test]
    fn runner_accepts_and_persists_only_eligible_signals() {
        let path = temp_store_path("accepts");
        let store = JsonlEventStore::new(&path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        store
            .append_events(&[
                generated_signal(
                    "sig-accepted",
                    0.81,
                    evaluation_time - Duration::seconds(30),
                ),
                generated_signal("sig-low", 0.51, evaluation_time - Duration::seconds(30)),
                generated_signal("sig-old", 0.91, evaluation_time - Duration::seconds(600)),
            ])
            .unwrap();

        let runner = ConfirmationRunner::new(
            &store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                max_age_seconds: 300,
                ..ConfirmationAgentConfig::default()
            }),
        );

        let report = runner.run().unwrap();
        let stored_events = store.read_all().unwrap();

        assert_eq!(report.total_signals_processed, 3);
        assert_eq!(report.accepted, 1);
        assert_eq!(report.rejected, 2);
        assert_eq!(report.rejected_low_confidence, 1);
        assert_eq!(report.rejected_stale, 1);
        assert_eq!(report.skipped_already_confirmed, 0);
        assert_eq!(report.persisted, 1);
        assert_eq!(report.duplicates, 0);
        let item_dispositions = report
            .items
            .iter()
            .map(|item| (item.signal_id.as_str(), item.disposition))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            item_dispositions.get("sig-accepted"),
            Some(&ConfirmationDisposition::Accepted)
        );
        assert_eq!(
            item_dispositions.get("sig-low"),
            Some(&ConfirmationDisposition::RejectedLowConfidence)
        );
        assert_eq!(
            item_dispositions.get("sig-old"),
            Some(&ConfirmationDisposition::RejectedStale)
        );
        assert_eq!(stored_events.len(), 4);
        assert!(stored_events
            .iter()
            .any(|event| event.event_type.as_str() == "signal.confirmed"));

        cleanup(&path);
    }

    #[test]
    fn runner_is_idempotent_when_reprocessing_same_store() {
        let path = temp_store_path("idempotent");
        let store = JsonlEventStore::new(&path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        store
            .append_event(&generated_signal(
                "sig-accepted",
                0.88,
                evaluation_time - Duration::seconds(10),
            ))
            .unwrap();

        let runner = ConfirmationRunner::new(
            &store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        );

        let first = runner.run().unwrap();
        let second = runner.run().unwrap();

        assert_eq!(first.total_signals_processed, 1);
        assert_eq!(first.accepted, 1);
        assert_eq!(first.persisted, 1);
        assert_eq!(second.total_signals_processed, 1);
        assert_eq!(second.accepted, 0);
        assert_eq!(second.skipped_already_confirmed, 1);
        assert_eq!(second.persisted, 0);
        assert_eq!(store.read_all().unwrap().len(), 2);

        cleanup(&path);
    }

    #[test]
    fn runner_is_deterministic_for_same_explicit_input() {
        let first_path = temp_store_path("deterministic-first");
        let second_path = temp_store_path("deterministic-second");
        let first_store = JsonlEventStore::new(&first_path).unwrap();
        let second_store = JsonlEventStore::new(&second_path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        let first_runner = ConfirmationRunner::new(
            &first_store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        );
        let second_runner = ConfirmationRunner::new(
            &second_store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        );
        let signals = vec![stored_signal_to_market_signal(
            "sig-1".into(),
            Some("market-1".into()),
            "market-1".into(),
            "activity_spike".into(),
            SignalSide::Long,
            0.83,
            Some("runner deterministic".into()),
            evaluation_time - Duration::seconds(20),
        )];

        let first = first_runner.run_signals(&signals).unwrap();
        let second = second_runner.run_signals(&signals).unwrap();

        assert_eq!(first.accepted, second.accepted);
        assert_eq!(first.rejected, second.rejected);
        assert_eq!(
            first.rejected_low_confidence,
            second.rejected_low_confidence
        );
        assert_eq!(first.rejected_stale, second.rejected_stale);
        assert_eq!(
            first.skipped_already_confirmed,
            second.skipped_already_confirmed
        );
        assert_eq!(
            first.total_signals_processed,
            second.total_signals_processed
        );
        assert_eq!(first.items, second.items);
        assert_eq!(first.emitted_events.len(), 1);
        assert_eq!(second.emitted_events.len(), 1);
        assert_eq!(
            first.emitted_events[0].idempotency_key,
            second.emitted_events[0].idempotency_key
        );
        assert_eq!(first.duplicates, 0);
        assert_eq!(second.duplicates, 0);

        cleanup(&first_path);
        cleanup(&second_path);
    }

    #[test]
    fn runner_classifies_already_confirmed_signal_as_skipped() {
        let path = temp_store_path("skipped");
        let store = JsonlEventStore::new(&path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        let generated = generated_signal(
            "sig-accepted",
            0.88,
            evaluation_time - Duration::seconds(10),
        );
        store.append_event(&generated).unwrap();
        let runner = ConfirmationRunner::new(
            &store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        );
        let first = runner.run().unwrap();
        let second = runner.run().unwrap();

        assert_eq!(first.accepted, 1);
        assert_eq!(second.accepted, 0);
        assert_eq!(second.skipped_already_confirmed, 1);
        assert_eq!(
            second.items[0].disposition,
            ConfirmationDisposition::SkippedAlreadyConfirmed
        );

        cleanup(&path);
    }

    #[test]
    fn frozen_signals_are_skipped_by_policy() {
        let path = temp_store_path("frozen-policy");
        let store = JsonlEventStore::new(&path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        store
            .append_event(&generated_signal(
                "sig-frozen",
                0.95,
                evaluation_time - Duration::seconds(10),
            ))
            .unwrap();

        let runner = ConfirmationRunner::new(
            &store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        )
        .with_policy(ConfirmationPolicy {
            metadata: None,
            rules: vec![SignalPolicyRule {
                signal_name: "odds_jump".into(),
                direction: Some(MarketSignalDirection::Yes),
                source: Some(MarketSource::Synthetic),
                status: SignalPolicyStatus::Frozen,
                confidence_threshold: None,
                horizon_seconds: None,
            }],
        });

        let report = runner.run().unwrap();

        assert_eq!(report.accepted, 0);
        assert_eq!(report.skipped_frozen, 1);
        assert_eq!(
            report.items[0].disposition,
            ConfirmationDisposition::SkippedFrozen
        );
        assert_eq!(store.read_all().unwrap().len(), 1);

        cleanup(&path);
    }

    #[test]
    fn promoted_signals_can_use_relaxed_thresholds() {
        let path = temp_store_path("promoted-policy");
        let store = JsonlEventStore::new(&path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        store
            .append_event(&generated_signal(
                "sig-promoted",
                0.55,
                evaluation_time - Duration::seconds(10),
            ))
            .unwrap();

        let runner = ConfirmationRunner::new(
            &store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                confidence_threshold: 0.6,
                ..ConfirmationAgentConfig::default()
            }),
        )
        .with_policy(ConfirmationPolicy {
            metadata: None,
            rules: vec![SignalPolicyRule {
                signal_name: "odds_jump".into(),
                direction: None,
                source: None,
                status: SignalPolicyStatus::Promoted,
                confidence_threshold: Some(0.5),
                horizon_seconds: Some(3600),
            }],
        });

        let report = runner.run().unwrap();

        assert_eq!(report.accepted, 1);
        assert_eq!(report.policy_overrides_used, 1);
        assert_eq!(report.items[0].applied_confidence_threshold, Some(0.5));
        assert_eq!(
            report.items[0].policy_status,
            Some(SignalPolicyStatus::Promoted)
        );

        cleanup(&path);
    }

    #[test]
    fn policy_keeps_deterministic_behavior_for_same_input() {
        let first_path = temp_store_path("policy-deterministic-first");
        let second_path = temp_store_path("policy-deterministic-second");
        let first_store = JsonlEventStore::new(&first_path).unwrap();
        let second_store = JsonlEventStore::new(&second_path).unwrap();
        let evaluation_time = Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap();
        let policy = ConfirmationPolicy {
            metadata: None,
            rules: vec![SignalPolicyRule {
                signal_name: "activity_spike".into(),
                direction: Some(MarketSignalDirection::Yes),
                source: Some(MarketSource::Synthetic),
                status: SignalPolicyStatus::Promoted,
                confidence_threshold: Some(0.4),
                horizon_seconds: None,
            }],
        };
        let first_runner = ConfirmationRunner::new(
            &first_store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        )
        .with_policy(policy.clone());
        let second_runner = ConfirmationRunner::new(
            &second_store,
            crate::ConfirmationAgent::new(ConfirmationAgentConfig {
                evaluation_time: Some(evaluation_time),
                ..ConfirmationAgentConfig::default()
            }),
        )
        .with_policy(policy);
        let signals = vec![stored_signal_to_market_signal(
            "sig-1".into(),
            Some("market-1".into()),
            "market-1".into(),
            "activity_spike".into(),
            SignalSide::Long,
            0.41,
            Some("runner deterministic".into()),
            evaluation_time - Duration::seconds(20),
        )];

        let first = first_runner.run_signals(&signals).unwrap();
        let second = second_runner.run_signals(&signals).unwrap();

        assert_eq!(first.accepted, second.accepted);
        assert_eq!(first.rejected, second.rejected);
        assert_eq!(
            first.rejected_low_confidence,
            second.rejected_low_confidence
        );
        assert_eq!(first.rejected_stale, second.rejected_stale);
        assert_eq!(first.skipped_frozen, second.skipped_frozen);
        assert_eq!(
            first.skipped_already_confirmed,
            second.skipped_already_confirmed
        );
        assert_eq!(first.policy_overrides_used, second.policy_overrides_used);
        assert_eq!(first.items, second.items);
        assert_eq!(first.emitted_events.len(), 1);
        assert_eq!(second.emitted_events.len(), 1);
        assert_eq!(
            first.emitted_events[0].idempotency_key,
            second.emitted_events[0].idempotency_key
        );
        assert_eq!(
            first.emitted_events[0].payload,
            second.emitted_events[0].payload
        );

        cleanup(&first_path);
        cleanup(&second_path);
    }
}
