use crate::events::{envelope::EventTyped, error::EventError, EventEnvelope};
use uuid::Uuid;

pub trait Validate {
    fn validate(&self) -> Result<(), EventError>;
}

pub(crate) fn validate_required_string(value: &str, field: &str) -> Result<(), EventError> {
    if value.trim().is_empty() {
        return Err(EventError::validation(format!("{field} cannot be empty")));
    }

    Ok(())
}

pub(crate) fn validate_optional_string(value: Option<&str>, field: &str) -> Result<(), EventError> {
    if let Some(value) = value {
        if value.trim().is_empty() {
            return Err(EventError::validation(format!(
                "{field} cannot be blank when provided"
            )));
        }
    }

    Ok(())
}

pub(crate) fn validate_idempotency_component(value: &str, field: &str) -> Result<(), EventError> {
    validate_required_string(value, field)?;

    if value.contains(':') {
        return Err(EventError::validation(format!(
            "{field} cannot contain ':'"
        )));
    }

    Ok(())
}

pub(crate) fn validate_probability(value: f64, field: &str) -> Result<(), EventError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(EventError::validation(format!("{field} must be in [0,1]")));
    }

    Ok(())
}

pub(crate) fn validate_positive_finite(value: f64, field: &str) -> Result<(), EventError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(EventError::validation(format!("{field} must be > 0")));
    }

    Ok(())
}

pub fn validate_envelope<T>(event: &EventEnvelope<T>) -> Result<(), EventError>
where
    T: Validate + EventTyped,
{
    validate_required_string(&event.event_id, "event_id")?;
    Uuid::parse_str(&event.event_id)
        .map_err(|_| EventError::invariant("event_id must be a valid UUID"))?;

    if event.schema_version != "v1" {
        return Err(EventError::validation("schema_version must be v1"));
    }
    validate_required_string(&event.produced_by, "produced_by")?;
    validate_required_string(&event.idempotency_key, "idempotency_key")?;
    validate_optional_string(event.aggregate_key.as_deref(), "aggregate_key")?;
    if event.event_type != T::event_type() {
        return Err(EventError::invariant(
            "event_type does not match payload type",
        ));
    }

    let expected_idempotency_key = event.payload.idempotency_key();
    if event.idempotency_key != expected_idempotency_key {
        return Err(EventError::invariant(
            "idempotency_key does not match payload-derived key",
        ));
    }

    event.linkage.validate()?;
    event.provenance.validate()?;
    event.payload.validate()
}
