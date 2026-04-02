use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_probability,
        validate_required_string, Validate,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HypothesisGenerated {
    pub hypothesis_id: String,
    pub instrument: String,
    pub timeframe: String,
    pub thesis: String,
    pub direction_hint: Option<String>,
    pub confidence: Option<f64>,
}

pub type HypothesisGeneratedPayload = HypothesisGenerated;

impl EventTyped for HypothesisGenerated {
    fn event_type() -> EventType {
        EventType::HypothesisGenerated
    }

    fn idempotency_key(&self) -> String {
        format!("hypothesis.generated:v1:{}", self.hypothesis_id)
    }
}

impl Validate for HypothesisGenerated {
    fn validate(&self) -> Result<(), EventError> {
        validate_idempotency_component(&self.hypothesis_id, "hypothesis_id")?;
        validate_required_string(&self.instrument, "instrument")?;
        validate_required_string(&self.timeframe, "timeframe")?;
        validate_required_string(&self.thesis, "thesis")?;
        validate_optional_string(self.direction_hint.as_deref(), "direction_hint")?;
        if let Some(confidence) = self.confidence {
            validate_probability(confidence, "confidence")?;
        }

        Ok(())
    }
}
