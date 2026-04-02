use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::Validate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillReceived {
    pub fill_id: String,
    pub decision_id: Option<String>,
    pub order_id: String,
    pub instrument: String,
    pub side: FillSide,
    pub quantity: f64,
    pub price: f64,
    pub venue: String,
    pub executed_at: DateTime<Utc>,
}

impl EventTyped for FillReceived {
    fn event_type() -> EventType {
        EventType::FillReceived
    }

    fn idempotency_key(&self) -> String {
        format!(
            "fill.received:v1:{}:{}:{}",
            self.venue, self.order_id, self.fill_id
        )
    }
}

impl Validate for FillReceived {
    fn validate(&self) -> Result<(), EventError> {
        if self.fill_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "fill_id cannot be empty".into(),
            ));
        }
        if self.order_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "order_id cannot be empty".into(),
            ));
        }
        if self.instrument.trim().is_empty() {
            return Err(EventError::ValidationError(
                "instrument cannot be empty".into(),
            ));
        }
        if self.venue.trim().is_empty() {
            return Err(EventError::ValidationError("venue cannot be empty".into()));
        }
        if self.quantity <= 0.0 {
            return Err(EventError::ValidationError("quantity must be > 0".into()));
        }
        if self.price <= 0.0 {
            return Err(EventError::ValidationError("price must be > 0".into()));
        }

        Ok(())
    }
}
