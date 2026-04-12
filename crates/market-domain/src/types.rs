use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    validation::{optional_positive_finite, optional_string, probability, required_string},
    Validate, ValidationError,
};

/// Origin of market data or signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketSource {
    Polymarket,
    Kalshi,
    Manual,
    Synthetic,
}

/// Canonical lifecycle state for a market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketStatus {
    Open,
    Closed,
    Resolved,
    Suspended,
    Cancelled,
}

/// Canonical activity classification for market updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketActivityKind {
    Trade,
    Volume,
    OpenInterest,
    Resolution,
}

/// Canonical direction for derived market signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketSignalDirection {
    Yes,
    No,
    Neutral,
}

/// Point-in-time market state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSnapshot {
    /// Stable market identifier within the source.
    pub market_id: String,
    /// Source that produced the snapshot.
    pub source: MarketSource,
    /// Human-readable market title.
    pub title: String,
    /// Canonical market lifecycle state.
    pub status: MarketStatus,
    /// Best bid price when available.
    pub best_bid: Option<f64>,
    /// Best ask price when available.
    pub best_ask: Option<f64>,
    /// Last traded price when available.
    pub last_price: Option<f64>,
    /// Aggregated volume when available.
    pub volume: Option<f64>,
    /// Observation timestamp in UTC.
    pub observed_at: DateTime<Utc>,
}

impl MarketSnapshot {
    /// Creates a validated market snapshot.
    pub fn new(
        market_id: impl Into<String>,
        source: MarketSource,
        title: impl Into<String>,
        status: MarketStatus,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let snapshot = Self {
            market_id: market_id.into(),
            source,
            title: title.into(),
            status,
            best_bid: None,
            best_ask: None,
            last_price: None,
            volume: None,
            observed_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl Validate for MarketSnapshot {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.market_id, "market_id")?;
        required_string(&self.title, "title")?;

        if let Some(best_bid) = self.best_bid {
            probability(best_bid, "best_bid")?;
        }
        if let Some(best_ask) = self.best_ask {
            probability(best_ask, "best_ask")?;
        }
        if let Some(last_price) = self.last_price {
            probability(last_price, "last_price")?;
        }
        optional_positive_finite(self.volume, "volume")?;

        if let (Some(best_bid), Some(best_ask)) = (self.best_bid, self.best_ask) {
            if best_bid > best_ask {
                return Err(ValidationError::new(
                    "best_bid cannot be greater than best_ask",
                ));
            }
        }

        Ok(())
    }
}

/// Point-in-time tradable quote for a market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketQuote {
    /// Stable market identifier within the source.
    pub market_id: String,
    /// Source that produced the quote.
    pub source: MarketSource,
    /// Best bid price.
    pub bid_price: f64,
    /// Best ask price.
    pub ask_price: f64,
    /// Last traded price when available.
    pub last_price: Option<f64>,
    /// Observation timestamp in UTC.
    pub observed_at: DateTime<Utc>,
}

impl MarketQuote {
    /// Creates a validated market quote.
    pub fn new(
        market_id: impl Into<String>,
        source: MarketSource,
        bid_price: f64,
        ask_price: f64,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let quote = Self {
            market_id: market_id.into(),
            source,
            bid_price,
            ask_price,
            last_price: None,
            observed_at,
        };
        quote.validate()?;
        Ok(quote)
    }
}

impl Validate for MarketQuote {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.market_id, "market_id")?;
        probability(self.bid_price, "bid_price")?;
        probability(self.ask_price, "ask_price")?;

        if let Some(last_price) = self.last_price {
            probability(last_price, "last_price")?;
        }
        if self.bid_price > self.ask_price {
            return Err(ValidationError::new(
                "bid_price cannot be greater than ask_price",
            ));
        }

        Ok(())
    }
}

/// Canonical market activity sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketActivity {
    /// Stable market identifier within the source.
    pub market_id: String,
    /// Source that produced the activity.
    pub source: MarketSource,
    /// Canonical activity kind.
    pub kind: MarketActivityKind,
    /// Activity price when relevant.
    pub price: Option<f64>,
    /// Activity quantity when relevant.
    pub quantity: Option<f64>,
    /// Observation timestamp in UTC.
    pub observed_at: DateTime<Utc>,
}

