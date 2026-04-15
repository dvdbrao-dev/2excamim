use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use market_domain::MarketSnapshot;

use crate::{
    advise_signal_families, compare_confirmation_quality, load_generated_signals_from_store,
    sweep_confirmation_policy, AdvisorySignalFamilyMetrics, ComparisonBreakdownRow,
    ConfirmationEra, ConfirmationEraSplit, ConfirmationEraWindow, ConfirmationOutcomeConfig,
    ConfirmationPolicyAdvisory, ConfirmationPolicyAdvisoryClassification,
    ConfirmationPolicyAdvisoryConfig, GeneratedSignalContext, PolicySweepBreakdownRow,
    PolicySweepResultRow,
};

#[derive(Debug, Clone, PartialEq)]
pub struct WalkForwardPolicyChoice {
    pub confidence_threshold: f64,
    pub horizon_seconds: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationWalkForwardStep {
    pub train_era_id: String,
    pub validation_era_id: String,
    pub chosen_confidence_threshold: f64,
    pub chosen_horizon_seconds: i64,
    pub train_metrics: PolicySweepResultRow,
    pub validation_metrics: PolicySweepResultRow,
    pub confirmed_sample_count_validation: usize,
    pub validation_advisory: Vec<ConfirmationPolicyAdvisory>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolicyChoiceFrequency<T> {
    pub value: T,
    pub count: usize,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdvisoryStabilityRow {
    pub signal_name: String,
    pub promote_candidate_count: usize,
    pub review_count: usize,
    pub freeze_candidate_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationWalkForwardSummary {
    pub average_validation_favorable_rate: f64,
    pub average_validation_uplift: f64,
    pub chosen_thresholds: Vec<PolicyChoiceFrequency<f64>>,
    pub chosen_horizons: Vec<PolicyChoiceFrequency<i64>>,
    pub advisory_stability: Vec<AdvisoryStabilityRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationWalkForwardReport {
    pub eras: Vec<ConfirmationEra>,
    pub steps: Vec<ConfirmationWalkForwardStep>,
    pub summary: ConfirmationWalkForwardSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationWalkForwardConfig {
    pub era_split: ConfirmationEraSplit,
    pub confidence_thresholds: Vec<f64>,
    pub horizons: Vec<i64>,
    pub delta_threshold: f64,
    pub advisory_config: ConfirmationPolicyAdvisoryConfig,
}

impl Default for ConfirmationWalkForwardConfig {
    fn default() -> Self {
        Self {
            era_split: ConfirmationEraSplit::EraCount(3),
            confidence_thresholds: vec![0.6],
            horizons: vec![3600],
            delta_threshold: 0.02,
            advisory_config: ConfirmationPolicyAdvisoryConfig::default(),
        }
    }
}

pub fn run_confirmation_walkforward(
    generated_signals: &[GeneratedSignalContext],
    snapshots: &[MarketSnapshot],
    config: &ConfirmationWalkForwardConfig,
) -> ConfirmationWalkForwardReport {
    let eras = crate::split_generated_signals_into_eras(generated_signals, config.era_split);
    let steps = eras
        .windows(2)
        .map(|pair| walkforward_step(&pair[0], &pair[1], snapshots, config))
        .collect::<Vec<_>>();

    ConfirmationWalkForwardReport {
        eras: eras.iter().map(|window| window.era.clone()).collect(),
        summary: build_summary(&steps),
        steps,
    }
}

pub fn run_confirmation_walkforward_from_store(
    store: &crate::store::JsonlEventStore,
    snapshots_path: &Path,
    config: &ConfirmationWalkForwardConfig,
) -> Result<ConfirmationWalkForwardReport, crate::store::StoreError> {
    let generated = load_generated_signals_from_store(store)?;
    let snapshots = load_snapshots_jsonl(snapshots_path)?;
    Ok(run_confirmation_walkforward(&generated, &snapshots, config))
}

pub fn load_snapshots_jsonl(path: &Path) -> Result<Vec<MarketSnapshot>, crate::store::StoreError> {
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
            crate::store::StoreError::invalid_data(format!(
                "failed to parse snapshot JSONL line {}: {error}",
                index + 1
            ))
        })?;
        market_domain::Validate::validate(&snapshot)
            .map_err(|error| crate::store::StoreError::invalid_data(error.to_string()))?;
        snapshots.push(snapshot);
    }
    snapshots.sort_by(|left, right| {
        left.market_id
            .cmp(&right.market_id)
            .then_with(|| left.observed_at.cmp(&right.observed_at))
    });
    Ok(snapshots)
}

fn walkforward_step(
    train_era: &ConfirmationEraWindow,
    validation_era: &ConfirmationEraWindow,
    snapshots: &[MarketSnapshot],
    config: &ConfirmationWalkForwardConfig,
) -> ConfirmationWalkForwardStep {
    let sweep = sweep_confirmation_policy(
        &train_era.signals,
        snapshots,
        &config.confidence_thresholds,
        &config.horizons,
        config.delta_threshold,
    );
    let train_metrics = choose_best_policy(&sweep.rows).unwrap_or_else(|| {
        evaluate_policy_for_signals(
            &train_era.signals,
            snapshots,
            config.confidence_thresholds[0],
            config.horizons[0],
            config.delta_threshold,
        )
    });
    let validation_metrics = evaluate_policy_for_signals(
        &validation_era.signals,
        snapshots,
        train_metrics.confidence_threshold,
        train_metrics.horizon_seconds,
        config.delta_threshold,
    );
    let validation_advisory =
        build_validation_advisory(&validation_metrics, &config.advisory_config);

    ConfirmationWalkForwardStep {
        train_era_id: train_era.era.era_id.clone(),
        validation_era_id: validation_era.era.era_id.clone(),
        chosen_confidence_threshold: train_metrics.confidence_threshold,
        chosen_horizon_seconds: train_metrics.horizon_seconds,
        confirmed_sample_count_validation: validation_metrics.confirmed_signals,
        train_metrics,
        validation_metrics,
        validation_advisory,
    }
}

fn choose_best_policy(rows: &[PolicySweepResultRow]) -> Option<PolicySweepResultRow> {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| {
        right
            .favorable_rate
            .total_cmp(&left.favorable_rate)
            .then_with(|| left.unfavorable_rate.total_cmp(&right.unfavorable_rate))
            .then_with(|| right.confirmed_signals.cmp(&left.confirmed_signals))
            .then_with(|| {
                left.confidence_threshold
                    .total_cmp(&right.confidence_threshold)
            })
            .then_with(|| left.horizon_seconds.cmp(&right.horizon_seconds))
    });
    ordered.into_iter().next()
}

fn evaluate_policy_for_signals(
    generated_signals: &[GeneratedSignalContext],
    snapshots: &[MarketSnapshot],
    confidence_threshold: f64,
    horizon_seconds: i64,
    delta_threshold: f64,
) -> PolicySweepResultRow {
    let outcome_config = ConfirmationOutcomeConfig {
        evaluation_horizon_seconds: horizon_seconds,
        delta_threshold,
    };
    let confirmed_ids = generated_signals
        .iter()
        .filter(|signal| signal.confidence >= confidence_threshold)
        .map(|signal| signal.signal_id.clone())
        .collect::<BTreeSet<_>>();
    let baseline = compare_confirmation_quality(
        generated_signals,
        &generated_signals
            .iter()
            .map(|signal| signal.signal_id.clone())
            .collect::<BTreeSet<_>>(),
        snapshots,
        &outcome_config,
    );
    let report = compare_confirmation_quality(
        generated_signals,
        &confirmed_ids,
        snapshots,
        &outcome_config,
    );

    policy_result_row(
        confidence_threshold,
        horizon_seconds,
        generated_signals.len(),
        confirmed_ids.len(),
        &baseline,
        &report,
    )
}

fn policy_result_row(
    confidence_threshold: f64,
    horizon_seconds: i64,
    total_signals: usize,
    confirmed_signals: usize,
    baseline: &crate::ConfirmationComparisonReport,
    report: &crate::ConfirmationComparisonReport,
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
        by_signal_name: to_breakdown_rows(&report.by_signal_name),
        by_direction: to_breakdown_rows(&report.by_direction),
        by_source: to_breakdown_rows(&report.by_source),
    }
}

fn to_breakdown_rows(rows: &[ComparisonBreakdownRow]) -> Vec<PolicySweepBreakdownRow> {
    rows.iter()
        .map(|row| PolicySweepBreakdownRow {
            key: row.key.clone(),
            total_signals: row.all_generated_count,
            confirmed_signals: row.confirmed_count,
            favorable_rate: row.favorable_rate_confirmed,
            unfavorable_rate: row.unfavorable_rate_confirmed,
        })
        .collect()
}

fn build_validation_advisory(
    validation_metrics: &PolicySweepResultRow,
    config: &ConfirmationPolicyAdvisoryConfig,
) -> Vec<ConfirmationPolicyAdvisory> {
    let metrics = validation_metrics
        .by_signal_name
        .iter()
        .map(|row| AdvisorySignalFamilyMetrics {
            signal_name: row.key.clone(),
            sample_count: row.confirmed_signals,
            favorable_rate: row.favorable_rate,
            unfavorable_rate: row.unfavorable_rate,
        })
        .collect::<Vec<_>>();
    advise_signal_families(&metrics, config)
}

fn build_summary(steps: &[ConfirmationWalkForwardStep]) -> ConfirmationWalkForwardSummary {
    let denominator = if steps.is_empty() {
        1.0
    } else {
        steps.len() as f64
    };

    ConfirmationWalkForwardSummary {
        average_validation_favorable_rate: steps
            .iter()
            .map(|step| step.validation_metrics.favorable_rate)
            .sum::<f64>()
            / denominator,
        average_validation_uplift: steps
            .iter()
            .map(|step| step.validation_metrics.uplift_vs_baseline.unwrap_or(0.0))
            .sum::<f64>()
            / denominator,
        chosen_thresholds: frequency_by_threshold(steps),
        chosen_horizons: frequency_by_horizon(steps),
        advisory_stability: advisory_stability(steps),
    }
}

fn frequency_by_threshold(
    steps: &[ConfirmationWalkForwardStep],
) -> Vec<PolicyChoiceFrequency<f64>> {
    let mut counts = BTreeMap::new();
    for step in steps {
        *counts
            .entry(step.chosen_confidence_threshold.to_bits())
            .or_insert(0usize) += 1;
    }

    let denominator = if steps.is_empty() {
        1.0
    } else {
        steps.len() as f64
    };
    counts
        .into_iter()
        .map(|(bits, count)| PolicyChoiceFrequency {
            value: f64::from_bits(bits),
            count,
            rate: count as f64 / denominator,
        })
        .collect()
}

fn frequency_by_horizon(steps: &[ConfirmationWalkForwardStep]) -> Vec<PolicyChoiceFrequency<i64>> {
    let mut counts = BTreeMap::new();
    for step in steps {
        *counts.entry(step.chosen_horizon_seconds).or_insert(0usize) += 1;
    }

    let denominator = if steps.is_empty() {
        1.0
    } else {
        steps.len() as f64
    };
    counts
        .into_iter()
        .map(|(value, count)| PolicyChoiceFrequency {
            value,
            count,
            rate: count as f64 / denominator,
        })
        .collect()
}

fn advisory_stability(steps: &[ConfirmationWalkForwardStep]) -> Vec<AdvisoryStabilityRow> {
    let mut counts = BTreeMap::<String, (usize, usize, usize)>::new();
    for step in steps {
        for advisory in &step.validation_advisory {
            let entry = counts
                .entry(advisory.signal_name.clone())
                .or_insert((0usize, 0usize, 0usize));
            match advisory.classification {
                ConfirmationPolicyAdvisoryClassification::PromoteCandidate => entry.0 += 1,
                ConfirmationPolicyAdvisoryClassification::Review => entry.1 += 1,
                ConfirmationPolicyAdvisoryClassification::FreezeCandidate => entry.2 += 1,
            }
        }
    }

    counts
        .into_iter()
        .map(
            |(signal_name, (promote_candidate_count, review_count, freeze_candidate_count))| {
                AdvisoryStabilityRow {
                    signal_name,
                    promote_candidate_count,
                    review_count,
                    freeze_candidate_count,
                }
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSnapshot, MarketSource, MarketStatus};

    use super::{
        choose_best_policy, run_confirmation_walkforward, AdvisoryStabilityRow,
        ConfirmationWalkForwardConfig,
    };
    use crate::{ConfirmationEraSplit, GeneratedSignalContext, PolicySweepResultRow};

    fn signal(
        id: &str,
        market_id: &str,
        signal_name: &str,
        confidence: f64,
        minutes_after: i64,
    ) -> GeneratedSignalContext {
        GeneratedSignalContext {
            signal_id: id.into(),
            market_id: market_id.into(),
            signal_name: signal_name.into(),
            direction: MarketSignalDirection::Yes,
            source: MarketSource::Synthetic,
            confidence,
            generated_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap()
                + Duration::minutes(minutes_after),
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
    fn policy_selection_is_deterministic() {
        let chosen = choose_best_policy(&[
            PolicySweepResultRow {
                confidence_threshold: 0.7,
                horizon_seconds: 7200,
                total_signals: 4,
                confirmed_signals: 2,
                acceptance_rate: 0.5,
                favorable_rate: 0.5,
                unfavorable_rate: 0.0,
                average_delta_probability: None,
                uplift_vs_baseline: Some(0.2),
                by_signal_name: Vec::new(),
                by_direction: Vec::new(),
                by_source: Vec::new(),
            },
            PolicySweepResultRow {
                confidence_threshold: 0.6,
                horizon_seconds: 3600,
                total_signals: 4,
                confirmed_signals: 3,
                acceptance_rate: 0.75,
                favorable_rate: 0.5,
                unfavorable_rate: 0.0,
                average_delta_probability: None,
                uplift_vs_baseline: Some(0.2),
                by_signal_name: Vec::new(),
                by_direction: Vec::new(),
                by_source: Vec::new(),
            },
        ])
        .unwrap();

        assert_eq!(chosen.confidence_threshold, 0.6);
        assert_eq!(chosen.horizon_seconds, 3600);
    }

    #[test]
    fn walkforward_report_is_stable() {
        let signals = vec![
            signal("sig-1", "market-1", "odds_jump", 0.4, 0),
            signal("sig-2", "market-2", "odds_jump", 0.9, 1),
            signal("sig-3", "market-3", "odds_jump", 0.4, 120),
            signal("sig-4", "market-4", "odds_jump", 0.9, 121),
        ];
        let snapshots = vec![
            snapshot("market-1", 0, 0.40),
            snapshot("market-1", 60, 0.38),
            snapshot("market-2", 1, 0.40),
            snapshot("market-2", 61, 0.50),
            snapshot("market-3", 120, 0.40),
            snapshot("market-3", 180, 0.39),
            snapshot("market-4", 121, 0.40),
            snapshot("market-4", 181, 0.49),
        ];
        let config = ConfirmationWalkForwardConfig {
            era_split: ConfirmationEraSplit::EraCount(2),
            confidence_thresholds: vec![0.5, 0.8],
            horizons: vec![3600],
            delta_threshold: 0.02,
            ..ConfirmationWalkForwardConfig::default()
        };

        let first = run_confirmation_walkforward(&signals, &snapshots, &config);
        let second = run_confirmation_walkforward(&signals, &snapshots, &config);

        assert_eq!(first, second);
        assert_eq!(first.steps.len(), 1);
        assert_eq!(first.steps[0].train_era_id, "era-001");
        assert_eq!(first.steps[0].validation_era_id, "era-002");
    }

    #[test]
    fn validation_uplift_is_computed_against_validation_baseline() {
        let signals = vec![
            signal("sig-1", "market-1", "odds_jump", 0.4, 0),
            signal("sig-2", "market-2", "odds_jump", 0.9, 1),
            signal("sig-3", "market-3", "odds_jump", 0.4, 120),
            signal("sig-4", "market-4", "odds_jump", 0.9, 121),
        ];
        let snapshots = vec![
            snapshot("market-1", 0, 0.40),
            snapshot("market-1", 60, 0.35),
            snapshot("market-2", 1, 0.40),
            snapshot("market-2", 61, 0.48),
            snapshot("market-3", 120, 0.40),
            snapshot("market-3", 180, 0.35),
            snapshot("market-4", 121, 0.40),
            snapshot("market-4", 181, 0.48),
        ];

        let report = run_confirmation_walkforward(
            &signals,
            &snapshots,
            &ConfirmationWalkForwardConfig {
                era_split: ConfirmationEraSplit::EraCount(2),
                confidence_thresholds: vec![0.5],
                horizons: vec![3600],
                delta_threshold: 0.02,
                ..ConfirmationWalkForwardConfig::default()
            },
        );

        assert_eq!(report.steps[0].validation_metrics.favorable_rate, 1.0);
        assert_eq!(
            report.steps[0].validation_metrics.uplift_vs_baseline,
            Some(0.5)
        );
    }

    #[test]
    fn advisory_summary_tracks_repeated_classifications() {
        let signals = vec![
            signal("sig-1", "market-1", "odds_jump", 0.9, 0),
            signal("sig-2", "market-2", "odds_jump", 0.9, 1),
            signal("sig-3", "market-3", "odds_jump", 0.9, 120),
            signal("sig-4", "market-4", "odds_jump", 0.9, 121),
            signal("sig-5", "market-5", "odds_jump", 0.9, 240),
            signal("sig-6", "market-6", "odds_jump", 0.9, 241),
        ];
        let snapshots = vec![
            snapshot("market-1", 0, 0.40),
            snapshot("market-1", 60, 0.48),
            snapshot("market-2", 1, 0.40),
            snapshot("market-2", 61, 0.49),
            snapshot("market-3", 120, 0.40),
            snapshot("market-3", 180, 0.48),
            snapshot("market-4", 121, 0.40),
            snapshot("market-4", 181, 0.49),
            snapshot("market-5", 240, 0.40),
            snapshot("market-5", 300, 0.48),
            snapshot("market-6", 241, 0.40),
            snapshot("market-6", 301, 0.49),
        ];
        let report = run_confirmation_walkforward(
            &signals,
            &snapshots,
            &ConfirmationWalkForwardConfig {
                era_split: ConfirmationEraSplit::EraCount(3),
                confidence_thresholds: vec![0.5],
                horizons: vec![3600],
                delta_threshold: 0.02,
                advisory_config: crate::ConfirmationPolicyAdvisoryConfig {
                    min_sample_size: 1,
                    ..crate::ConfirmationPolicyAdvisoryConfig::default()
                },
            },
        );

        assert_eq!(
            report.summary.advisory_stability,
            vec![AdvisoryStabilityRow {
                signal_name: "odds_jump".into(),
                promote_candidate_count: 2,
                review_count: 0,
                freeze_candidate_count: 0,
            }]
        );
    }
}
