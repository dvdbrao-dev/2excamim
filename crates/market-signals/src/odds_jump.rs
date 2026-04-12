use chrono::{DateTime, Utc};
use market_domain::{MarketQuote, MarketSignal, MarketSignalDirection, Validate};

use crate::DetectorError;

/// Configuration for the deterministic odds-jump detector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OddsJumpDetectorConfig {
    /// Minimum number of quotes required before detection can run.
    pub min_history: usize,
    /// Absolute probability jump required to emit an upward signal.
    pub upward_threshold: f64,
    /// Absolute probability jump required to emit a downward signal.
    pub downward_threshold: f64,
}

impl OddsJumpDetectorConfig {
    /// Creates a new validated detector configuration.
    pub fn new(
        min_history: usize,
        upward_threshold: f64,
        downward_threshold: f64,
    ) -> Result<Self, DetectorError> {
        let config = Self {
            min_history,
            upward_threshold,
            downward_threshold,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), DetectorError> {
        if self.min_history < 2 {
            return Err(DetectorError::invalid_config(
                "min_history must be at least 2",
            ));
        }
        if !self.upward_threshold.is_finite() || self.upward_threshold <= 0.0 {
            return Err(DetectorError::invalid_config(
                "upward_threshold must be > 0",
            ));
        }
        if !self.downward_threshold.is_finite() || self.downward_threshold <= 0.0 {
            return Err(DetectorError::invalid_config(
                "downward_threshold must be > 0",
            ));
        }

        Ok(())
    }
}

impl Default for OddsJumpDetectorConfig {
    fn default() -> Self {
        Self {
            min_history: 2,
            upward_threshold: 0.10,
            downward_threshold: 0.10,
        }
    }
}

/// Deterministic detector for large quote-to-quote odds jumps.
pub struct OddsJumpDetector {
    config: OddsJumpDetectorConfig,
}

impl OddsJumpDetector {
    /// Creates a detector from validated configuration.
    pub fn new(config: OddsJumpDetectorConfig) -> Result<Self, DetectorError> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Evaluates quote history and emits one canonical signal when a jump is detected.
    pub fn detect(&self, history: &[MarketQuote]) -> Result<Option<MarketSignal>, DetectorError> {
        if history.len() < self.config.min_history {
            return Ok(None);
        }

        let mut quotes = history.to_vec();
        quotes.sort_by_key(|quote| quote.observed_at);

        let latest = quotes.last().expect("history length checked");
        let previous = &quotes[quotes.len() - 2];

        if latest.market_id != previous.market_id {
            return Err(DetectorError::invalid_input(
                "quote history must belong to one market_id",
            ));
        }
        if latest.source != previous.source {
            return Err(DetectorError::invalid_input(
                "quote history must belong to one source",
            ));
        }
        latest
            .validate()
            .map_err(|error| DetectorError::invalid_input(error.to_string()))?;
        previous
            .validate()
            .map_err(|error| DetectorError::invalid_input(error.to_string()))?;

        let latest_price = signal_price(latest);
        let previous_price = signal_price(previous);
        let delta = latest_price - previous_price;

        let (direction, threshold) = if delta >= self.config.upward_threshold {
            (MarketSignalDirection::Yes, self.config.upward_threshold)
        } else if -delta >= self.config.downward_threshold {
            (MarketSignalDirection::No, self.config.downward_threshold)
        } else {
            return Ok(None);
        };

        let confidence = (delta.abs() / threshold).min(1.0);
        let signal = MarketSignal::new(
            signal_id(
                "odds-jump",
                &latest.market_id,
                latest.observed_at,
                direction,
            ),
            latest.market_id.clone(),
            latest.source,
            "odds_jump",
            direction,
            confidence,
            latest.observed_at,
        )
        .map_err(|error| DetectorError::invalid_input(error.to_string()))?;

        Ok(Some(signal))
    }
}

fn signal_price(quote: &MarketQuote) -> f64 {
    quote
        .last_price
        .unwrap_or_else(|| (quote.bid_price + quote.ask_price) / 2.0)
}

pub(crate) fn signal_id(
    detector_name: &str,
    market_id: &str,
    observed_at: DateTime<Utc>,
    direction: MarketSignalDirection,
) -> String {
    let direction = match direction {
        MarketSignalDirection::Yes => "yes",
        MarketSignalDirection::No => "no",
        MarketSignalDirection::Neutral => "neutral",
    };

    format!(
        "{detector_name}:{market_id}:{direction}:{}",
        observed_at.to_rfc3339()
    )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use market_domain::{MarketQuote, MarketSignalDirection, MarketSource};

    use super::{OddsJumpDetector, OddsJumpDetectorConfig};

    #[test]
    fn returns_no_signal_when_jump_is_below_threshold() {
        let detector = detector();
        let quotes = vec![
            quote("market-1", 0.40, 0.42, Some(0.41), 0),
            quote("market-1", 0.43, 0.45, Some(0.44), 1),
        ];

        let signal = detector.detect(&quotes).unwrap();
        assert!(signal.is_none());
    }

    #[test]
    fn emits_upward_jump_signal() {
        let detector = detector();
        let quotes = vec![
            quote("market-1", 0.40, 0.42, Some(0.41), 0),
            quote("market-1", 0.56, 0.58, Some(0.57), 1),
        ];

        let signal = detector.detect(&quotes).unwrap().unwrap();
        assert_eq!(signal.direction, MarketSignalDirection::Yes);
        assert_eq!(signal.signal_name, "odds_jump");
    }

    #[test]
    fn emits_downward_jump_signal() {
        let detector = detector();
        let quotes = vec![
            quote("market-1", 0.60, 0.62, Some(0.61), 0),
            quote("market-1", 0.42, 0.44, Some(0.43), 1),
        ];

        let signal = detector.detect(&quotes).unwrap().unwrap();
        assert_eq!(signal.direction, MarketSignalDirection::No);
    }

    #[test]
    fn returns_no_signal_for_insufficient_history() {
        let detector = detector();
        let quotes = vec![quote("market-1", 0.40, 0.42, Some(0.41), 0)];

        let signal = detector.detect(&quotes).unwrap();
        assert!(signal.is_none());
    }

    fn detector() -> OddsJumpDetector {
        OddsJumpDetector::new(OddsJumpDetectorConfig::new(2, 0.10, 0.10).unwrap()).unwrap()
    }

    fn quote(
        market_id: &str,
        bid_price: f64,
        ask_price: f64,
        last_price: Option<f64>,
        minute: u32,
    ) -> MarketQuote {
        let mut quote = MarketQuote::new(
            market_id,
            MarketSource::Polymarket,
            bid_price,
            ask_price,
            Utc.with_ymd_and_hms(2026, 4, 12, 0, minute, 0).unwrap(),
        )
        .unwrap();
        quote.last_price = last_price;
        quote
    }
}
