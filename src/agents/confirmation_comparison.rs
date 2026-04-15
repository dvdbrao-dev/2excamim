use std::collections::BTreeSet;

use market_domain::{MarketSignalDirection, MarketSnapshot, MarketSource};

use crate::{
    codecs::RehydratedEvent,
    evaluate_outcome,
    store::{JsonlEventStore, StoreError},
    ConfirmationOutcomeConfig, ConfirmationOutcomeLabel, ConfirmationOutcomeRecord,
    OutcomeSignalContext,
};

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedSignalContext {
    pub signal_id: String,
    pub market_id: String,
    pub signal_name: String,
    pub direction: MarketSignalDirection,
    pub source: MarketSource,
    pub confidence: f64,
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonBreakdownRow {
    pub key: String,
    pub confirmed_count: usize,
    pub all_generated_count: usize,
    pub favorable_rate_confirmed: f64,
    pub favorable_rate_all_generated: f64,
    pub uplift_favorable_rate: f64,
    pub unfavorable_rate_confirmed: f64,
    pub unfavorable_rate_all_generated: f64,
    pub average_delta_confirmed: Option<f64>,
    pub average_delta_all_generated: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationComparisonReport {
    pub favorable_rate_confirmed: f64,
    pub favorable_rate_all_generated: f64,
    pub uplift_favorable_rate: f64,
    pub unfavorable_rate_confirmed: f64,
    pub unfavorable_rate_all_generated: f64,
    pub average_delta_confirmed: Option<f64>,
    pub average_delta_all_generated: Option<f64>,
    pub confirmed_outcomes: Vec<ConfirmationOutcomeRecord>,
    pub all_generated_outcomes: Vec<ConfirmationOutcomeRecord>,
    pub by_signal_name: Vec<ComparisonBreakdownRow>,
    pub by_direction: Vec<ComparisonBreakdownRow>,
    pub by_source: Vec<ComparisonBreakdownRow>,
}

pub fn load_generated_signals_from_store(
    store: &JsonlEventStore,
) -> Result<Vec<GeneratedSignalContext>, StoreError> {
    let mut signals = store
        .read_all()?
        .into_iter()
        .filter_map(|event| match RehydratedEvent::try_from(event) {
            Ok(RehydratedEvent::SignalGenerated(event)) => Some(GeneratedSignalContext {
                signal_id: event.payload.signal_id,
                market_id: event.aggregate_key.unwrap_or(event.payload.instrument),
                signal_name: event.payload.timeframe,
                direction: match event.payload.side {
                    crate::events::SignalSide::Long => MarketSignalDirection::Yes,
                    crate::events::SignalSide::Short => MarketSignalDirection::No,
                    crate::events::SignalSide::Flat => MarketSignalDirection::Neutral,
                },
                source: MarketSource::Synthetic,
                confidence: event.payload.strength,
                generated_at: event.occurred_at,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    signals.sort_by(|left, right| {
        left.generated_at
            .cmp(&right.generated_at)
            .then_with(|| left.signal_id.cmp(&right.signal_id))
    });
    Ok(signals)
}

pub fn load_confirmed_signal_ids_from_store(
    store: &JsonlEventStore,
) -> Result<BTreeSet<String>, StoreError> {
    Ok(store
        .read_all()?
        .into_iter()
        .filter_map(|event| match RehydratedEvent::try_from(event) {
            Ok(RehydratedEvent::SignalConfirmed(event)) => Some(event.payload.signal_id),
            _ => None,
        })
        .collect())
}

pub fn compare_confirmation_quality(
    generated_signals: &[GeneratedSignalContext],
    confirmed_signal_ids: &BTreeSet<String>,
    snapshots: &[MarketSnapshot],
    outcome_config: &ConfirmationOutcomeConfig,
) -> ConfirmationComparisonReport {
    let mut all_generated_outcomes = generated_signals
        .iter()
        .map(|signal| evaluate_generated_signal(signal, snapshots, outcome_config))
        .collect::<Vec<_>>();
    let mut confirmed_outcomes = generated_signals
        .iter()
        .filter(|signal| confirmed_signal_ids.contains(&signal.signal_id))
        .map(|signal| evaluate_generated_signal(signal, snapshots, outcome_config))
        .collect::<Vec<_>>();
    all_generated_outcomes.sort_by(|left, right| left.signal_id.cmp(&right.signal_id));
    confirmed_outcomes.sort_by(|left, right| left.signal_id.cmp(&right.signal_id));

    ConfirmationComparisonReport {
        favorable_rate_confirmed: favorable_rate(&confirmed_outcomes),
        favorable_rate_all_generated: favorable_rate(&all_generated_outcomes),
        uplift_favorable_rate: favorable_rate(&confirmed_outcomes)
            - favorable_rate(&all_generated_outcomes),
        unfavorable_rate_confirmed: unfavorable_rate(&confirmed_outcomes),
        unfavorable_rate_all_generated: unfavorable_rate(&all_generated_outcomes),
        average_delta_confirmed: average_delta(&confirmed_outcomes),
        average_delta_all_generated: average_delta(&all_generated_outcomes),
        by_signal_name: breakdown(
            generated_signals,
            confirmed_signal_ids,
            snapshots,
            outcome_config,
            |signal| signal.signal_name.clone(),
            all_signal_names(generated_signals),
        ),
        by_direction: breakdown(
            generated_signals,
            confirmed_signal_ids,
            snapshots,
            outcome_config,
            |signal| format!("{:?}", signal.direction),
            vec!["Yes".into(), "No".into(), "Neutral".into()],
        ),
        by_source: breakdown(
            generated_signals,
            confirmed_signal_ids,
            snapshots,
            outcome_config,
            |signal| format!("{:?}", signal.source),
            vec![
                "Polymarket".into(),
                "Kalshi".into(),
                "Manual".into(),
                "Synthetic".into(),
            ],
        ),
        confirmed_outcomes,
        all_generated_outcomes,
    }
}

fn evaluate_generated_signal(
    signal: &GeneratedSignalContext,
    snapshots: &[MarketSnapshot],
    outcome_config: &ConfirmationOutcomeConfig,
) -> ConfirmationOutcomeRecord {
    evaluate_outcome(
        &OutcomeSignalContext {
            signal_id: signal.signal_id.clone(),
            market_id: signal.market_id.clone(),
            signal_name: signal.signal_name.clone(),
            direction: signal.direction,
            source: signal.source,
            evaluation_started_at: signal.generated_at,
        },
        snapshots,
        outcome_config,
    )
}

fn favorable_rate(records: &[ConfirmationOutcomeRecord]) -> f64 {
    rate_for(records, ConfirmationOutcomeLabel::Favorable)
}

fn unfavorable_rate(records: &[ConfirmationOutcomeRecord]) -> f64 {
    rate_for(records, ConfirmationOutcomeLabel::Unfavorable)
}

fn rate_for(records: &[ConfirmationOutcomeRecord], label: ConfirmationOutcomeLabel) -> f64 {
    let denominator = if records.is_empty() {
        1.0
    } else {
        records.len() as f64
    };
    records
        .iter()
        .filter(|record| record.outcome_label == label)
        .count() as f64
        / denominator
}

fn average_delta(records: &[ConfirmationOutcomeRecord]) -> Option<f64> {
    let deltas = records
        .iter()
        .filter_map(|record| record.delta_probability)
        .collect::<Vec<_>>();
    (!deltas.is_empty()).then_some(deltas.iter().sum::<f64>() / deltas.len() as f64)
}

fn all_signal_names(generated_signals: &[GeneratedSignalContext]) -> Vec<String> {
    generated_signals
        .iter()
        .map(|signal| signal.signal_name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn breakdown<F>(
    generated_signals: &[GeneratedSignalContext],
    confirmed_signal_ids: &BTreeSet<String>,
    snapshots: &[MarketSnapshot],
    outcome_config: &ConfirmationOutcomeConfig,
    key_fn: F,
    ordered_keys: Vec<String>,
) -> Vec<ComparisonBreakdownRow>
where
    F: Fn(&GeneratedSignalContext) -> String,
{
    ordered_keys
        .into_iter()
        .map(|key| {
            let relevant = generated_signals
                .iter()
                .filter(|signal| key_fn(signal) == key)
                .collect::<Vec<_>>();
            let all_records = relevant
                .iter()
                .map(|signal| evaluate_generated_signal(signal, snapshots, outcome_config))
                .collect::<Vec<_>>();
            let confirmed_records = relevant
                .iter()
                .filter(|signal| confirmed_signal_ids.contains(&signal.signal_id))
                .map(|signal| evaluate_generated_signal(signal, snapshots, outcome_config))
                .collect::<Vec<_>>();
            ComparisonBreakdownRow {
                key,
                confirmed_count: confirmed_records.len(),
                all_generated_count: all_records.len(),
                favorable_rate_confirmed: favorable_rate(&confirmed_records),
                favorable_rate_all_generated: favorable_rate(&all_records),
                uplift_favorable_rate: favorable_rate(&confirmed_records)
                    - favorable_rate(&all_records),
                unfavorable_rate_confirmed: unfavorable_rate(&confirmed_records),
                unfavorable_rate_all_generated: unfavorable_rate(&all_records),
                average_delta_confirmed: average_delta(&confirmed_records),
                average_delta_all_generated: average_delta(&all_records),
            }
        })
        .filter(|row| row.confirmed_count > 0 || row.all_generated_count > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSnapshot, MarketSource, MarketStatus};

    use super::{compare_confirmation_quality, GeneratedSignalContext};
    use crate::ConfirmationOutcomeConfig;

    fn signal(id: &str, signal_name: &str, confidence: f64) -> GeneratedSignalContext {
        GeneratedSignalContext {
            signal_id: id.into(),
            market_id: format!("market-{id}"),
            signal_name: signal_name.into(),
            direction: MarketSignalDirection::Yes,
            source: MarketSource::Synthetic,
            confidence,
            generated_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
        }
    }

    fn snapshot(market_id: &str, minutes_after: i64, price: f64) -> MarketSnapshot {
        let mut snapshot = MarketSnapshot::new(
            market_id,
            MarketSource::Synthetic,
            "Example",
            MarketStatus::Open,
            Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap() + Duration::minutes(minutes_after),
        )
        .unwrap();
        snapshot.last_price = Some(price);
        snapshot
    }

    #[test]
    fn comparison_computes_uplift() {
        let signals = vec![signal("1", "odds_jump", 0.8), signal("2", "odds_jump", 0.4)];
        let confirmed = ["1".to_string()].into_iter().collect();
        let snapshots = vec![
            snapshot("market-1", 0, 0.4),
            snapshot("market-1", 60, 0.46),
            snapshot("market-2", 0, 0.4),
            snapshot("market-2", 60, 0.35),
        ];

        let report = compare_confirmation_quality(
            &signals,
            &confirmed,
            &snapshots,
            &ConfirmationOutcomeConfig::default(),
        );

        assert_eq!(report.favorable_rate_confirmed, 1.0);
        assert_eq!(report.favorable_rate_all_generated, 0.5);
        assert_eq!(report.uplift_favorable_rate, 0.5);
    }
}
