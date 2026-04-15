use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_positive_finite, validate_probability, Validate,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketScoredParameters {
    pub price_gap_limit: f64,
    pub min_volume_usdc: f64,
    pub min_resolution_hours: f64,
    pub max_resolution_hours: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketScored {
    pub market_id: String,
    pub scored_on: String,
    pub score: f64,
    pub market_price: f64,
    pub price_gap_to_half: f64,
    pub volume_usdc: f64,
    pub hours_to_resolution: f64,
    pub parameters: MarketScoredParameters,
}

pub type MarketScoredPayload = MarketScored;
pub type MarketScoredParametersPayload = MarketScoredParameters;

impl EventTyped for MarketScored {
    fn event_type() -> EventType {
        EventType::MarketScored
    }

    fn idempotency_key(&self) -> String {
        format!("market.scored:v1:{}:{}", self.market_id, self.scored_on)
    }
}

impl Validate for MarketScoredParameters {
    fn validate(&self) -> Result<(), EventError> {
        validate_probability(self.price_gap_limit, "parameters.price_gap_limit")?;
        validate_positive_finite(self.min_volume_usdc, "parameters.min_volume_usdc")?;
        validate_positive_finite(self.min_resolution_hours, "parameters.min_resolution_hours")?;
        validate_positive_finite(self.max_resolution_hours, "parameters.max_resolution_hours")?;

        if self.min_resolution_hours > self.max_resolution_hours {
            return Err(EventError::validation(
                "parameters.min_resolution_hours cannot exceed parameters.max_resolution_hours",
            ));
        }

        Ok(())
    }
}

impl Validate for MarketScored {
    fn validate(&self) -> Result<(), EventError> {
        validate_idempotency_component(&self.market_id, "market_id")?;
        validate_idempotency_component(&self.scored_on, "scored_on")?;
        NaiveDate::parse_from_str(&self.scored_on, "%Y-%m-%d").map_err(|_| {
            EventError::validation("scored_on must be an ISO 8601 date in YYYY-MM-DD format")
        })?;
        validate_probability(self.score, "score")?;
        validate_probability(self.market_price, "market_price")?;
        validate_probability(self.price_gap_to_half, "price_gap_to_half")?;
        validate_positive_finite(self.volume_usdc, "volume_usdc")?;
        validate_positive_finite(self.hours_to_resolution, "hours_to_resolution")?;
        self.parameters.validate()?;

        Ok(())
    }
}
