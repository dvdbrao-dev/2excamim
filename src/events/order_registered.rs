use serde::{Deserialize, Serialize};

use crate::events::{
    envelope::{EventType, EventTyped},
    error::EventError,
    validation::{
        validate_idempotency_component, validate_optional_string, validate_required_string,
        Validate,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRegistered {
    pub order_id: String,
    pub decision_id: Option<String>,
    pub instrument: String,
    pub venue: String,
}

pub type OrderRegisteredPayload = OrderRegistered;

impl EventTyped for OrderRegistered {
    fn event_type() -> EventType {
        EventType::OrderRegistered
    }

    fn idempotency_key(&self) -> String {
        format!("order.registered:v1:{}:{}", self.venue, self.order_id)
    }
}

impl Validate for OrderRegistered {
    fn validate(&self) -> Result<(), EventError> {
        validate_idempotency_component(&self.order_id, "order_id")?;
        validate_optional_string(self.decision_id.as_deref(), "decision_id")?;
        if let Some(decision_id) = self.decision_id.as_deref() {
            validate_idempotency_component(decision_id, "decision_id")?;
        }
        validate_required_string(&self.instrument, "instrument")?;
        validate_idempotency_component(&self.venue, "venue")?;
        Ok(())
    }
}
