use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::Validate,
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
        if self.hypothesis_id.trim().is_empty() {
            return Err(EventError::ValidationError(
                "hypothesis_id cannot be empty".into(),
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
        if self.thesis.trim().is_empty() {
            return Err(EventError::ValidationError("thesis cannot be empty".into()));
        }
        if let Some(confidence) = self.confidence {
            if !(0.0..=1.0).contains(&confidence) {
                return Err(EventError::ValidationError(
                    "confidence must be in [0,1]".into(),
                ));
            }
        }

        Ok(())
    }
}
