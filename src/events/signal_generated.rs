use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::Validate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalSide {
    Long,
    Short,
    Flat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalGenerated {
    pub signal_id: String,
    pub hypothesis_id: Option<String>,
    pub instrument: String,
    pub timeframe: String,
    pub side: SignalSide,
    pub strength: f64,
    pub rationale: Option<String>,
}

impl EventTyped for SignalGenerated {
    fn event_type() -> EventType {
        EventType::SignalGenerated
    }

    fn idempotency_key(&self) -> String {
        format!("signal.generated:v1:{}", self.signal_id)
    }
}

impl Validate for SignalGenerated {
    fn validate(&self) -> Result<(), EventError> {
        if self.signal_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "signal_id cannot be empty".into(),
            ));
        }
        if self.instrument.trim().is_empty() {
            return Err(EventError::ValidationError(
                "instrument cannot be empty".into(),
            ));
        }
        if self.timeframe.trim().is_empty() {
            return Err(EventError::ValidationError(
                "timeframe cannot be empty".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.strength) {
            return Err(EventError::ValidationError(
                "strength must be in [0,1]".into(),
            ));
        }

        Ok(())
    }
}
