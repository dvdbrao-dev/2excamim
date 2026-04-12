use chrono::Duration;
use market_domain::{MarketActivity, MarketSignal, MarketSignalDirection, Validate};

use crate::{odds_jump::signal_id, DetectorError};

/// Configuration for the deterministic activity-spike detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivitySpikeDetectorConfig {
    /// Lookback duration for the recent activity window in seconds.
    pub recent_window_secs: i64,
    /// Lookback duration for the baseline activity window in seconds.
    pub baseline_window_secs: i64,
    /// Minimum number of baseline records required before detection can run.
    pub min_baseline_points: usize,
    /// Minimum ratio of recent rate to baseline rate required to emit a signal.
    pub spike_ratio_threshold: u32,
}

impl ActivitySpikeDetectorConfig {
    /// Creates a new validated detector configuration.
    pub fn new(
        recent_window_secs: i64,
        baseline_window_secs: i64,
        min_baseline_points: usize,
        spike_ratio_threshold: u32,
    ) -> Result<Self, DetectorError> {
        let config = Self {
            recent_window_secs,
            baseline_window_secs,
            min_baseline_points,
            spike_ratio_threshold,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), DetectorError> {
        if self.recent_window_secs <= 0 {
            return Err(DetectorError::invalid_config(
                "recent_window_secs must be > 0",
            ));
        }
        if self.baseline_window_secs <= self.recent_window_secs {
            return Err(DetectorError::invalid_config(
                "baseline_window_secs must be greater than recent_window_secs",
            ));
        }
        if self.min_baseline_points == 0 {
            return Err(DetectorError::invalid_config(
                "min_baseline_points must be > 0",
            ));
        }
        if self.spike_ratio_threshold < 2 {
            return Err(DetectorError::invalid_config(
                "spike_ratio_threshold must be at least 2",
            ));
        }

        Ok(())
    }
}

impl Default for ActivitySpikeDetectorConfig {
    fn default() -> Self {
        Self {
            recent_window_secs: 300,
            baseline_window_secs: 1800,
            min_baseline_points: 3,
            spike_ratio_threshold: 2,
        }
    }
}

/// Deterministic detector for unusual bursts of recent activity.
pub struct ActivitySpikeDetector {
    config: ActivitySpikeDetectorConfig,
}

impl ActivitySpikeDetector {
    /// Creates a detector from validated configuration.
    pub fn new(config: ActivitySpikeDetectorConfig) -> Result<Self, DetectorError> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Evaluates canonical activity history and emits one signal when recent activity spikes.
    pub fn detect(
        &self,
        history: &[MarketActivity],
    ) -> Result<Option<MarketSignal>, DetectorError> {
        if history.is_empty() {
            return Ok(None);
        }

        let mut activity = history.to_vec();
        activity.sort_by_key(|item| item.observed_at);

        let latest = activity.last().expect("history length checked");
        validate_history(&activity)?;

        let recent_start = latest.observed_at - Duration::seconds(self.config.recent_window_secs);
        let baseline_start =
            latest.observed_at - Duration::seconds(self.config.baseline_window_secs);

        let recent_count = activity
            .iter()
            .filter(|item| item.observed_at > recent_start)
            .count();
        let baseline_count = activity
            .iter()
            .filter(|item| item.observed_at > baseline_start && item.observed_at <= recent_start)
            .count();

        if baseline_count < self.config.min_baseline_points {
            return Ok(None);
        }

        let baseline_secs = self.config.baseline_window_secs - self.config.recent_window_secs;
        let recent_rate = recent_count as f64 / self.config.recent_window_secs as f64;
        let baseline_rate = baseline_count as f64 / baseline_secs as f64;

        if baseline_rate <= 0.0 {
            return Ok(None);
        }

        let ratio = recent_rate / baseline_rate;
        if ratio < self.config.spike_ratio_threshold as f64 {
            return Ok(None);
        }

        let confidence = (ratio / self.config.spike_ratio_threshold as f64).min(1.0);
        let mut signal = MarketSignal::new(
            signal_id(
                "activity-spike",
                &latest.market_id,
                latest.observed_at,
                MarketSignalDirection::Yes,
            ),
            latest.market_id.clone(),
            latest.source,
            "activity_spike",
            MarketSignalDirection::Yes,
            confidence,
            latest.observed_at,
        )
        .map_err(|error| DetectorError::invalid_input(error.to_string()))?;
        signal.rationale = Some(evidence_rationale(
            recent_count,
            baseline_count,
            self.config.recent_window_secs,
            baseline_secs,
            ratio,
        ));
        signal
            .validate()
            .map_err(|error| DetectorError::invalid_input(error.to_string()))?;

        Ok(Some(signal))
    }
}

fn validate_history(history: &[MarketActivity]) -> Result<(), DetectorError> {
    let first = &history[0];
    for item in history {
        item.validate()
            .map_err(|error| DetectorError::invalid_input(error.to_string()))?;
        if item.market_id != first.market_id {
            return Err(DetectorError::invalid_input(
                "activity history must belong to one market_id",
            ));
        }
        if item.source != first.source {
            return Err(DetectorError::invalid_input(
                "activity history must belong to one source",
            ));
        }
    }

    Ok(())
}

fn evidence_rationale(
    recent_count: usize,
    baseline_count: usize,
    recent_window_secs: i64,
    baseline_window_secs: i64,
    ratio: f64,
) -> String {
    format!(
        "{{\"detector\":\"activity_spike\",\"recent_count\":{recent_count},\"baseline_count\":{baseline_count},\"recent_window_secs\":{recent_window_secs},\"baseline_window_secs\":{baseline_window_secs},\"spike_ratio\":{ratio:.3}}}"
    )
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketActivity, MarketActivityKind, MarketSignalDirection, MarketSource};

