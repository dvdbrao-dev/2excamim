use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_positive_finite,
        validate_required_string, Validate,
    },
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

pub type FillReceivedPayload = FillReceived;

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
        validate_idempotency_component(&self.fill_id, "fill_id")?;
        validate_optional_string(self.decision_id.as_deref(), "decision_id")?;
        validate_idempotency_component(&self.order_id, "order_id")?;
        validate_required_string(&self.instrument, "instrument")?;
        validate_idempotency_component(&self.venue, "venue")?;
        validate_positive_finite(self.quantity, "quantity")?;
        validate_positive_finite(self.price, "price")?;

        Ok(())
    }
}
