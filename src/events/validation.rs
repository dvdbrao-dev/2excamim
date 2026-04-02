use crate::events::{envelope::EventTyped, error::EventError, EventEnvelope};

pub trait Validate {
    fn validate(&self) -> Result<(), EventError>;
}

pub fn validate_envelope<T>(event: &EventEnvelope<T>) -> Result<(), EventError>
where
    T: Validate + EventTyped,
{
    if event.event_id.trim().is_empty() {
        return Err(EventError::ValidationError(
            "event_id cannot be empty".into(),
        ));
    }
    if event.schema_version != "v1" {
        return Err(EventError::ValidationError(
            "schema_version must be v1".into(),
        ));
    }
    if event.produced_by.trim().is_empty() {
        return Err(EventError::ValidationError(
            "produced_by cannot be empty".into(),
        ));
    }
    if event.idempotency_key.trim().is_empty() {
        return Err(EventError::ValidationError(
            "idempotency_key cannot be empty".into(),
        ));
    }
    if event.event_type != T::event_type() {
        return Err(EventError::InvariantError(
            "event_type does not match payload type".into(),
        ));
    }

    event.payload.validate()
}
