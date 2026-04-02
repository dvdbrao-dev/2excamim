use serde::{Deserialize, Serialize};

use crate::events::{
    error::EventError,
    validation::{validate_optional_string, Validate},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source_kind: SourceKind,
    pub source_ref: Option<String>,
    pub producer_run_id: Option<String>,
    pub actor: Option<String>,
    pub trace_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Research,
    Runtime,
    MarketData,
    ExecutionVenue,
    HumanOverride,
    Derived,
}

impl Validate for Provenance {
    fn validate(&self) -> Result<(), EventError> {
        validate_optional_string(self.source_ref.as_deref(), "provenance.source_ref")?;
        validate_optional_string(
            self.producer_run_id.as_deref(),
            "provenance.producer_run_id",
        )?;
        validate_optional_string(self.actor.as_deref(), "provenance.actor")?;
        validate_optional_string(self.trace_id.as_deref(), "provenance.trace_id")?;
        validate_optional_string(self.notes.as_deref(), "provenance.notes")?;
        Ok(())
    }
}
