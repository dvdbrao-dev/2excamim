use crate::ValidationError;

/// Lightweight validation contract for domain values.
pub trait Validate {
    /// Validates the value.
    fn validate(&self) -> Result<(), ValidationError>;
}

pub(crate) fn required_string(value: &str, field: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new(format!("{field} cannot be empty")));
    }

    Ok(())
}

pub(crate) fn optional_string(value: Option<&str>, field: &str) -> Result<(), ValidationError> {
    if let Some(value) = value {
        required_string(value, field)?;
    }

    Ok(())
}

pub(crate) fn probability(value: f64, field: &str) -> Result<(), ValidationError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(ValidationError::new(format!("{field} must be in [0,1]")));
    }

    Ok(())
}

pub(crate) fn positive_finite(value: f64, field: &str) -> Result<(), ValidationError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ValidationError::new(format!("{field} must be > 0")));
    }

    Ok(())
}

pub(crate) fn optional_positive_finite(
    value: Option<f64>,
    field: &str,
) -> Result<(), ValidationError> {
    if let Some(value) = value {
        positive_finite(value, field)?;
    }

    Ok(())
}
