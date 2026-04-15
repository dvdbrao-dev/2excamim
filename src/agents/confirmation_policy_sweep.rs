use std::collections::BTreeSet;

use market_domain::MarketSnapshot;

use crate::{
    compare_confirmation_quality, ConfirmationComparisonReport, ConfirmationOutcomeConfig,
    GeneratedSignalContext,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PolicySweepBreakdownRow {
    pub key: String,
    pub total_signals: usize,
    pub confirmed_signals: usize,
    pub favorable_rate: f64,
    pub unfavorable_rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolicySweepResultRow {
    pub confidence_threshold: f64,
    pub horizon_seconds: i64,
    pub total_signals: usize,
    pub confirmed_signals: usize,
    pub acceptance_rate: f64,
    pub favorable_rate: f64,
    pub unfavorable_rate: f64,
    pub average_delta_probability: Option<f64>,
    pub uplift_vs_baseline: Option<f64>,
    pub by_signal_name: Vec<PolicySweepBreakdownRow>,
    pub by_direction: Vec<PolicySweepBreakdownRow>,
    pub by_source: Vec<PolicySweepBreakdownRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolicySweepSummary {
    pub rows: Vec<PolicySweepResultRow>,
}

pub fn sweep_confirmation_policy(
    generated_signals: &[GeneratedSignalContext],
    snapshots: &[MarketSnapshot],
    confidence_thresholds: &[f64],
    horizons: &[i64],
    delta_threshold: f64,
) -> PolicySweepSummary {
    let mut rows = Vec::new();
    let mut thresholds = confidence_thresholds.to_vec();
    thresholds.sort_by(|a, b| a.total_cmp(b));
    thresholds.dedup();
    let mut horizons = horizons.to_vec();
    horizons.sort();
    horizons.dedup();

    for horizon_seconds in horizons {
        let baseline = compare_confirmation_quality(
            generated_signals,
            &generated_signals
                .iter()
                .map(|signal| signal.signal_id.clone())
                .collect::<BTreeSet<_>>(),
            snapshots,
            &ConfirmationOutcomeConfig {
                evaluation_horizon_seconds: horizon_seconds,
                delta_threshold,
            },
        );
        for confidence_threshold in &thresholds {
            let accepted = generated_signals
                .iter()
                .filter(|signal| signal.confidence >= *confidence_threshold)
                .cloned()
                .collect::<Vec<_>>();
            let confirmed_ids = accepted
                .iter()
                .map(|signal| signal.signal_id.clone())
                .collect::<BTreeSet<_>>();
            let report = compare_confirmation_quality(
                generated_signals,
                &confirmed_ids,
                snapshots,
                &ConfirmationOutcomeConfig {
                    evaluation_horizon_seconds: horizon_seconds,
                    delta_threshold,
                },
            );
            rows.push(result_row(
                *confidence_threshold,
                horizon_seconds,
                generated_signals.len(),
                accepted.len(),
                &baseline,
                &report,
            ));
        }
    }

    PolicySweepSummary { rows }
}

fn result_row(
    confidence_threshold: f64,
    horizon_seconds: i64,
    total_signals: usize,
    confirmed_signals: usize,
    baseline: &ConfirmationComparisonReport,
    report: &ConfirmationComparisonReport,
) -> PolicySweepResultRow {
    let denominator = if total_signals == 0 {
        1.0
    } else {
        total_signals as f64
    };
    PolicySweepResultRow {
        confidence_threshold,
        horizon_seconds,
        total_signals,
        confirmed_signals,
        acceptance_rate: confirmed_signals as f64 / denominator,
        favorable_rate: report.favorable_rate_confirmed,
        unfavorable_rate: report.unfavorable_rate_confirmed,
        average_delta_probability: report.average_delta_confirmed,
        uplift_vs_baseline: Some(
            report.favorable_rate_confirmed - baseline.favorable_rate_all_generated,
        ),
        by_signal_name: report
            .by_signal_name
            .iter()
            .map(|row| PolicySweepBreakdownRow {
                key: row.key.clone(),
                total_signals: row.all_generated_count,
                confirmed_signals: row.confirmed_count,
                favorable_rate: row.favorable_rate_confirmed,
                unfavorable_rate: row.unfavorable_rate_confirmed,
            })
            .collect(),
        by_direction: report
            .by_direction
            .iter()
            .map(|row| PolicySweepBreakdownRow {
                key: row.key.clone(),
                total_signals: row.all_generated_count,
                confirmed_signals: row.confirmed_count,
                favorable_rate: row.favorable_rate_confirmed,
                unfavorable_rate: row.unfavorable_rate_confirmed,
            })
            .collect(),
        by_source: report
            .by_source
            .iter()
            .map(|row| PolicySweepBreakdownRow {
                key: row.key.clone(),
                total_signals: row.all_generated_count,
                confirmed_signals: row.confirmed_count,
                favorable_rate: row.favorable_rate_confirmed,
                unfavorable_rate: row.unfavorable_rate_confirmed,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSnapshot, MarketSource, MarketStatus};

    use super::sweep_confirmation_policy;
    use crate::GeneratedSignalContext;

    fn signal(id: &str, confidence: f64) -> GeneratedSignalContext {
        GeneratedSignalContext {
            signal_id: id.into(),
            market_id: format!("market-{id}"),
            signal_name: "odds_jump".into(),
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
    fn sweep_is_deterministic() {
        let signals = vec![signal("1", 0.4), signal("2", 0.8)];
        let snapshots = vec![
            snapshot("market-1", 0, 0.4),
            snapshot("market-1", 60, 0.41),
            snapshot("market-2", 0, 0.4),
            snapshot("market-2", 60, 0.46),
        ];

        let first = sweep_confirmation_policy(&signals, &snapshots, &[0.5, 0.7], &[3600], 0.02);
        let second = sweep_confirmation_policy(&signals, &snapshots, &[0.7, 0.5], &[3600], 0.02);

        assert_eq!(first, second);
    }
}
