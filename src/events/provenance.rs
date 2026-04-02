use serde::{Deserialize, Serialize};

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
