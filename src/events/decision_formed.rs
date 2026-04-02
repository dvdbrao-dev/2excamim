use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    signal_generated::SignalSide,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_positive_finite,
        validate_required_string, Validate,
    },
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

pub type DecisionFormedPayload = DecisionFormed;

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
        validate_idempotency_component(&self.decision_id, "decision_id")?;
        validate_required_string(&self.instrument, "instrument")?;
        validate_optional_string(self.rationale.as_deref(), "rationale")?;
        if let Some(size_hint) = self.size_hint {
            validate_positive_finite(size_hint, "size_hint")?;
        }

        Ok(())
    }
}