    use super::{ActivitySpikeDetector, ActivitySpikeDetectorConfig};

    #[test]
    fn returns_no_signal_when_recent_activity_is_not_unusual() {
        let detector = detector();
        let history = vec![
            activity_at(0),
            activity_at(5),
            activity_at(10),
            activity_at(20),
            activity_at(28),
        ];

        let signal = detector.detect(&history).unwrap();
        assert!(signal.is_none());
    }

    #[test]
    fn emits_signal_for_recent_activity_burst() {
        let detector = detector();
        let history = vec![
            activity_at(24),
            activity_at(16),
            activity_at(8),
            activity_at(4),
            activity_at(3),
            activity_at(2),
            activity_at(1),
            activity_at(0),
        ];

        let signal = detector.detect(&history).unwrap().unwrap();
        assert_eq!(signal.direction, MarketSignalDirection::Yes);
        assert_eq!(signal.signal_name, "activity_spike");
        assert!(signal
            .rationale
            .as_deref()
            .unwrap()
            .contains("recent_count"));
    }

    #[test]
    fn returns_no_signal_for_insufficient_baseline_history() {
        let detector = detector();
        let history = vec![
            activity_at(24),
            activity_at(25),
            activity_at(26),
            activity_at(27),
        ];

        let signal = detector.detect(&history).unwrap();
        assert!(signal.is_none());
    }

    fn detector() -> ActivitySpikeDetector {
        ActivitySpikeDetector::new(ActivitySpikeDetectorConfig::new(300, 1800, 3, 2).unwrap())
            .unwrap()
    }

    fn activity_at(minutes_before_latest: i64) -> MarketActivity {
        let latest = Utc.with_ymd_and_hms(2026, 4, 12, 0, 30, 0).unwrap();
        MarketActivity {
            market_id: "market-1".into(),
            source: MarketSource::Polymarket,
            kind: MarketActivityKind::Trade,
            price: Some(0.51),
            quantity: Some(10.0),
            observed_at: latest - Duration::minutes(minutes_before_latest),
        }
    }
}
