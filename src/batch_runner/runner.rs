use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    handoff::{
        ingest_research_signals_file, ResearchSignalIngestOptions, ResearchSignalIngestReport,
    },
    materialization::{
        materialize_decisions, DecisionMaterializationOptions, DecisionMaterializationReport,
    },
    observability::{summary_from_store, ObservabilitySummary},
    queries::QueryService,
    store::JsonlEventStore,
};

use super::BatchRunnerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BatchRunOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchRunReport {
    pub batch_trace_id: String,
    pub store_path: PathBuf,
    pub research_signals_path: PathBuf,
    pub dry_run: bool,
    pub success: bool,
    pub ingest: ResearchSignalIngestReport,
    pub materialization: DecisionMaterializationReport,
    pub final_summary: ObservabilitySummary,
}

pub fn run_batch(
    store: &JsonlEventStore,
    research_signals_path: impl AsRef<Path>,
    options: BatchRunOptions,
) -> Result<BatchRunReport, BatchRunnerError> {
    let research_signals_path = research_signals_path.as_ref().to_path_buf();
    let batch_trace_id = build_batch_trace_id();

    let ingest = ingest_research_signals_file(
        store,
        &research_signals_path,
        ResearchSignalIngestOptions {
            dry_run: options.dry_run,
        },
    )?;

    let materialization = materialize_decisions(
        &QueryService::new(store),
        DecisionMaterializationOptions {
            dry_run: options.dry_run,
        },
    )?;

    let final_summary = summary_from_store(store)?;

    Ok(BatchRunReport {
        batch_trace_id,
        store_path: store.path().to_path_buf(),
        research_signals_path,
        dry_run: options.dry_run,
        success: true,
        ingest,
        materialization,
        final_summary,
    })
}

fn build_batch_trace_id() -> String {
    format!("runtime-batch-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"))
}
