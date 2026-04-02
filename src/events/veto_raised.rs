use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::Validate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VetoScope {
    Signal,
    Decision,
    Order,
    Global,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VetoRaised {
    pub veto_id: String,
    pub scope: VetoScope,
    pub target_id: String,
    pub reason_code: String,
    pub reason_text: Option<String>,
    pub raised_by: String,
}

impl EventTyped for VetoRaised {
    fn event_type() -> EventType {
        EventType::VetoRaised
    }

    fn idempotency_key(&self) -> String {
        format!("veto.raised:v1:{}", self.veto_id)
    }
}

impl Validate for VetoRaised {
    fn validate(&self) -> Result<(), EventError> {
        if self.veto_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "veto_id cannot be empty".into(),
            ));
        }
        if self.target_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "target_id cannot be empty".into(),
            ));
        }
        if self.reason_code.trim().is_empty() {
            return Err(EventError::ValidationError(
                "reason_code cannot be empty".into(),
            ));
        }
        if self.raised_by.trim().is_empty() {
            return Err(EventError::ValidationError(
                "raised_by cannot be empty".into(),
            ));
        }

        Ok(())
    }
}
