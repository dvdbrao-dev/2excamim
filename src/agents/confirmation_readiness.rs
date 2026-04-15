use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chrono::{TimeZone, Utc};
use market_domain::{MarketSignalDirection, MarketSource};
use serde::{Deserialize, Serialize};

use crate::{
    ConfirmationPolicyLoadError, ConfirmationPolicyProposal, ConfirmationWalkForwardReport,
    GeneratedSignalContext, SignalPolicyStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationReadinessStatus {
    Experimental,
    Candidate,
    Promoted,
    Frozen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationReadinessConfig {
    pub min_sample_count: usize,
    pub min_candidate_eras: usize,
    pub min_promote_eras: usize,
    pub min_freeze_eras: usize,
    pub threshold_consistency_min_rate: f64,
    pub horizon_consistency_min_rate: f64,
}

impl Default for ConfirmationReadinessConfig {
    fn default() -> Self {
        Self {
            min_sample_count: 2,
            min_candidate_eras: 1,
            min_promote_eras: 2,
            min_freeze_eras: 2,
            threshold_consistency_min_rate: 0.75,
            horizon_consistency_min_rate: 0.75,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationReadinessEvidence {
    pub validation_steps: usize,
    pub confirmed_sample_count: usize,
    pub promote_candidate_count: usize,
    pub review_count: usize,
    pub freeze_candidate_count: usize,
    pub favorable_rate: f64,
    pub unfavorable_rate: f64,
    pub average_validation_uplift: f64,
    pub threshold_consistency_rate: f64,
    pub horizon_consistency_rate: f64,
    pub proposed_policy_status: Option<SignalPolicyStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationReadinessState {
    pub signal_name: String,
    pub direction: Option<MarketSignalDirection>,
    pub source: Option<MarketSource>,
    pub readiness_status: ConfirmationReadinessStatus,
    pub evidence: ConfirmationReadinessEvidence,
    pub last_updated_at: chrono::DateTime<Utc>,
    pub provenance: String,
    pub rationale_summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationReadinessBreakdownRow {
    pub key: String,
    pub total: usize,
    pub experimental: usize,
    pub candidate: usize,
    pub promoted: usize,
    pub frozen: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationReadinessSummary {
    pub total: usize,
    pub experimental: usize,
    pub candidate: usize,
    pub promoted: usize,
    pub frozen: usize,
    pub by_signal_name: Vec<ConfirmationReadinessBreakdownRow>,
    pub by_direction: Vec<ConfirmationReadinessBreakdownRow>,
    pub by_source: Vec<ConfirmationReadinessBreakdownRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationReadinessReport {
    pub generated_at: chrono::DateTime<Utc>,
    pub source_analysis: String,
    pub classification_rules: String,
    pub summary: ConfirmationReadinessSummary,
    pub states: Vec<ConfirmationReadinessState>,
}

pub fn materialize_confirmation_readiness(
    walkforward: &ConfirmationWalkForwardReport,
    generated_signals: &[GeneratedSignalContext],
    proposed_policy: Option<&ConfirmationPolicyProposal>,
    source_analysis: &str,
    config: &ConfirmationReadinessConfig,
) -> ConfirmationReadinessReport {
    let generated_at = walkforward
        .eras
        .last()
        .map(|era| era.end_time)
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap());
    let sample_counts = aggregate_validation_samples(walkforward);
    let rates = aggregate_validation_rates(walkforward);
    let advisory_counts = advisory_counts(walkforward);
    let metadata = family_metadata(generated_signals);
    let proposed_status = proposed_policy_statuses(proposed_policy);
    let signal_names = sample_counts
        .keys()
        .cloned()
        .chain(advisory_counts.keys().cloned())
        .chain(metadata.keys().cloned())
        .chain(proposed_status.keys().cloned())
        .collect::<BTreeSet<_>>();
    let threshold_consistency_rate = best_threshold_rate(walkforward);
    let horizon_consistency_rate = best_horizon_rate(walkforward);

    let mut states = signal_names
        .into_iter()
        .map(|signal_name| {
            let confirmed_sample_count = sample_counts.get(&signal_name).copied().unwrap_or(0);
            let (promote_candidate_count, review_count, freeze_candidate_count) = advisory_counts
                .get(&signal_name)
                .copied()
                .unwrap_or((0, 0, 0));
            let (favorable_rate, unfavorable_rate) =
                rates.get(&signal_name).copied().unwrap_or((0.0, 0.0));
            let readiness_status = classify_readiness(
                confirmed_sample_count,
                promote_candidate_count,
                freeze_candidate_count,
                threshold_consistency_rate,
                horizon_consistency_rate,
                config,
            );
            let (direction, source) = metadata.get(&signal_name).copied().unwrap_or((None, None));
            let policy_status = proposed_status.get(&signal_name).copied();
            let evidence = ConfirmationReadinessEvidence {
                validation_steps: walkforward.steps.len(),
                confirmed_sample_count,
                promote_candidate_count,
                review_count,
                freeze_candidate_count,
                favorable_rate,
                unfavorable_rate,
                average_validation_uplift: walkforward.summary.average_validation_uplift,
                threshold_consistency_rate,
                horizon_consistency_rate,
                proposed_policy_status: policy_status,
            };

            ConfirmationReadinessState {
                signal_name,
                direction,
                source,
                readiness_status,
                rationale_summary: rationale(readiness_status, &evidence, config),
                evidence,
                last_updated_at: generated_at,
                provenance: source_analysis.to_string(),
            }
        })
        .collect::<Vec<_>>();
    states.sort_by(|left, right| left.signal_name.cmp(&right.signal_name));

    ConfirmationReadinessReport {
        generated_at,
        source_analysis: source_analysis.to_string(),
        classification_rules: classification_rules(config),
        summary: summarize_readiness(&states),
        states,
    }
}

pub fn write_confirmation_readiness_report(
    report: &ConfirmationReadinessReport,
    output_path: impl AsRef<Path>,
) -> Result<(), ConfirmationPolicyLoadError> {
    let path = output_path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

pub fn read_confirmation_readiness_report(
    input_path: impl AsRef<Path>,
) -> Result<ConfirmationReadinessReport, ConfirmationPolicyLoadError> {
    let file = std::fs::File::open(input_path)?;
    Ok(serde_json::from_reader(file)?)
}

fn classify_readiness(
    confirmed_sample_count: usize,
    promote_candidate_count: usize,
    freeze_candidate_count: usize,
    threshold_consistency_rate: f64,
    horizon_consistency_rate: f64,
    config: &ConfirmationReadinessConfig,
) -> ConfirmationReadinessStatus {
    if confirmed_sample_count < config.min_sample_count {
        return ConfirmationReadinessStatus::Experimental;
    }
    if freeze_candidate_count >= config.min_freeze_eras
        && freeze_candidate_count > promote_candidate_count
    {
        return ConfirmationReadinessStatus::Frozen;
    }
    if promote_candidate_count >= config.min_promote_eras
        && promote_candidate_count > freeze_candidate_count
        && threshold_consistency_rate >= config.threshold_consistency_min_rate
        && horizon_consistency_rate >= config.horizon_consistency_min_rate
    {
        return ConfirmationReadinessStatus::Promoted;
    }
    if promote_candidate_count >= config.min_candidate_eras
        && promote_candidate_count > freeze_candidate_count
    {
        return ConfirmationReadinessStatus::Candidate;
    }
    ConfirmationReadinessStatus::Experimental
}

pub fn summarize_readiness(states: &[ConfirmationReadinessState]) -> ConfirmationReadinessSummary {
    ConfirmationReadinessSummary {
        total: states.len(),
        experimental: count_status(states, ConfirmationReadinessStatus::Experimental),
        candidate: count_status(states, ConfirmationReadinessStatus::Candidate),
        promoted: count_status(states, ConfirmationReadinessStatus::Promoted),
        frozen: count_status(states, ConfirmationReadinessStatus::Frozen),
        by_signal_name: breakdown_by(states, |state| state.signal_name.clone()),
        by_direction: breakdown_by(states, |state| {
            state
                .direction
                .map(|direction| format!("{direction:?}"))
                .unwrap_or_else(|| "unknown".into())
        }),
        by_source: breakdown_by(states, |state| {
            state
                .source
                .map(|source| format!("{source:?}"))
                .unwrap_or_else(|| "unknown".into())
        }),
    }
}

fn count_status(
    states: &[ConfirmationReadinessState],
    status: ConfirmationReadinessStatus,
) -> usize {
    states
        .iter()
        .filter(|state| state.readiness_status == status)
        .count()
}

fn breakdown_by<F>(
    states: &[ConfirmationReadinessState],
    key_fn: F,
) -> Vec<ConfirmationReadinessBreakdownRow>
where
    F: Fn(&ConfirmationReadinessState) -> String,
{
    let mut rows = BTreeMap::<String, ConfirmationReadinessBreakdownRow>::new();
    for state in states {
        let row = rows
            .entry(key_fn(state))
            .or_insert_with(|| ConfirmationReadinessBreakdownRow {
                key: key_fn(state),
                total: 0,
                experimental: 0,
                candidate: 0,
                promoted: 0,
                frozen: 0,
            });
        row.total += 1;
        match state.readiness_status {
            ConfirmationReadinessStatus::Experimental => row.experimental += 1,
            ConfirmationReadinessStatus::Candidate => row.candidate += 1,
            ConfirmationReadinessStatus::Promoted => row.promoted += 1,
            ConfirmationReadinessStatus::Frozen => row.frozen += 1,
        }
    }
    rows.into_values().collect()
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

fn aggregate_validation_rates(
    report: &ConfirmationWalkForwardReport,
) -> BTreeMap<String, (f64, f64)> {
    let mut weighted = BTreeMap::<String, (usize, f64, f64)>::new();
    for step in &report.steps {
        for row in &step.validation_metrics.by_signal_name {
            let entry = weighted.entry(row.key.clone()).or_insert((0, 0.0, 0.0));
            entry.0 += row.confirmed_signals;
            entry.1 += row.favorable_rate * row.confirmed_signals as f64;
            entry.2 += row.unfavorable_rate * row.confirmed_signals as f64;
        }
    }
    weighted
        .into_iter()
        .map(|(signal_name, (samples, favorable, unfavorable))| {
            let denominator = if samples == 0 { 1.0 } else { samples as f64 };
            (
                signal_name,
                (favorable / denominator, unfavorable / denominator),
            )
        })
        .collect()
}

fn advisory_counts(
    report: &ConfirmationWalkForwardReport,
) -> BTreeMap<String, (usize, usize, usize)> {
    report
        .summary
        .advisory_stability
        .iter()
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

fn family_metadata(
    generated_signals: &[GeneratedSignalContext],
) -> BTreeMap<String, (Option<MarketSignalDirection>, Option<MarketSource>)> {
    let mut directions = BTreeMap::<String, Vec<MarketSignalDirection>>::new();
    let mut sources = BTreeMap::<String, Vec<MarketSource>>::new();
    for signal in generated_signals {
        let direction_values = directions.entry(signal.signal_name.clone()).or_default();
        if !direction_values.contains(&signal.direction) {
            direction_values.push(signal.direction);
        }
        let source_values = sources.entry(signal.signal_name.clone()).or_default();
        if !source_values.contains(&signal.source) {
            source_values.push(signal.source);
        }
    }

    directions
        .keys()
        .cloned()
        .chain(sources.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|signal_name| {
            let direction = single_value(directions.get(&signal_name));
            let source = single_value(sources.get(&signal_name));
            (signal_name, (direction, source))
        })
        .collect()
}

fn single_value<T: Copy>(values: Option<&Vec<T>>) -> Option<T> {
    let values = values?;
    (values.len() == 1).then(|| values[0])
}

fn proposed_policy_statuses(
    proposed_policy: Option<&ConfirmationPolicyProposal>,
) -> BTreeMap<String, SignalPolicyStatus> {
    proposed_policy
        .map(|proposal| {
            proposal
                .policy
                .rules
                .iter()
                .map(|rule| (rule.signal_name.clone(), rule.status))
                .collect()
        })
        .unwrap_or_default()
}

fn best_threshold_rate(report: &ConfirmationWalkForwardReport) -> f64 {
    report
        .summary
        .chosen_thresholds
        .iter()
        .map(|row| row.rate)
        .max_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn best_horizon_rate(report: &ConfirmationWalkForwardReport) -> f64 {
    report
        .summary
        .chosen_horizons
        .iter()
        .map(|row| row.rate)
        .max_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn rationale(
    status: ConfirmationReadinessStatus,
    evidence: &ConfirmationReadinessEvidence,
    config: &ConfirmationReadinessConfig,
) -> String {
    match status {
        ConfirmationReadinessStatus::Promoted => format!(
            "promoted: promote_candidate_count={} min_promote_eras={} threshold_consistency_rate={:.4} horizon_consistency_rate={:.4}",
            evidence.promote_candidate_count,
            config.min_promote_eras,
            evidence.threshold_consistency_rate,
            evidence.horizon_consistency_rate
        ),
        ConfirmationReadinessStatus::Candidate => format!(
            "candidate: repeated strong evidence without promoted policy stability; promote_candidate_count={} min_candidate_eras={}",
            evidence.promote_candidate_count, config.min_candidate_eras
        ),
        ConfirmationReadinessStatus::Frozen => format!(
            "frozen: freeze_candidate_count={} min_freeze_eras={}",
            evidence.freeze_candidate_count, config.min_freeze_eras
        ),
        ConfirmationReadinessStatus::Experimental => {
            "experimental: insufficient or mixed evidence".to_string()
        }
    }
}

fn classification_rules(config: &ConfirmationReadinessConfig) -> String {
    format!(
        "frozen_if samples>={} and freeze_candidate_count>={} and freeze_candidate_count>promote_candidate_count; promoted_if samples>={} and promote_candidate_count>={} and promote_candidate_count>freeze_candidate_count and threshold_consistency_rate>={:.2} and horizon_consistency_rate>={:.2}; candidate_if samples>={} and promote_candidate_count>={} and promote_candidate_count>freeze_candidate_count; otherwise experimental",
        config.min_sample_count,
        config.min_freeze_eras,
        config.min_sample_count,
        config.min_promote_eras,
        config.threshold_consistency_min_rate,
        config.horizon_consistency_min_rate,
        config.min_sample_count,
        config.min_candidate_eras,
    )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSource};

    use super::{
        materialize_confirmation_readiness, read_confirmation_readiness_report,
        write_confirmation_readiness_report, ConfirmationReadinessConfig,
        ConfirmationReadinessStatus,
    };
    use crate::{
        AdvisoryStabilityRow, ConfirmationEra, ConfirmationPolicy, ConfirmationPolicyProposal,
        ConfirmationPolicyProposalSummary, ConfirmationWalkForwardReport,
        ConfirmationWalkForwardStep, ConfirmationWalkForwardSummary, GeneratedSignalContext,
        PolicyChoiceFrequency, PolicySweepBreakdownRow, PolicySweepResultRow, SignalPolicyRule,
        SignalPolicyStatus,
    };

    fn generated(signal_name: &str) -> GeneratedSignalContext {
        GeneratedSignalContext {
            signal_id: format!("sig-{signal_name}"),
            market_id: format!("market-{signal_name}"),
            signal_name: signal_name.into(),
            direction: MarketSignalDirection::Yes,
            source: MarketSource::Synthetic,
            confidence: 0.8,
            generated_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
        }
    }

    fn report(
        advisory_stability: Vec<AdvisoryStabilityRow>,
        samples: Vec<(&str, usize, f64, f64)>,
        threshold_rate: f64,
        horizon_rate: f64,
    ) -> ConfirmationWalkForwardReport {
        ConfirmationWalkForwardReport {
            eras: vec![ConfirmationEra {
                era_id: "era-001".into(),
                start_time: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
                end_time: Utc.with_ymd_and_hms(2026, 4, 13, 13, 0, 0).unwrap(),
                label: None,
            }],
            steps: vec![ConfirmationWalkForwardStep {
                train_era_id: "era-001".into(),
                validation_era_id: "era-002".into(),
                chosen_confidence_threshold: 0.5,
                chosen_horizon_seconds: 3600,
                train_metrics: row(&samples),
                validation_metrics: row(&samples),
                confirmed_sample_count_validation: samples
                    .iter()
                    .map(|(_, count, _, _)| count)
                    .sum(),
                validation_advisory: Vec::new(),
            }],
            summary: ConfirmationWalkForwardSummary {
                average_validation_favorable_rate: 0.7,
                average_validation_uplift: 0.2,
                chosen_thresholds: vec![PolicyChoiceFrequency {
                    value: 0.5,
                    count: 1,
                    rate: threshold_rate,
                }],
                chosen_horizons: vec![PolicyChoiceFrequency {
                    value: 3600,
                    count: 1,
                    rate: horizon_rate,
                }],
                advisory_stability,
            },
        }
    }

    fn row(samples: &[(&str, usize, f64, f64)]) -> PolicySweepResultRow {
        PolicySweepResultRow {
            confidence_threshold: 0.5,
            horizon_seconds: 3600,
            total_signals: samples.iter().map(|(_, count, _, _)| count).sum(),
            confirmed_signals: samples.iter().map(|(_, count, _, _)| count).sum(),
            acceptance_rate: 1.0,
            favorable_rate: 1.0,
            unfavorable_rate: 0.0,
            average_delta_probability: Some(0.1),
            uplift_vs_baseline: Some(0.2),
            by_signal_name: samples
                .iter()
                .map(
                    |(signal_name, confirmed_signals, favorable_rate, unfavorable_rate)| {
                        PolicySweepBreakdownRow {
                            key: (*signal_name).into(),
                            total_signals: *confirmed_signals,
                            confirmed_signals: *confirmed_signals,
                            favorable_rate: *favorable_rate,
                            unfavorable_rate: *unfavorable_rate,
                        }
                    },
                )
                .collect(),
            by_direction: Vec::new(),
            by_source: Vec::new(),
        }
    }

    #[test]
    fn deterministic_readiness_classification() {
        let walkforward = report(
            vec![AdvisoryStabilityRow {
                signal_name: "odds_jump".into(),
                promote_candidate_count: 2,
                review_count: 0,
                freeze_candidate_count: 0,
            }],
            vec![("odds_jump", 4, 0.8, 0.0)],
            1.0,
            1.0,
        );
        let generated = vec![generated("odds_jump")];

        let first = materialize_confirmation_readiness(
            &walkforward,
            &generated,
            None,
            "test",
            &ConfirmationReadinessConfig::default(),
        );
        let second = materialize_confirmation_readiness(
            &walkforward,
            &generated,
            None,
            "test",
            &ConfirmationReadinessConfig::default(),
        );

        assert_eq!(first, second);
    }

    #[test]
    fn repeated_strong_families_become_candidate_or_promoted() {
        let promoted = materialize_confirmation_readiness(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "odds_jump".into(),
                    promote_candidate_count: 2,
                    review_count: 0,
                    freeze_candidate_count: 0,
                }],
                vec![("odds_jump", 4, 0.8, 0.0)],
                1.0,
                1.0,
            ),
            &[generated("odds_jump")],
            None,
            "test",
            &ConfirmationReadinessConfig::default(),
        );
        let candidate = materialize_confirmation_readiness(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "activity_spike".into(),
                    promote_candidate_count: 1,
                    review_count: 1,
                    freeze_candidate_count: 0,
                }],
                vec![("activity_spike", 3, 0.7, 0.1)],
                0.5,
                1.0,
            ),
            &[generated("activity_spike")],
            None,
            "test",
            &ConfirmationReadinessConfig::default(),
        );

        assert_eq!(
            promoted.states[0].readiness_status,
            ConfirmationReadinessStatus::Promoted
        );
        assert_eq!(
            candidate.states[0].readiness_status,
            ConfirmationReadinessStatus::Candidate
        );
    }

    #[test]
    fn repeated_weak_families_become_frozen() {
        let readiness = materialize_confirmation_readiness(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "mean_reversion".into(),
                    promote_candidate_count: 0,
                    review_count: 0,
                    freeze_candidate_count: 2,
                }],
                vec![("mean_reversion", 4, 0.1, 0.8)],
                1.0,
                1.0,
            ),
            &[generated("mean_reversion")],
            None,
            "test",
            &ConfirmationReadinessConfig::default(),
        );

        assert_eq!(
            readiness.states[0].readiness_status,
            ConfirmationReadinessStatus::Frozen
        );
    }

    #[test]
    fn mixed_evidence_remains_experimental() {
        let readiness = materialize_confirmation_readiness(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "activity_spike".into(),
                    promote_candidate_count: 1,
                    review_count: 1,
                    freeze_candidate_count: 1,
                }],
                vec![("activity_spike", 4, 0.4, 0.4)],
                1.0,
                1.0,
            ),
            &[generated("activity_spike")],
            None,
            "test",
            &ConfirmationReadinessConfig::default(),
        );

        assert_eq!(
            readiness.states[0].readiness_status,
            ConfirmationReadinessStatus::Experimental
        );
    }

    #[test]
    fn json_persistence_shape_and_reloadability() {
        let proposal = ConfirmationPolicyProposal {
            policy: ConfirmationPolicy {
                metadata: None,
                rules: vec![SignalPolicyRule {
                    signal_name: "odds_jump".into(),
                    direction: None,
                    source: None,
                    status: SignalPolicyStatus::Promoted,
                    confidence_threshold: Some(0.5),
                    horizon_seconds: Some(3600),
                }],
            },
            summary: ConfirmationPolicyProposalSummary {
                promoted_rules: 1,
                review_rules: 0,
                frozen_rules: 0,
            },
        };
        let readiness = materialize_confirmation_readiness(
            &report(
                vec![AdvisoryStabilityRow {
                    signal_name: "odds_jump".into(),
                    promote_candidate_count: 2,
                    review_count: 0,
                    freeze_candidate_count: 0,
                }],
                vec![("odds_jump", 4, 0.8, 0.0)],
                1.0,
                1.0,
            ),
            &[generated("odds_jump")],
            Some(&proposal),
            "test",
            &ConfirmationReadinessConfig::default(),
        );
        let path = std::env::temp_dir().join("twoexcamim-confirmation-readiness-test.json");

        write_confirmation_readiness_report(&readiness, &path).unwrap();
        let loaded = read_confirmation_readiness_report(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();

        assert_eq!(loaded, readiness);
        assert_eq!(value["summary"]["promoted"], 1);
        assert_eq!(
            value["states"][0]["readiness_status"],
            serde_json::json!("promoted")
        );
        assert_eq!(
            value["states"][0]["evidence"]["proposed_policy_status"],
            serde_json::json!("promoted")
        );
        let _ = std::fs::remove_file(path);
    }
}
