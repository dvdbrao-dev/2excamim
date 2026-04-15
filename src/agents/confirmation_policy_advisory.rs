#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationPolicyAdvisoryClassification {
    PromoteCandidate,
    Review,
    FreezeCandidate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdvisorySignalFamilyMetrics {
    pub signal_name: String,
    pub sample_count: usize,
    pub favorable_rate: f64,
    pub unfavorable_rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationPolicyAdvisory {
    pub signal_name: String,
    pub sample_count: usize,
    pub favorable_rate: f64,
    pub unfavorable_rate: f64,
    pub classification: ConfirmationPolicyAdvisoryClassification,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationPolicyAdvisoryConfig {
    pub min_sample_size: usize,
    pub promote_min_favorable_rate: f64,
    pub freeze_min_unfavorable_rate: f64,
}

impl Default for ConfirmationPolicyAdvisoryConfig {
    fn default() -> Self {
        Self {
            min_sample_size: 3,
            promote_min_favorable_rate: 0.6,
            freeze_min_unfavorable_rate: 0.5,
        }
    }
}

pub fn advise_signal_families(
    metrics: &[AdvisorySignalFamilyMetrics],
    config: &ConfirmationPolicyAdvisoryConfig,
) -> Vec<ConfirmationPolicyAdvisory> {
    let mut advisories = metrics
        .iter()
        .map(|metric| ConfirmationPolicyAdvisory {
            signal_name: metric.signal_name.clone(),
            sample_count: metric.sample_count,
            favorable_rate: metric.favorable_rate,
            unfavorable_rate: metric.unfavorable_rate,
            classification: if metric.sample_count >= config.min_sample_size
                && metric.favorable_rate >= config.promote_min_favorable_rate
            {
                ConfirmationPolicyAdvisoryClassification::PromoteCandidate
            } else if metric.sample_count >= config.min_sample_size
                && metric.unfavorable_rate >= config.freeze_min_unfavorable_rate
            {
                ConfirmationPolicyAdvisoryClassification::FreezeCandidate
            } else {
                ConfirmationPolicyAdvisoryClassification::Review
            },
        })
        .collect::<Vec<_>>();
    advisories.sort_by(|left, right| left.signal_name.cmp(&right.signal_name));
    advisories
}

#[cfg(test)]
mod tests {
    use super::{
        advise_signal_families, AdvisorySignalFamilyMetrics,
        ConfirmationPolicyAdvisoryClassification, ConfirmationPolicyAdvisoryConfig,
    };

    #[test]
    fn advisory_classifies_signal_families() {
        let advisories = advise_signal_families(
            &[
                AdvisorySignalFamilyMetrics {
                    signal_name: "activity_spike".into(),
                    sample_count: 5,
                    favorable_rate: 0.7,
                    unfavorable_rate: 0.1,
                },
                AdvisorySignalFamilyMetrics {
                    signal_name: "mean_reversion".into(),
                    sample_count: 5,
                    favorable_rate: 0.2,
                    unfavorable_rate: 0.6,
                },
                AdvisorySignalFamilyMetrics {
                    signal_name: "odds_jump".into(),
                    sample_count: 2,
                    favorable_rate: 1.0,
                    unfavorable_rate: 0.0,
                },
            ],
            &ConfirmationPolicyAdvisoryConfig::default(),
        );

        assert_eq!(
            advisories[0].classification,
            ConfirmationPolicyAdvisoryClassification::PromoteCandidate
        );
        assert_eq!(
            advisories[1].classification,
            ConfirmationPolicyAdvisoryClassification::FreezeCandidate
        );
        assert_eq!(
            advisories[2].classification,
            ConfirmationPolicyAdvisoryClassification::Review
        );
    }
}
