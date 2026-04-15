use chrono::{DateTime, Duration, Utc};
use market_domain::{MarketSignalDirection, MarketSnapshot, MarketSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationOutcomeLabel {
    Favorable,
    Unfavorable,
    Neutral,
    InsufficientData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationOutcomeRecord {
    pub signal_id: String,
    pub market_id: String,
    pub signal_name: String,
    pub direction: MarketSignalDirection,
    pub confirmed_at: DateTime<Utc>,
    pub evaluation_horizon_seconds: i64,
    pub entry_probability: Option<f64>,
    pub exit_probability: Option<f64>,
    pub delta_probability: Option<f64>,
    pub outcome_label: ConfirmationOutcomeLabel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeSignalContext {
    pub signal_id: String,
    pub market_id: String,
    pub signal_name: String,
    pub direction: MarketSignalDirection,
    pub source: MarketSource,
    pub evaluation_started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationOutcomeConfig {
    pub evaluation_horizon_seconds: i64,
    pub delta_threshold: f64,
}

impl Default for ConfirmationOutcomeConfig {
    fn default() -> Self {
        Self {
            evaluation_horizon_seconds: 3600,
            delta_threshold: 0.02,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedSignalContext {
    pub signal_id: String,
    pub market_id: String,
    pub signal_name: String,
    pub direction: MarketSignalDirection,
    pub source: MarketSource,
    pub confirmed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectionOutcomeStats {
    pub direction: MarketSignalDirection,
    pub favorable: usize,
    pub unfavorable: usize,
    pub neutral: usize,
    pub insufficient_data: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationOutcomeScorecard {
    pub processed_count: usize,
    pub favorable_rate: f64,
    pub unfavorable_rate: f64,
    pub neutral_rate: f64,
    pub insufficient_data_rate: f64,
    pub average_delta_probability: Option<f64>,
    pub by_direction: Vec<DirectionOutcomeStats>,
    pub by_signal_name: Vec<(String, usize)>,
}

impl ConfirmationOutcomeScorecard {
    pub fn from_records(records: &[ConfirmationOutcomeRecord]) -> Self {
        let total = records.len() as f64;
        let denominator = if total > 0.0 { total } else { 1.0 };
        let favorable = records
            .iter()
            .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::Favorable)
            .count();
        let unfavorable = records
            .iter()
            .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::Unfavorable)
            .count();
        let neutral = records
            .iter()
            .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::Neutral)
            .count();
        let insufficient_data = records
            .iter()
            .filter(|record| record.outcome_label == ConfirmationOutcomeLabel::InsufficientData)
            .count();
        let deltas = records
            .iter()
            .filter_map(|record| record.delta_probability)
            .collect::<Vec<_>>();
        let average_delta_probability =
            (!deltas.is_empty()).then_some(deltas.iter().sum::<f64>() / deltas.len() as f64);

        let by_direction = [
            MarketSignalDirection::Yes,
            MarketSignalDirection::No,
            MarketSignalDirection::Neutral,
        ]
        .into_iter()
        .map(|direction| DirectionOutcomeStats {
            direction,
            favorable: records
                .iter()
                .filter(|record| {
                    record.direction == direction
                        && record.outcome_label == ConfirmationOutcomeLabel::Favorable
                })
                .count(),
            unfavorable: records
                .iter()
                .filter(|record| {
                    record.direction == direction
                        && record.outcome_label == ConfirmationOutcomeLabel::Unfavorable
                })
                .count(),
            neutral: records
                .iter()
                .filter(|record| {
                    record.direction == direction
                        && record.outcome_label == ConfirmationOutcomeLabel::Neutral
                })
                .count(),
            insufficient_data: records
                .iter()
                .filter(|record| {
                    record.direction == direction
                        && record.outcome_label == ConfirmationOutcomeLabel::InsufficientData
                })
                .count(),
        })
        .collect();

        let mut by_signal_name = std::collections::BTreeMap::new();
        for record in records {
            *by_signal_name
                .entry(record.signal_name.clone())
                .or_insert(0usize) += 1;
        }

        Self {
            processed_count: records.len(),
            favorable_rate: favorable as f64 / denominator,
            unfavorable_rate: unfavorable as f64 / denominator,
            neutral_rate: neutral as f64 / denominator,
            insufficient_data_rate: insufficient_data as f64 / denominator,
            average_delta_probability,
            by_direction,
            by_signal_name: by_signal_name.into_iter().collect(),
        }
    }
}

pub fn evaluate_confirmation_outcome(
    signal: &ConfirmedSignalContext,
    snapshots: &[MarketSnapshot],
    config: &ConfirmationOutcomeConfig,
) -> ConfirmationOutcomeRecord {
    evaluate_outcome(
        &OutcomeSignalContext {
            signal_id: signal.signal_id.clone(),
            market_id: signal.market_id.clone(),
            signal_name: signal.signal_name.clone(),
            direction: signal.direction,
            source: signal.source,
            evaluation_started_at: signal.confirmed_at,
        },
        snapshots,
        config,
    )
}

pub fn evaluate_outcome(
    signal: &OutcomeSignalContext,
    snapshots: &[MarketSnapshot],
    config: &ConfirmationOutcomeConfig,
) -> ConfirmationOutcomeRecord {
    let horizon_at =
        signal.evaluation_started_at + Duration::seconds(config.evaluation_horizon_seconds.max(0));
    let market_snapshots = snapshots
        .iter()
        .filter(|snapshot| snapshot.market_id == signal.market_id)
        .collect::<Vec<_>>();

    let entry_probability = market_snapshots
        .iter()
        .filter(|snapshot| snapshot.observed_at >= signal.evaluation_started_at)
        .find_map(|snapshot| snapshot_probability(snapshot));
    let exit_probability = market_snapshots
        .iter()
        .filter(|snapshot| snapshot.observed_at >= horizon_at)
        .find_map(|snapshot| snapshot_probability(snapshot));
    let delta_probability = match (entry_probability, exit_probability) {
        (Some(entry), Some(exit)) => Some(exit - entry),
        _ => None,
    };

    let outcome_label =
        classify_outcome(signal.direction, delta_probability, config.delta_threshold);

    ConfirmationOutcomeRecord {
        signal_id: signal.signal_id.clone(),
        market_id: signal.market_id.clone(),
        signal_name: signal.signal_name.clone(),
        direction: signal.direction,
        confirmed_at: signal.evaluation_started_at,
        evaluation_horizon_seconds: config.evaluation_horizon_seconds,
        entry_probability,
        exit_probability,
        delta_probability,
        outcome_label,
    }
}

fn classify_outcome(
    direction: MarketSignalDirection,
    delta_probability: Option<f64>,
    threshold: f64,
) -> ConfirmationOutcomeLabel {
    let Some(delta_probability) = delta_probability else {
        return ConfirmationOutcomeLabel::InsufficientData;
    };

    match direction {
        MarketSignalDirection::Yes => {
            if delta_probability > threshold {
                ConfirmationOutcomeLabel::Favorable
            } else if delta_probability < -threshold {
                ConfirmationOutcomeLabel::Unfavorable
            } else {
                ConfirmationOutcomeLabel::Neutral
            }
        }
        MarketSignalDirection::No => {
            if delta_probability < -threshold {
                ConfirmationOutcomeLabel::Favorable
            } else if delta_probability > threshold {
                ConfirmationOutcomeLabel::Unfavorable
            } else {
                ConfirmationOutcomeLabel::Neutral
            }
        }
        MarketSignalDirection::Neutral => {
            if delta_probability.abs() <= threshold {
                ConfirmationOutcomeLabel::Neutral
            } else {
                ConfirmationOutcomeLabel::Unfavorable
            }
        }
    }
}

fn snapshot_probability(snapshot: &MarketSnapshot) -> Option<f64> {
    snapshot
        .last_price
        .or_else(|| match (snapshot.best_bid, snapshot.best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            (Some(bid), None) => Some(bid),
            (None, Some(ask)) => Some(ask),
            (None, None) => None,
        })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSnapshot, MarketSource, MarketStatus};

    use super::{
        evaluate_confirmation_outcome, evaluate_outcome, ConfirmationOutcomeConfig,
        ConfirmationOutcomeLabel, ConfirmationOutcomeRecord, ConfirmationOutcomeScorecard,
        ConfirmedSignalContext, OutcomeSignalContext,
    };

    fn signal(direction: MarketSignalDirection) -> ConfirmedSignalContext {
        ConfirmedSignalContext {
            signal_id: "sig-1".into(),
            market_id: "market-1".into(),
            signal_name: "odds_jump".into(),
            direction,
            source: MarketSource::Synthetic,
            confirmed_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
        }
    }

    fn generated_signal(direction: MarketSignalDirection) -> OutcomeSignalContext {
        OutcomeSignalContext {
            signal_id: "sig-1".into(),
            market_id: "market-1".into(),
            signal_name: "odds_jump".into(),
            direction,
            source: MarketSource::Synthetic,
            evaluation_started_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
        }
    }

    fn snapshot(mins: i64, price: f64) -> MarketSnapshot {
        let mut snapshot = MarketSnapshot::new(
            "market-1",
            MarketSource::Synthetic,
            "Example",
            MarketStatus::Open,
            Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap() + chrono::Duration::minutes(mins),
        )
        .unwrap();
        snapshot.last_price = Some(price);
        snapshot
    }

    #[test]
    fn favorable_up_outcome_is_classified() {
        let record = evaluate_confirmation_outcome(
            &signal(MarketSignalDirection::Yes),
            &[snapshot(0, 0.40), snapshot(60, 0.46)],
            &ConfirmationOutcomeConfig::default(),
        );

        assert_eq!(record.outcome_label, ConfirmationOutcomeLabel::Favorable);
        assert_eq!(record.delta_probability, Some(0.06));
    }

    #[test]
    fn generated_signal_uses_generic_evaluation_start() {
        let record = evaluate_outcome(
            &generated_signal(MarketSignalDirection::Yes),
            &[snapshot(0, 0.40), snapshot(60, 0.46)],
            &ConfirmationOutcomeConfig::default(),
        );

        assert_eq!(record.outcome_label, ConfirmationOutcomeLabel::Favorable);
    }

    #[test]
    fn unfavorable_down_outcome_is_classified() {
        let record = evaluate_confirmation_outcome(
            &signal(MarketSignalDirection::No),
            &[snapshot(0, 0.40), snapshot(60, 0.46)],
            &ConfirmationOutcomeConfig::default(),
        );

        assert_eq!(record.outcome_label, ConfirmationOutcomeLabel::Unfavorable);
    }

    #[test]
    fn neutral_outcome_is_classified() {
        let record = evaluate_confirmation_outcome(
            &signal(MarketSignalDirection::Yes),
            &[snapshot(0, 0.40), snapshot(60, 0.41)],
            &ConfirmationOutcomeConfig::default(),
        );

        assert_eq!(record.outcome_label, ConfirmationOutcomeLabel::Neutral);
    }

    #[test]
    fn insufficient_data_is_classified() {
        let record = evaluate_confirmation_outcome(
            &signal(MarketSignalDirection::Yes),
            &[snapshot(0, 0.40)],
            &ConfirmationOutcomeConfig::default(),
        );

        assert_eq!(
            record.outcome_label,
            ConfirmationOutcomeLabel::InsufficientData
        );
        assert_eq!(record.exit_probability, None);
    }

    #[test]
    fn scorecard_computes_outcome_rates() {
        let records = vec![
            ConfirmationOutcomeRecord {
                signal_id: "a".into(),
                market_id: "m".into(),
                signal_name: "odds_jump".into(),
                direction: MarketSignalDirection::Yes,
                confirmed_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
                evaluation_horizon_seconds: 3600,
                entry_probability: Some(0.4),
                exit_probability: Some(0.5),
                delta_probability: Some(0.1),
                outcome_label: ConfirmationOutcomeLabel::Favorable,
            },
            ConfirmationOutcomeRecord {
                signal_id: "b".into(),
                market_id: "m".into(),
                signal_name: "odds_jump".into(),
                direction: MarketSignalDirection::No,
                confirmed_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
                evaluation_horizon_seconds: 3600,
                entry_probability: Some(0.4),
                exit_probability: Some(0.45),
                delta_probability: Some(0.05),
                outcome_label: ConfirmationOutcomeLabel::Unfavorable,
            },
            ConfirmationOutcomeRecord {
                signal_id: "c".into(),
                market_id: "m".into(),
                signal_name: "activity_spike".into(),
                direction: MarketSignalDirection::Neutral,
                confirmed_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
                evaluation_horizon_seconds: 3600,
                entry_probability: None,
                exit_probability: None,
                delta_probability: None,
                outcome_label: ConfirmationOutcomeLabel::InsufficientData,
            },
        ];

        let scorecard = ConfirmationOutcomeScorecard::from_records(&records);
        assert_eq!(scorecard.favorable_rate, 1.0 / 3.0);
        assert_eq!(scorecard.unfavorable_rate, 1.0 / 3.0);
        assert_eq!(scorecard.insufficient_data_rate, 1.0 / 3.0);
        assert!((scorecard.average_delta_probability.unwrap() - 0.075).abs() < 1e-9);
    }
}
