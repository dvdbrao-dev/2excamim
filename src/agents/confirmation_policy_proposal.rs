use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::{TimeZone, Utc};

use crate::{
    AdvisoryStabilityRow, ConfirmationPolicy, ConfirmationPolicyMetadata,
    ConfirmationWalkForwardReport, SignalPolicyRule, SignalPolicyStatus,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationPolicyProposalConfig {
    pub min_promote_eras: usize,
    pub min_freeze_eras: usize,
    pub min_sample_count: usize,
    pub threshold_consistency_min_rate: f64,
    pub horizon_consistency_min_rate: f64,
}

impl Default for ConfirmationPolicyProposalConfig {
    fn default() -> Self {
        Self {
            min_promote_eras: 2,
            min_freeze_eras: 2,
            min_sample_count: 2,
            threshold_consistency_min_rate: 0.75,
            horizon_consistency_min_rate: 0.75,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationPolicyProposalSummary {
    pub promoted_rules: usize,
    pub review_rules: usize,
    pub frozen_rules: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationPolicyProposal {
    pub policy: ConfirmationPolicy,
    pub summary: ConfirmationPolicyProposalSummary,
}

pub fn propose_confirmation_policy(
    report: &ConfirmationWalkForwardReport,
    source_analysis: &str,
    config: &ConfirmationPolicyProposalConfig,
) -> ConfirmationPolicyProposal {
    let sample_counts = aggregate_validation_samples(report);
    let advisory_map = advisory_map(&report.summary.advisory_stability);
    let consistent_threshold = consistent_threshold(report, config.threshold_consistency_min_rate);
    let consistent_horizon = consistent_horizon(report, config.horizon_consistency_min_rate);

    let signal_names = sample_counts
        .keys()
        .cloned()
        .chain(advisory_map.keys().cloned())
        .collect::<BTreeSet<_>>();

    let mut rules = signal_names
        .into_iter()
        .map(|signal_name| {
            let samples = sample_counts.get(&signal_name).copied().unwrap_or(0);
            let counts = advisory_map
                .get(&signal_name)
                .cloned()
                .unwrap_or((0usize, 0usize, 0usize));
            let status = classify_signal_family(samples, counts, config);

            SignalPolicyRule {
                signal_name,
                direction: None,
                source: None,
                status,
                confidence_threshold: (status == SignalPolicyStatus::Promoted)
                    .then_some(consistent_threshold)
                    .flatten(),
                horizon_seconds: (status == SignalPolicyStatus::Promoted)
                    .then_some(consistent_horizon)
                    .flatten(),
            }
        })
        .collect::<Vec<_>>();
    rules.sort_by(|left, right| left.signal_name.cmp(&right.signal_name));

    let metadata = Some(ConfirmationPolicyMetadata {
        generated_at: report
            .eras
            .last()
            .map(|era| era.end_time)
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap()),
        source_analysis: source_analysis.to_string(),
        eras_analyzed: report.eras.len(),
        selection_criteria_summary: format!(
            "promote_if_eras>={}; freeze_if_eras>={}; min_sample_count={}; threshold_consistency_min_rate={:.2}; horizon_consistency_min_rate={:.2}",
            config.min_promote_eras,
            config.min_freeze_eras,
            config.min_sample_count,
            config.threshold_consistency_min_rate,
            config.horizon_consistency_min_rate
        ),
    });

    let policy = ConfirmationPolicy { metadata, rules };
    let summary = ConfirmationPolicyProposalSummary {
        promoted_rules: policy
            .rules
            .iter()
            .filter(|rule| rule.status == SignalPolicyStatus::Promoted)
            .count(),
        review_rules: policy
            .rules
            .iter()
            .filter(|rule| rule.status == SignalPolicyStatus::Review)
            .count(),
        frozen_rules: policy
            .rules
            .iter()
            .filter(|rule| rule.status == SignalPolicyStatus::Frozen)
            .count(),
    };

    ConfirmationPolicyProposal { policy, summary }
}

pub fn write_confirmation_policy_proposal(
    policy: &ConfirmationPolicy,
    output_path: impl AsRef<Path>,
) -> Result<(), crate::ConfirmationPolicyLoadError> {
    let path = output_path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, policy)?;
    Ok(())
}

fn aggregate_validation_samples(report: &ConfirmationWalkForwardReport) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for step in &report.steps {
        for row in &step.validation_metrics.by_signal_name {
            *counts.entry(row.key.clone()).or_insert(0usize) += row.confirmed_signals;
        }
    }
    counts
}

fn advisory_map(rows: &[AdvisoryStabilityRow]) -> BTreeMap<String, (usize, usize, usize)> {
    rows.iter()
        .map(|row| {
            (
                row.signal_name.clone(),
                (
                    row.promote_candidate_count,
                    row.review_count,
                    row.freeze_candidate_count,
                ),
            )
        })
        .collect()
}

fn classify_signal_family(
    sample_count: usize,
    advisory_counts: (usize, usize, usize),
    config: &ConfirmationPolicyProposalConfig,
) -> SignalPolicyStatus {
    let (promote_count, _review_count, freeze_count) = advisory_counts;

    if sample_count >= config.min_sample_count
        && promote_count >= config.min_promote_eras
        && promote_count > freeze_count
    {
        SignalPolicyStatus::Promoted
    } else if sample_count >= config.min_sample_count
        && freeze_count >= config.min_freeze_eras
        && freeze_count > promote_count
    {
        SignalPolicyStatus::Frozen
    } else {
        SignalPolicyStatus::Review
    }
}

fn consistent_threshold(report: &ConfirmationWalkForwardReport, min_rate: f64) -> Option<f64> {
    let best = report
        .summary
        .chosen_thresholds
        .iter()
        .max_by(|left, right| {
            left.rate
                .total_cmp(&right.rate)
                .then_with(|| left.value.total_cmp(&right.value))
        })?;
    (best.rate >= min_rate).then_some(best.value)
}

fn consistent_horizon(report: &ConfirmationWalkForwardReport, min_rate: f64) -> Option<i64> {
    let best = report
        .summary
        .chosen_horizons
        .iter()
        .max_by(|left, right| {
            left.rate
                .total_cmp(&right.rate)
                .then_with(|| left.value.cmp(&right.value))
        })?;
    (best.rate >= min_rate).then_some(best.value)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::{
        propose_confirmation_policy, write_confirmation_policy_proposal,
        ConfirmationPolicyProposalConfig,
    };
    use crate::{
        AdvisoryStabilityRow, ConfirmationEra, ConfirmationPolicyLoadError,
        ConfirmationWalkForwardReport, ConfirmationWalkForwardStep, ConfirmationWalkForwardSummary,
        PolicyChoiceFrequency, PolicySweepResultRow, SignalPolicyStatus,
    };

    fn report(
        advisory_stability: Vec<AdvisoryStabilityRow>,
        thresholds: Vec<PolicyChoiceFrequency<f64>>,
        signal_name: &str,
        sample_count: usize,
    ) -> ConfirmationWalkForwardReport {
        report_with_samples(
            advisory_stability,
            thresholds,
            vec![(signal_name, sample_count)],
        )
    }

    fn report_with_samples(
        advisory_stability: Vec<AdvisoryStabilityRow>,
        thresholds: Vec<PolicyChoiceFrequency<f64>>,
        samples: Vec<(&str, usize)>,
    ) -> ConfirmationWalkForwardReport {
        ConfirmationWalkForwardReport {
            eras: vec![
                ConfirmationEra {
                    era_id: "era-001".into(),
                    start_time: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
                    end_time: Utc.with_ymd_and_hms(2026, 4, 13, 13, 0, 0).unwrap(),
                    label: None,
                },
                ConfirmationEra {
                    era_id: "era-002".into(),
                    start_time: Utc.with_ymd_and_hms(2026, 4, 13, 14, 0, 0).unwrap(),
                    end_time: Utc.with_ymd_and_hms(2026, 4, 13, 15, 0, 0).unwrap(),
                    label: None,
                },
                ConfirmationEra {
                    era_id: "era-003".into(),
                    start_time: Utc.with_ymd_and_hms(2026, 4, 13, 16, 0, 0).unwrap(),
                    end_time: Utc.with_ymd_and_hms(2026, 4, 13, 17, 0, 0).unwrap(),
                    label: None,
                },
            ],
            steps: vec![
                ConfirmationWalkForwardStep {
                    train_era_id: "era-001".into(),
                    validation_era_id: "era-002".into(),
                    chosen_confidence_threshold: 0.5,
                    chosen_horizon_seconds: 3600,
                    train_metrics: multi_row(&samples),
                    validation_metrics: multi_row(&samples),
                    confirmed_sample_count_validation: samples.iter().map(|(_, count)| count).sum(),
                    validation_advisory: Vec::new(),
                },
                ConfirmationWalkForwardStep {
                    train_era_id: "era-002".into(),
                    validation_era_id: "era-003".into(),
                    chosen_confidence_threshold: 0.5,
                    chosen_horizon_seconds: 3600,
                    train_metrics: multi_row(&samples),
                    validation_metrics: multi_row(&samples),
                    confirmed_sample_count_validation: samples.iter().map(|(_, count)| count).sum(),
                    validation_advisory: Vec::new(),
                },
            ],
            summary: ConfirmationWalkForwardSummary {
                average_validation_favorable_rate: 1.0,
                average_validation_uplift: 0.5,
                chosen_thresholds: thresholds,
                chosen_horizons: vec![PolicyChoiceFrequency {
                    value: 3600,
                    count: 2,
                    rate: 1.0,
                }],
                advisory_stability,
            },
        }
    }

    fn multi_row(samples: &[(&str, usize)]) -> crate::PolicySweepResultRow {
        let confirmed_signals = samples.iter().map(|(_, count)| count).sum();
        PolicySweepResultRow {
            confidence_threshold: 0.5,
            horizon_seconds: 3600,
            total_signals: confirmed_signals,
            confirmed_signals,
            acceptance_rate: 1.0,
            favorable_rate: 1.0,
            unfavorable_rate: 0.0,
            average_delta_probability: Some(0.1),
            uplift_vs_baseline: Some(0.5),
            by_signal_name: samples
                .iter()
                .map(
                    |(signal_name, confirmed_signals)| crate::PolicySweepBreakdownRow {
                        key: (*signal_name).into(),
                        total_signals: *confirmed_signals,
                        confirmed_signals: *confirmed_signals,
                        favorable_rate: 1.0,
                        unfavorable_rate: 0.0,
                    },
                )
                .collect(),
            by_direction: Vec::new(),
            by_source: Vec::new(),
        }
    }

    #[test]
    fn proposal_generation_is_deterministic() {
        let report = report(
            vec![AdvisoryStabilityRow {
                signal_name: "odds_jump".into(),
                promote_candidate_count: 2,
                review_count: 0,
                freeze_candidate_count: 0,
            }],
            vec![PolicyChoiceFrequency {
                value: 0.5,
                count: 2,
                rate: 1.0,
            }],
            "odds_jump",
            4,
        );

        let first = propose_confirmation_policy(
            &report,
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );
        let second = propose_confirmation_policy(
            &report,
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );

        assert_eq!(first, second);
    }

    #[test]
    fn repeated_strong_families_become_promoted() {
        let proposal = propose_confirmation_policy(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "odds_jump".into(),
                    promote_candidate_count: 2,
                    review_count: 0,
                    freeze_candidate_count: 0,
                }],
                vec![PolicyChoiceFrequency {
                    value: 0.5,
                    count: 2,
                    rate: 1.0,
                }],
                "odds_jump",
                4,
            ),
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );

        assert_eq!(
            proposal.policy.rules[0].status,
            SignalPolicyStatus::Promoted
        );
        assert_eq!(proposal.policy.rules[0].confidence_threshold, Some(0.5));
        assert_eq!(proposal.policy.rules[0].horizon_seconds, Some(3600));
    }

    #[test]
    fn repeated_weak_families_become_frozen() {
        let proposal = propose_confirmation_policy(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "mean_reversion".into(),
                    promote_candidate_count: 0,
                    review_count: 0,
                    freeze_candidate_count: 2,
                }],
                vec![PolicyChoiceFrequency {
                    value: 0.5,
                    count: 2,
                    rate: 1.0,
                }],
                "mean_reversion",
                4,
            ),
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );

        assert_eq!(proposal.policy.rules[0].status, SignalPolicyStatus::Frozen);
        assert_eq!(proposal.policy.rules[0].confidence_threshold, None);
    }

    #[test]
    fn ambiguous_families_remain_review() {
        let proposal = propose_confirmation_policy(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "activity_spike".into(),
                    promote_candidate_count: 1,
                    review_count: 1,
                    freeze_candidate_count: 1,
                }],
                vec![PolicyChoiceFrequency {
                    value: 0.5,
                    count: 1,
                    rate: 0.5,
                }],
                "activity_spike",
                2,
            ),
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );

        assert_eq!(proposal.policy.rules[0].status, SignalPolicyStatus::Review);
        assert_eq!(proposal.policy.rules[0].confidence_threshold, None);
        assert_eq!(proposal.policy.rules[0].horizon_seconds, None);
    }

    #[test]
    fn promoted_rules_require_consistent_horizon_to_export_it() {
        let mut report = report(
            vec![AdvisoryStabilityRow {
                signal_name: "odds_jump".into(),
                promote_candidate_count: 2,
                review_count: 0,
                freeze_candidate_count: 0,
            }],
            vec![PolicyChoiceFrequency {
                value: 0.5,
                count: 2,
                rate: 1.0,
            }],
            "odds_jump",
            4,
        );
        report.summary.chosen_horizons = vec![
            PolicyChoiceFrequency {
                value: 3600,
                count: 1,
                rate: 0.5,
            },
            PolicyChoiceFrequency {
                value: 7200,
                count: 1,
                rate: 0.5,
            },
        ];

        let proposal = propose_confirmation_policy(
            &report,
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );

        assert_eq!(
            proposal.policy.rules[0].status,
            SignalPolicyStatus::Promoted
        );
        assert_eq!(proposal.policy.rules[0].confidence_threshold, Some(0.5));
        assert_eq!(proposal.policy.rules[0].horizon_seconds, None);
    }

    #[test]
    fn proposal_summary_counts_all_statuses_and_sorts_rules() {
        let proposal = propose_confirmation_policy(
            &report_with_samples(
                vec![
                    AdvisoryStabilityRow {
                        signal_name: "mean_reversion".into(),
                        promote_candidate_count: 0,
                        review_count: 0,
                        freeze_candidate_count: 2,
                    },
                    AdvisoryStabilityRow {
                        signal_name: "activity_spike".into(),
                        promote_candidate_count: 1,
                        review_count: 1,
                        freeze_candidate_count: 0,
                    },
                    AdvisoryStabilityRow {
                        signal_name: "odds_jump".into(),
                        promote_candidate_count: 2,
                        review_count: 0,
                        freeze_candidate_count: 0,
                    },
                ],
                vec![PolicyChoiceFrequency {
                    value: 0.5,
                    count: 2,
                    rate: 1.0,
                }],
                vec![
                    ("mean_reversion", 4),
                    ("activity_spike", 4),
                    ("odds_jump", 4),
                ],
            ),
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );

        let signal_names = proposal
            .policy
            .rules
            .iter()
            .map(|rule| rule.signal_name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            signal_names,
            vec!["activity_spike", "mean_reversion", "odds_jump"]
        );
        assert_eq!(proposal.summary.promoted_rules, 1);
        assert_eq!(proposal.summary.review_rules, 1);
        assert_eq!(proposal.summary.frozen_rules, 1);
    }

    #[test]
    fn exported_json_loads_with_policy_loader() -> Result<(), ConfirmationPolicyLoadError> {
        let proposal = propose_confirmation_policy(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "odds_jump".into(),
                    promote_candidate_count: 2,
                    review_count: 0,
                    freeze_candidate_count: 0,
                }],
                vec![PolicyChoiceFrequency {
                    value: 0.5,
                    count: 2,
                    rate: 1.0,
                }],
                "odds_jump",
                4,
            ),
            "propose-confirmation-policy",
            &ConfirmationPolicyProposalConfig::default(),
        );
        let path = std::env::temp_dir().join(format!(
            "twoexcamim-proposed-policy-{}.json",
            Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0)
                .unwrap()
                .checked_add_signed(Duration::seconds(1))
                .unwrap()
                .timestamp()
        ));

        write_confirmation_policy_proposal(&proposal.policy, &path)?;
        let loaded = crate::ConfirmationPolicy::from_file(&path)?;

        assert_eq!(loaded, proposal.policy);
        let _ = std::fs::remove_file(path);
        Ok(())
    }
}
