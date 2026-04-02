use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_required_string,
        Validate,
    },
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

pub type VetoRaisedPayload = VetoRaised;

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
        validate_idempotency_component(&self.veto_id, "veto_id")?;
        validate_required_string(&self.target_id, "target_id")?;
        validate_required_string(&self.reason_code, "reason_code")?;
        validate_optional_string(self.reason_text.as_deref(), "reason_text")?;
        validate_required_string(&self.raised_by, "raised_by")?;

        Ok(())
    }
}
