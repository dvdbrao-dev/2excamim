mod error;
mod research_signals;

pub use error::HandoffError;
pub use research_signals::{
    ingest_research_signals_file, IngestedResearchSignal, ResearchSignalIngestRejection,
    ResearchSignalIngestReport, ResearchSignalInputRecord,
};
