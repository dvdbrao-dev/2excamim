use market_domain::{MarketSignalDirection, MarketSource};

use crate::{ConfirmationDisposition, ConfirmationRunReport};

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationDirectionStats {
    pub direction: MarketSignalDirection,
    pub total: usize,
    pub accepted: usize,
    pub rejected_low_confidence: usize,
    pub rejected_stale: usize,
    pub skipped_frozen: usize,
    pub skipped_already_confirmed: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationScorecard {
    pub processed_count: usize,
    pub acceptance_rate: f64,
    pub rejection_rate: f64,
    pub low_confidence_rate: f64,
    pub stale_rate: f64,
    pub skipped_frozen_rate: f64,
    pub skipped_already_confirmed_rate: f64,
    pub by_direction: Vec<ConfirmationDirectionStats>,
    pub by_source: Vec<(MarketSource, usize)>,
    pub by_signal_name: Vec<(String, usize)>,
}

impl ConfirmationScorecard {
    pub fn from_report(report: &ConfirmationRunReport) -> Self {
        let processed = report.total_signals_processed as f64;
        let denominator = if processed > 0.0 { processed } else { 1.0 };

        let by_direction = [
            MarketSignalDirection::Yes,
            MarketSignalDirection::No,
            MarketSignalDirection::Neutral,
        ]
        .into_iter()
        .map(|direction| {
            let relevant = report
                .items
                .iter()
                .filter(|item| item.direction == direction)
                .collect::<Vec<_>>();

            ConfirmationDirectionStats {
                direction,
                total: relevant.len(),
                accepted: relevant
                    .iter()
                    .filter(|item| item.disposition == ConfirmationDisposition::Accepted)
                    .count(),
                rejected_low_confidence: relevant
                    .iter()
                    .filter(|item| {
                        item.disposition == ConfirmationDisposition::RejectedLowConfidence
                    })
                    .count(),
                rejected_stale: relevant
                    .iter()
                    .filter(|item| item.disposition == ConfirmationDisposition::RejectedStale)
                    .count(),
                skipped_frozen: relevant
                    .iter()
                    .filter(|item| item.disposition == ConfirmationDisposition::SkippedFrozen)
                    .count(),
                skipped_already_confirmed: relevant
                    .iter()
                    .filter(|item| {
                        item.disposition == ConfirmationDisposition::SkippedAlreadyConfirmed
                    })
                    .count(),
            }
        })
        .collect::<Vec<_>>();

        let mut by_signal_name = std::collections::BTreeMap::new();
        for item in &report.items {
            *by_signal_name
                .entry(item.signal_name.clone())
                .or_insert(0usize) += 1;
        }
        let by_source = [
            MarketSource::Polymarket,
            MarketSource::Kalshi,
            MarketSource::Manual,
            MarketSource::Synthetic,
        ]
        .into_iter()
        .filter_map(|source| {
            let count = report
                .items
                .iter()
                .filter(|item| item.source == source)
                .count();
            (count > 0).then_some((source, count))
        })
        .collect();

        Self {
            processed_count: report.total_signals_processed,
            acceptance_rate: report.accepted as f64 / denominator,
            rejection_rate: report.rejected as f64 / denominator,
            low_confidence_rate: report.rejected_low_confidence as f64 / denominator,
            stale_rate: report.rejected_stale as f64 / denominator,
            skipped_frozen_rate: report.skipped_frozen as f64 / denominator,
            skipped_already_confirmed_rate: report.skipped_already_confirmed as f64 / denominator,
            by_direction,
            by_source,
            by_signal_name: by_signal_name.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use market_domain::{MarketSignalDirection, MarketSource};

    use crate::{ConfirmationDisposition, ConfirmationRunItem, ConfirmationRunReport};

    use super::ConfirmationScorecard;

    #[test]
    fn scorecard_computes_rates_and_breakdowns() {
        let report = ConfirmationRunReport {
            total_signals_processed: 4,
            accepted: 1,
            rejected: 2,
            rejected_low_confidence: 1,
            rejected_stale: 1,
            skipped_frozen: 0,
            skipped_already_confirmed: 1,
            policy_overrides_used: 0,
            persisted: 1,
            duplicates: 0,
            items: vec![
                ConfirmationRunItem {
                    signal_id: "sig-1".into(),
                    disposition: ConfirmationDisposition::Accepted,
                    persisted: true,
                    market_id: "market-1".into(),
                    direction: MarketSignalDirection::Yes,
                    source: MarketSource::Synthetic,
                    signal_name: "odds_jump".into(),
                    applied_confidence_threshold: None,
                    policy_status: None,
                },
                ConfirmationRunItem {
                    signal_id: "sig-2".into(),
                    disposition: ConfirmationDisposition::RejectedLowConfidence,
                    persisted: false,
                    market_id: "market-1".into(),
                    direction: MarketSignalDirection::No,
                    source: MarketSource::Synthetic,
                    signal_name: "odds_jump".into(),
                    applied_confidence_threshold: None,
                    policy_status: None,
                },
                ConfirmationRunItem {
                    signal_id: "sig-3".into(),
                    disposition: ConfirmationDisposition::RejectedStale,
                    persisted: false,
                    market_id: "market-2".into(),
                    direction: MarketSignalDirection::Neutral,
                    source: MarketSource::Synthetic,
                    signal_name: "activity_spike".into(),
                    applied_confidence_threshold: None,
                    policy_status: None,
                },
                ConfirmationRunItem {
                    signal_id: "sig-4".into(),
                    disposition: ConfirmationDisposition::SkippedAlreadyConfirmed,
                    persisted: false,
                    market_id: "market-2".into(),
                    direction: MarketSignalDirection::Yes,
                    source: MarketSource::Synthetic,
                    signal_name: "activity_spike".into(),
                    applied_confidence_threshold: None,
                    policy_status: None,
                },
            ],
            emitted_events: Vec::new(),
        };

        let scorecard = ConfirmationScorecard::from_report(&report);

        assert_eq!(scorecard.processed_count, 4);
        assert_eq!(scorecard.acceptance_rate, 0.25);
        assert_eq!(scorecard.rejection_rate, 0.5);
        assert_eq!(scorecard.low_confidence_rate, 0.25);
        assert_eq!(scorecard.stale_rate, 0.25);
        assert_eq!(scorecard.skipped_frozen_rate, 0.0);
        assert_eq!(scorecard.skipped_already_confirmed_rate, 0.25);
        assert_eq!(scorecard.by_source, vec![(MarketSource::Synthetic, 4)]);
        assert_eq!(
            scorecard.by_signal_name,
            vec![("activity_spike".into(), 2), ("odds_jump".into(), 2)]
        );
        assert_eq!(
            scorecard.by_direction[0].direction,
            MarketSignalDirection::Yes
        );
        assert_eq!(scorecard.by_direction[0].total, 2);
        assert_eq!(scorecard.by_direction[0].accepted, 1);
        assert_eq!(scorecard.by_direction[0].skipped_frozen, 0);
        assert_eq!(scorecard.by_direction[0].skipped_already_confirmed, 1);
    }
}
