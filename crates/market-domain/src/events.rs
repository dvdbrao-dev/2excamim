use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    validation::required_string, MarketActivity, MarketQuote, MarketSignal, MarketSnapshot,
    Validate, ValidationError,
};

/// Payload for a received market snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSnapshotReceived {
    /// Upstream event identifier from the source system.
    pub source_event_id: String,
    /// Validated snapshot payload.
    pub snapshot: MarketSnapshot,
    /// Receipt timestamp in UTC.
    pub received_at: DateTime<Utc>,
}

impl MarketSnapshotReceived {
    /// Creates a validated snapshot payload.
    pub fn new(
        source_event_id: impl Into<String>,
        snapshot: MarketSnapshot,
        received_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let payload = Self {
            source_event_id: source_event_id.into(),
            snapshot,
            received_at,
        };
        payload.validate()?;
        Ok(payload)
    }
}

impl Validate for MarketSnapshotReceived {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.source_event_id, "source_event_id")?;
        self.snapshot.validate()
    }
}

/// Payload for a quote update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketQuoteUpdated {
    /// Upstream event identifier from the source system.
    pub source_event_id: String,
    /// Validated quote payload.
    pub quote: MarketQuote,
    /// Receipt timestamp in UTC.
    pub received_at: DateTime<Utc>,
}

impl MarketQuoteUpdated {
    /// Creates a validated quote payload.
    pub fn new(
        source_event_id: impl Into<String>,
        quote: MarketQuote,
        received_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let payload = Self {
            source_event_id: source_event_id.into(),
            quote,
            received_at,
        };
        payload.validate()?;
        Ok(payload)
    }
}

impl Validate for MarketQuoteUpdated {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.source_event_id, "source_event_id")?;
        self.quote.validate()
    }
}

/// Payload for market activity ingestion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketActivityReceived {
    /// Upstream event identifier from the source system.
    pub source_event_id: String,
    /// Validated activity payload.
    pub activity: MarketActivity,
    /// Receipt timestamp in UTC.
    pub received_at: DateTime<Utc>,
}

impl MarketActivityReceived {
    /// Creates a validated activity payload.
    pub fn new(
        source_event_id: impl Into<String>,
        activity: MarketActivity,
        received_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let payload = Self {
            source_event_id: source_event_id.into(),
            activity,
            received_at,
        };
        payload.validate()?;
        Ok(payload)
    }
}

impl Validate for MarketActivityReceived {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.source_event_id, "source_event_id")?;
        self.activity.validate()
    }
}

/// Payload for a generated market signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSignalGenerated {
    /// Upstream event identifier from the source system.
    pub source_event_id: String,
    /// Validated signal payload.
    pub signal: MarketSignal,
    /// Emission timestamp in UTC.
    pub emitted_at: DateTime<Utc>,
}

impl MarketSignalGenerated {
    /// Creates a validated signal payload.
    pub fn new(
        source_event_id: impl Into<String>,
        signal: MarketSignal,
        emitted_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let payload = Self {
            source_event_id: source_event_id.into(),
            signal,
            emitted_at,
        };
        payload.validate()?;
        Ok(payload)
    }
}

impl Validate for MarketSignalGenerated {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.source_event_id, "source_event_id")?;
        self.signal.validate()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{MarketActivityReceived, MarketQuoteUpdated, MarketSignalGenerated};
    use crate::{
        MarketActivity, MarketActivityKind, MarketQuote, MarketSignal, MarketSignalDirection,
        MarketSource, Validate,
    };

    #[test]
    fn quote_payload_propagates_quote_validation() {
        let mut quote =
            MarketQuote::new("market-1", MarketSource::Kalshi, 0.30, 0.40, Utc::now()).unwrap();
        quote.last_price = Some(1.2);

        let err = MarketQuoteUpdated::new("evt-1", quote, Utc::now()).unwrap_err();
        assert!(err.message().contains("last_price"));
    }

    #[test]
    fn activity_payload_rejects_blank_source_event_id() {
        let activity = MarketActivity {
            market_id: "market-1".into(),
            source: MarketSource::Polymarket,
            kind: MarketActivityKind::Volume,
            price: None,
            quantity: Some(100.0),
            observed_at: Utc::now(),
        };

        let err = MarketActivityReceived::new("   ", activity, Utc::now()).unwrap_err();
        assert!(err.message().contains("source_event_id"));
    }

    #[test]
    fn signal_payload_accepts_valid_signal() {
        let signal = MarketSignal::new(
            "signal-1",
            "market-1",
            MarketSource::Synthetic,
            "momentum",
            MarketSignalDirection::No,
            0.64,
            Utc::now(),
        )
        .unwrap();

        let payload = MarketSignalGenerated::new("evt-1", signal, Utc::now()).unwrap();
        assert!(payload.validate().is_ok());
    }
}