impl MarketActivity {
    /// Creates a validated market activity record.
    pub fn new(
        market_id: impl Into<String>,
        source: MarketSource,
        kind: MarketActivityKind,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let activity = Self {
            market_id: market_id.into(),
            source,
            kind,
            price: None,
            quantity: None,
            observed_at,
        };
        activity.validate()?;
        Ok(activity)
    }
}

impl Validate for MarketActivity {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.market_id, "market_id")?;

        if let Some(price) = self.price {
            probability(price, "price")?;
        }
        optional_positive_finite(self.quantity, "quantity")?;

        if matches!(self.kind, MarketActivityKind::Trade)
            && (self.price.is_none() || self.quantity.is_none())
        {
            return Err(ValidationError::new(
                "trade activity requires both price and quantity",
            ));
        }

        Ok(())
    }
}

/// Canonical derived signal for a market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSignal {
    /// Stable signal identifier.
    pub signal_id: String,
    /// Stable market identifier within the source.
    pub market_id: String,
    /// Source that produced the signal.
    pub source: MarketSource,
    /// Signal family or strategy label.
    pub signal_name: String,
    /// Canonical directional bias.
    pub direction: MarketSignalDirection,
    /// Confidence score in the inclusive range [0,1].
    pub confidence: f64,
    /// Optional human-readable rationale.
    pub rationale: Option<String>,
    /// Generation timestamp in UTC.
    pub generated_at: DateTime<Utc>,
}

impl MarketSignal {
    /// Creates a validated market signal.
    pub fn new(
        signal_id: impl Into<String>,
        market_id: impl Into<String>,
        source: MarketSource,
        signal_name: impl Into<String>,
        direction: MarketSignalDirection,
        confidence: f64,
        generated_at: DateTime<Utc>,
    ) -> Result<Self, ValidationError> {
        let signal = Self {
            signal_id: signal_id.into(),
            market_id: market_id.into(),
            source,
            signal_name: signal_name.into(),
            direction,
            confidence,
            rationale: None,
            generated_at,
        };
        signal.validate()?;
        Ok(signal)
    }
}

impl Validate for MarketSignal {
    fn validate(&self) -> Result<(), ValidationError> {
        required_string(&self.signal_id, "signal_id")?;
        required_string(&self.market_id, "market_id")?;
        required_string(&self.signal_name, "signal_name")?;
        probability(self.confidence, "confidence")?;
        optional_string(self.rationale.as_deref(), "rationale")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        MarketActivity, MarketActivityKind, MarketQuote, MarketSignal, MarketSignalDirection,
        MarketSnapshot, MarketSource, MarketStatus,
    };
    use crate::Validate;

    #[test]
    fn snapshot_rejects_crossed_book() {
        let mut snapshot = MarketSnapshot::new(
            "market-1",
            MarketSource::Polymarket,
            "Fed decision",
            MarketStatus::Open,
            Utc::now(),
        )
        .unwrap();
        snapshot.best_bid = Some(0.61);
        snapshot.best_ask = Some(0.60);

        let err = snapshot.validate().unwrap_err();
        assert!(err.message().contains("best_bid"));
    }

    #[test]
    fn quote_rejects_price_outside_probability_range() {
        let err = MarketQuote::new("market-1", MarketSource::Kalshi, -0.01, 0.55, Utc::now())
            .unwrap_err();

        assert!(err.message().contains("bid_price"));
    }

    #[test]
    fn trade_activity_requires_price_and_quantity() {
        let activity = MarketActivity {
            market_id: "market-1".into(),
            source: MarketSource::Polymarket,
            kind: MarketActivityKind::Trade,
            price: Some(0.53),
            quantity: None,
            observed_at: Utc::now(),
        };

        let err = activity.validate().unwrap_err();
        assert!(err.message().contains("trade activity"));
    }

    #[test]
    fn signal_rejects_blank_rationale_when_present() {
        let mut signal = MarketSignal::new(
            "signal-1",
            "market-1",
            MarketSource::Synthetic,
            "fair-value-gap",
            MarketSignalDirection::Yes,
            0.72,
            Utc::now(),
        )
        .unwrap();
        signal.rationale = Some("   ".into());

        let err = signal.validate().unwrap_err();
        assert!(err.message().contains("rationale"));
    }
}
