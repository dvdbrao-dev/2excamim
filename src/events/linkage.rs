use serde::{Deserialize, Serialize};

use crate::events::{
    error::EventError,
    validation::{validate_optional_string, Validate},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linkage {
    pub hypothesis_id: Option<String>,
    pub signal_id: Option<String>,
    pub decision_id: Option<String>,
    pub order_id: Option<String>,
    pub position_id: Option<String>,
    pub parent_event_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl Validate for Linkage {
    fn validate(&self) -> Result<(), EventError> {
        validate_optional_string(self.hypothesis_id.as_deref(), "linkage.hypothesis_id")?;
        validate_optional_string(self.signal_id.as_deref(), "linkage.signal_id")?;
        validate_optional_string(self.decision_id.as_deref(), "linkage.decision_id")?;
        validate_optional_string(self.order_id.as_deref(), "linkage.order_id")?;
        validate_optional_string(self.position_id.as_deref(), "linkage.position_id")?;
        validate_optional_string(self.parent_event_id.as_deref(), "linkage.parent_event_id")?;
        validate_optional_string(self.correlation_id.as_deref(), "linkage.correlation_id")?;
        Ok(())
    }
}
