use serde::{Deserialize, Serialize};

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
