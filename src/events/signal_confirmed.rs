use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::Validate,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalConfirmed {
    pub signal_id: String,
    pub confirmed_by: String,
    pub confirmation_reason: Option<String>,
    pub confirmation_score: Option<f64>,
}

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
        if self.signal_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "signal_id cannot be empty".into(),
            ));
        }
        if self.confirmed_by.trim().is_empty() {
            return Err(EventError::ValidationError(
                "confirmed_by cannot be empty".into(),
            ));
        }
        if let Some(score) = self.confirmation_score {
            if !(0.0..=1.0).contains(&score) {
                return Err(EventError::ValidationError(
                    "confirmation_score must be in [0,1]".into(),
                ));
            }
        }

        Ok(())
    }
}
