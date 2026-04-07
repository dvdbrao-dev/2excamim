mod error;
mod research_signals;

pub use error::HandoffError;
pub use research_signals::{
    ingest_research_signals_file, IngestedResearchSignal, ResearchSignalIngestOptions,
    ResearchSignalIngestRejection, ResearchSignalIngestReport, ResearchSignalInputRecord,
    ResearchSignalRejectedReason, RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION,
};
