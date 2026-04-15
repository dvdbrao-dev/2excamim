use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use market_domain::{MarketSignal, MarketSignalDirection, MarketSource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalPolicyStatus {
    Promoted,
    Review,
    Frozen,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalPolicyRule {
    pub signal_name: String,
    pub direction: Option<MarketSignalDirection>,
    pub source: Option<MarketSource>,
    pub status: SignalPolicyStatus,
    pub confidence_threshold: Option<f64>,
    pub horizon_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmationPolicyMetadata {
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub source_analysis: String,
    pub eras_analyzed: usize,
    pub selection_criteria_summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConfirmationPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ConfirmationPolicyMetadata>,
    pub rules: Vec<SignalPolicyRule>,
}

impl ConfirmationPolicy {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfirmationPolicyLoadError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let policy: Self = serde_json::from_reader(reader)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn matching_rule<'a>(&'a self, signal: &MarketSignal) -> Option<&'a SignalPolicyRule> {
        self.rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule_matches(rule, signal))
            .max_by(|(left_index, left), (right_index, right)| {
                rule_specificity(left)
                    .cmp(&rule_specificity(right))
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(_, rule)| rule)
    }

    fn validate(&self) -> Result<(), ConfirmationPolicyLoadError> {
        for rule in &self.rules {
            if rule.signal_name.trim().is_empty() {
                return Err(ConfirmationPolicyLoadError::invalid_data(
                    "signal_name cannot be empty",
                ));
            }
            if let Some(threshold) = rule.confidence_threshold {
                if !(0.0..=1.0).contains(&threshold) {
                    return Err(ConfirmationPolicyLoadError::invalid_data(
                        "confidence_threshold must be in [0,1]",
                    ));
                }
            }
            if let Some(horizon_seconds) = rule.horizon_seconds {
                if horizon_seconds < 0 {
                    return Err(ConfirmationPolicyLoadError::invalid_data(
                        "horizon_seconds must be >= 0",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum ConfirmationPolicyLoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidData(String),
}

impl ConfirmationPolicyLoadError {
    fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }
}

impl std::fmt::Display for ConfirmationPolicyLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::InvalidData(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ConfirmationPolicyLoadError {}

impl From<std::io::Error> for ConfirmationPolicyLoadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ConfirmationPolicyLoadError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

fn rule_matches(rule: &SignalPolicyRule, signal: &MarketSignal) -> bool {
    rule.signal_name == signal.signal_name
        && rule
            .direction
            .is_none_or(|direction| direction == signal.direction)
        && rule.source.is_none_or(|source| source == signal.source)
}

fn rule_specificity(rule: &SignalPolicyRule) -> usize {
    usize::from(rule.direction.is_some()) + usize::from(rule.source.is_some())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSource};

    use super::{ConfirmationPolicy, SignalPolicyRule, SignalPolicyStatus};

    fn signal() -> market_domain::MarketSignal {
        market_domain::MarketSignal::new(
            "sig-1",
            "market-1",
            MarketSource::Synthetic,
            "odds_jump",
            MarketSignalDirection::Yes,
            0.7,
            Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn chooses_most_specific_matching_rule() {
        let policy = ConfirmationPolicy {
            metadata: None,
            rules: vec![
                SignalPolicyRule {
                    signal_name: "odds_jump".into(),
                    direction: None,
                    source: None,
                    status: SignalPolicyStatus::Review,
                    confidence_threshold: None,
                    horizon_seconds: None,
                },
                SignalPolicyRule {
                    signal_name: "odds_jump".into(),
                    direction: Some(MarketSignalDirection::Yes),
                    source: Some(MarketSource::Synthetic),
                    status: SignalPolicyStatus::Promoted,
                    confidence_threshold: Some(0.4),
                    horizon_seconds: Some(3600),
                },
            ],
        };

        let rule = policy.matching_rule(&signal()).unwrap();

        assert_eq!(rule.status, SignalPolicyStatus::Promoted);
        assert_eq!(rule.confidence_threshold, Some(0.4));
    }
}
