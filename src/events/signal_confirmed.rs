use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_probability, Validate,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalConfirmed {
    pub signal_id: String,
    pub confirmed_by: String,
    pub confirmation_reason: Option<String>,
    pub confirmation_score: Option<f64>,
}

pub type SignalConfirmedPayload = SignalConfirmed;

impl EventTyped for SignalConfirmed {
    fn event_type() -> EventType {
        EventType::SignalConfirmed
    }

    fn idempotency_key(&self) -> String {
        format!(
            "signal.confirmed:v1:{}:{}",
            self.signal_id, self.confirmed_by
        )
    }
}

impl Validate for SignalConfirmed {
    fn validate(&self) -> Result<(), EventError> {
        validate_idempotency_component(&self.signal_id, "signal_id")?;
        validate_idempotency_component(&self.confirmed_by, "confirmed_by")?;
        validate_optional_string(self.confirmation_reason.as_deref(), "confirmation_reason")?;
        if let Some(score) = self.confirmation_score {
            validate_probability(score, "confirmation_score")?;
        }

        Ok(())
    }
}
