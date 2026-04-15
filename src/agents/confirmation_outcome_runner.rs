use std::path::{Path, PathBuf};

use market_domain::{MarketSignalDirection, MarketSnapshot};

use crate::{
    codecs::RehydratedEvent,
    evaluate_confirmation_outcome,
    store::{JsonlEventStore, StoreError},
    ConfirmationOutcomeConfig, ConfirmationOutcomeLabel, ConfirmationOutcomeRecord,
    ConfirmedSignalContext,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationOutcomeRunReport {
    pub total_confirmed_signals_processed: usize,
    pub favorable: usize,
    pub unfavorable: usize,
    pub neutral: usize,
    pub insufficient_data: usize,
    pub outcomes: Vec<ConfirmationOutcomeRecord>,
}

#[derive(Debug)]
pub struct ConfirmationOutcomeRunner<'a> {
    store: &'a JsonlEventStore,
    snapshots_path: PathBuf,
    config: ConfirmationOutcomeConfig,
}

impl<'a> ConfirmationOutcomeRunner<'a> {
    pub fn new(
        store: &'a JsonlEventStore,
        snapshots_path: impl Into<PathBuf>,
        config: ConfirmationOutcomeConfig,
    ) -> Self {
        Self {
            store,
            snapshots_path: snapshots_path.into(),
            config,
        }
    }

    pub fn run(&self) -> Result<ConfirmationOutcomeRunReport, StoreError> {
        let signals = self.load_confirmed_signals()?;
        let snapshots = load_snapshots(&self.snapshots_path)?;
        Ok(self.run_with_snapshots(&signals, &snapshots))
    }

