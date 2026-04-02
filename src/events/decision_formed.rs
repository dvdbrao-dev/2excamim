use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    signal_generated::SignalSide,
    validation::Validate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionAction {
    Enter,
    Exit,
    Reduce,
    Hold,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionFormed {
    pub decision_id: String,
    pub instrument: String,
    pub action: DecisionAction,
    pub side: Option<SignalSide>,
    pub size_hint: Option<f64>,
    pub rationale: Option<String>,
}

impl EventTyped for DecisionFormed {
    fn event_type() -> EventType {
        EventType::DecisionFormed
    }

    fn idempotency_key(&self) -> String {
        format!("decision.formed:v1:{}", self.decision_id)
    }
}

impl Validate for DecisionFormed {
    fn validate(&self) -> Result<(), EventError> {
        if self.decision_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "decision_id cannot be empty".into(),
            ));
        }
        if self.instrument.trim().is_empty() {
            return Err(EventError::ValidationError(
                "instrument cannot be empty".into(),
            ));
        }
        if let Some(size_hint) = self.size_hint {
            if size_hint <= 0.0 {
                return Err(EventError::ValidationError("size_hint must be > 0".into()));
            }
        }

        Ok(())
    }
}
