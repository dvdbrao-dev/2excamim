use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_probability,
        validate_required_string, Validate,
    },
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

pub type SignalGeneratedPayload = SignalGenerated;

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
        validate_idempotency_component(&self.signal_id, "signal_id")?;
        validate_optional_string(self.hypothesis_id.as_deref(), "hypothesis_id")?;
        validate_required_string(&self.instrument, "instrument")?;
        validate_required_string(&self.timeframe, "timeframe")?;
        validate_probability(self.strength, "strength")?;
        validate_optional_string(self.rationale.as_deref(), "rationale")?;

        Ok(())
    }
}