    pub fn run_with_snapshots(
        &self,
        signals: &[ConfirmedSignalContext],
        snapshots: &[MarketSnapshot],
    ) -> ConfirmationOutcomeRunReport {
        let mut outcomes = signals
            .iter()
            .map(|signal| evaluate_confirmation_outcome(signal, snapshots, &self.config))
            .collect::<Vec<_>>();
        outcomes.sort_by(|left, right| left.signal_id.cmp(&right.signal_id));

        ConfirmationOutcomeRunReport {
            total_confirmed_signals_processed: outcomes.len(),
            favorable: outcomes
                .iter()
                .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::Favorable)
                .count(),
            unfavorable: outcomes
                .iter()
                .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::Unfavorable)
                .count(),
            neutral: outcomes
                .iter()
                .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::Neutral)
                .count(),
            insufficient_data: outcomes
                .iter()
                .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::InsufficientData)
                .count(),
            outcomes,
        }
    }

    fn load_confirmed_signals(&self) -> Result<Vec<ConfirmedSignalContext>, StoreError> {
        let events = self.store.read_all()?;
        let mut generated_by_signal_id = std::collections::BTreeMap::new();
        for event in &events {
            if let Ok(RehydratedEvent::SignalGenerated(event)) = RehydratedEvent::try_from(event) {
                generated_by_signal_id.insert(
                    event.payload.signal_id.clone(),
                    (
                        event.aggregate_key.unwrap_or(event.payload.instrument),
                        event.payload.timeframe,
                        match event.payload.side {
                            crate::events::SignalSide::Long => MarketSignalDirection::Yes,
                            crate::events::SignalSide::Short => MarketSignalDirection::No,
                            crate::events::SignalSide::Flat => MarketSignalDirection::Neutral,
                        },
                        market_domain::MarketSource::Synthetic,
                    ),
                );
            }
        }

        let mut confirmed = events
            .into_iter()
            .filter_map(|event| match RehydratedEvent::try_from(event) {
                Ok(RehydratedEvent::SignalConfirmed(event)) => {
                    generated_by_signal_id.get(&event.payload.signal_id).map(
                        |(market_id, signal_name, direction, source)| ConfirmedSignalContext {
                            signal_id: event.payload.signal_id.clone(),
                            market_id: market_id.clone(),
                            signal_name: signal_name.clone(),
                            direction: *direction,
                            source: *source,
                            confirmed_at: event.occurred_at,
                        },
                    )
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        confirmed.sort_by(|left, right| {
            left.confirmed_at
                .cmp(&right.confirmed_at)
                .then_with(|| left.signal_id.cmp(&right.signal_id))
        });
        confirmed.dedup_by(|left, right| left.signal_id == right.signal_id);

        Ok(confirmed)
    }
}

fn load_snapshots(path: &Path) -> Result<Vec<MarketSnapshot>, StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::File::create(path)?;
    }

    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut snapshots = Vec::new();
    for (index, line) in std::io::BufRead::lines(reader).enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let snapshot: MarketSnapshot = serde_json::from_str(&line).map_err(|error| {
            StoreError::invalid_data(format!(
                "failed to parse snapshot JSONL line {}: {error}",
                index + 1
            ))
        })?;
        market_domain::Validate::validate(&snapshot)
            .map_err(|error| StoreError::invalid_data(error.to_string()))?;
        snapshots.push(snapshot);
    }
    snapshots.sort_by(|left, right| {
        left.market_id
            .cmp(&right.market_id)
            .then_with(|| left.observed_at.cmp(&right.observed_at))
    });
    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketSnapshot, MarketSource, MarketStatus};

    use crate::{
        events::{
            EventEnvelope, Linkage, Provenance, SignalConfirmed, SignalGenerated, SignalSide,
            SourceKind,
        },
        store::{JsonlEventStore, StoredEvent},
        ConfirmationOutcomeConfig, ConfirmationOutcomeLabel, ConfirmationOutcomeRunner,
    };

    fn temp_path(name: &str, suffix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("twoexcamim-{name}-{nanos}.{suffix}"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }

    fn provenance() -> Provenance {
        Provenance {
            source_kind: SourceKind::Derived,
            source_ref: Some("confirmation-outcomes://tests".into()),
            producer_run_id: Some("run-1".into()),
            actor: Some("tests".into()),
            trace_id: Some("trace-1".into()),
            notes: None,
        }
    }

    fn stored<T: serde::Serialize>(event: EventEnvelope<T>) -> StoredEvent {
        StoredEvent::try_from(event).unwrap()
    }

    fn sample_store() -> (JsonlEventStore, std::path::PathBuf) {
        let path = temp_path("confirmation-outcomes-store", "jsonl");
        (JsonlEventStore::new(&path).unwrap(), path)
    }

    fn snapshot(path: &std::path::Path, minutes_after: i64, price: f64) {
        let mut snapshot = MarketSnapshot::new(
            "market-1",
            MarketSource::Synthetic,
            "Example",
            MarketStatus::Open,
            Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap() + Duration::minutes(minutes_after),
        )
        .unwrap();
        snapshot.last_price = Some(price);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        serde_json::to_writer(&mut file, &snapshot).unwrap();
        use std::io::Write;
        file.write_all(b"\n").unwrap();
    }

    fn seed_confirmed_signal(store: &JsonlEventStore) {
        let generated = stored(
            EventEnvelope::new_signal_generated(
                "signal-engine",
                Some("market-1".into()),
                Linkage {
                    signal_id: Some("sig-1".into()),
                    ..Linkage::default()
                },
                provenance(),
                SignalGenerated {
                    signal_id: "sig-1".into(),
                    hypothesis_id: None,
                    instrument: "market-1".into(),
                    timeframe: "odds_jump".into(),
                    side: SignalSide::Long,
                    strength: 0.8,
                    rationale: None,
                },
            )
            .unwrap(),
        );
        let mut confirmed = EventEnvelope::new_signal_confirmed(
            "confirmation-agent-v1",
            Some("market-1".into()),
            Linkage {
                signal_id: Some("sig-1".into()),
                ..Linkage::default()
            },
            provenance(),
            SignalConfirmed {
                signal_id: "sig-1".into(),
                confirmed_by: "confirmation-agent-v1".into(),
                confirmation_reason: None,
                confirmation_score: Some(0.8),
            },
        )
        .unwrap();
        confirmed.occurred_at = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();

        store
            .append_events(&[generated, stored(confirmed)])
            .unwrap();
    }

    #[test]
    fn runner_reports_favorable_outcome() {
        let (store, store_path) = sample_store();
        let snapshots_path = temp_path("confirmation-outcomes-snapshots", "jsonl");
        seed_confirmed_signal(&store);
        snapshot(&snapshots_path, 0, 0.40);
        snapshot(&snapshots_path, 60, 0.46);

        let report = ConfirmationOutcomeRunner::new(
            &store,
            &snapshots_path,
            ConfirmationOutcomeConfig::default(),
        )
        .run()
        .unwrap();

        assert_eq!(report.total_confirmed_signals_processed, 1);
        assert_eq!(report.favorable, 1);
        assert_eq!(
            report.outcomes[0].outcome_label,
            ConfirmationOutcomeLabel::Favorable
        );

        cleanup(&store_path);
        cleanup(&snapshots_path);
    }

    #[test]
    fn runner_is_deterministic_for_same_inputs() {
        let (store, store_path) = sample_store();
        let snapshots_path = temp_path("confirmation-outcomes-deterministic", "jsonl");
        seed_confirmed_signal(&store);
        snapshot(&snapshots_path, 0, 0.40);
        snapshot(&snapshots_path, 60, 0.46);

        let runner = ConfirmationOutcomeRunner::new(
            &store,
            &snapshots_path,
            ConfirmationOutcomeConfig::default(),
        );
        let first = runner.run().unwrap();
        let second = runner.run().unwrap();

        assert_eq!(first, second);

        cleanup(&store_path);
        cleanup(&snapshots_path);
    }
}
