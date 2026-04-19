use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    agents::ConfirmationPolicyLoadError,
    batch_runner::BatchRunnerError,
    codecs::CodecError,
    dashboard::DashboardError,
    execution::{
        PaperDecisionRunnerError, PaperExecutionRequest, PaperRiskGuardConfig,
        PolymarketPaperAdapterError,
    },
    handoff::HandoffError,
    materialization::{FillObservationRequest, MaterializationError},
    observability::ObservabilityError,
    operations::{OperationalSummaryError, OperationalSummaryFormat},
    queries::QueryError,
    store::StoreError,
};

mod cli_parser;
mod command_dispatch;
mod json_renderer;
mod text_renderer;

pub(crate) const DEFAULT_STORE_PATH: &str = "./var/events.jsonl";
pub(crate) const DEFAULT_SNAPSHOTS_PATH: &str = "./var/market_snapshots.jsonl";
pub(crate) const DEFAULT_READINESS_PATH: &str = "./var/readiness/confirmation_readiness.json";
pub(crate) const DEFAULT_DASHBOARD_PATH: &str = "./var/dashboard/control_room.html";
pub(crate) const DEFAULT_DASHBOARD_HOST: &str = "127.0.0.1";
pub(crate) const DEFAULT_DASHBOARD_PORT: u16 = 8000;
pub(crate) const DEFAULT_OPERATIONAL_SUMMARY_JSON_PATH: &str =
    "./var/operations/latest_summary.json";
pub(crate) const DEFAULT_OPERATIONAL_SUMMARY_MARKDOWN_PATH: &str =
    "./var/operations/latest_summary.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Command {
    Summary,
    Signal {
        signal_id: String,
    },
    Decision {
        decision_id: String,
    },
    Order {
        order_id: String,
    },
    Fill {
        fill_id: String,
    },
    ObserveFill {
        request: FillObservationRequest,
    },
    PolicySignal {
        signal_id: String,
    },
    PolicyDecision {
        decision_id: String,
    },
    PolicyOrder {
        order_id: String,
    },
    IngestResearchSignals {
        input_path: PathBuf,
    },
    ConfirmSignal {
        signal_id: String,
        confirmed_by: String,
        confirmation_reasons: Option<Vec<String>>,
        rejection_reasons: Option<Vec<String>>,
    },
    ConfirmSignals,
    MeasureConfirmationOutcomes,
    EvaluateConfirmationPolicy,
    WalkForwardConfirmationPolicy,
    ProposeConfirmationPolicy,
    MaterializeConfirmationReadiness,
    SimulatePaperFill {
        request: PaperExecutionRequest,
    },
    RunPaperDecisions,
    RunPaperPipeline {
        research_signals_path: Option<PathBuf>,
    },
    ServeDashboard,
    GenerateOperationalSummary,
    ShowPaperLedger,
    MaterializeDecisions,
    MaterializeOrders,
    SubmitOrders,
    RunBatch {
        research_signals_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Config {
    pub(crate) command: Command,
    pub(crate) store_path: PathBuf,
    pub(crate) snapshots_path: PathBuf,
    pub(crate) format: OutputFormat,
    pub(crate) dry_run: bool,
    pub(crate) horizon_seconds: i64,
    pub(crate) delta_threshold: f64,
    pub(crate) horizons: Vec<i64>,
    pub(crate) confidence_thresholds: Vec<f64>,
    pub(crate) eras: usize,
    pub(crate) window_size_seconds: Option<i64>,
    pub(crate) policy_file: Option<PathBuf>,
    pub(crate) output_path: Option<PathBuf>,
    pub(crate) dashboard_host: Option<String>,
    pub(crate) dashboard_port: Option<u16>,
    pub(crate) operational_summary_format: OperationalSummaryFormat,
    pub(crate) backend_trades_json: Option<PathBuf>,
    pub(crate) materialize_readiness: bool,
    pub(crate) usd_size: f64,
    pub(crate) backend_account: String,
    pub(crate) backend_data_dir: PathBuf,
    pub(crate) paper_risk: PaperRiskGuardConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperPipelineReport {
    pub dry_run: bool,
    pub stages: Vec<String>,
    pub signals_seen: usize,
    pub signals_generated: usize,
    pub signals_confirmed: usize,
    pub execution_requests_sent: usize,
    pub fills_persisted: usize,
    pub blocked_by_risk: usize,
    pub open_positions: usize,
    pub total_notional_spent: f64,
    pub total_notional_received: f64,
    pub readiness_states_materialized: Option<usize>,
}

#[derive(Debug)]
pub(crate) enum ParseOutcome {
    Config(Config),
    Help,
}

#[derive(Debug)]
pub(crate) enum RuntimeError {
    Usage(String),
    NotFound { entity: &'static str, id: String },
    Store(StoreError),
    Query(QueryError),
    Codec(CodecError),
    Handoff(HandoffError),
    Materialization(MaterializationError),
    Batch(BatchRunnerError),
    Observability(ObservabilityError),
    Execution(PolymarketPaperAdapterError),
    PaperDecisionRunner(PaperDecisionRunnerError),
    Dashboard(DashboardError),
    OperationalSummary(OperationalSummaryError),
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::NotFound { entity, id } => write!(f, "{entity} {id} not found in store"),
            Self::Store(error) => write!(f, "{error}"),
            Self::Query(error) => write!(f, "{error}"),
            Self::Codec(error) => write!(f, "{error}"),
            Self::Handoff(error) => write!(f, "{error}"),
            Self::Materialization(error) => write!(f, "{error}"),
            Self::Batch(error) => write!(f, "{error}"),
            Self::Observability(error) => write!(f, "{error}"),
            Self::Execution(error) => write!(f, "{error}"),
            Self::PaperDecisionRunner(error) => write!(f, "{error}"),
            Self::Dashboard(error) => write!(f, "{error}"),
            Self::OperationalSummary(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
        }
    }
}

impl From<StoreError> for RuntimeError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<QueryError> for RuntimeError {
    fn from(value: QueryError) -> Self {
        Self::Query(value)
    }
}

impl From<CodecError> for RuntimeError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<ObservabilityError> for RuntimeError {
    fn from(value: ObservabilityError) -> Self {
        Self::Observability(value)
    }
}

impl From<HandoffError> for RuntimeError {
    fn from(value: HandoffError) -> Self {
        Self::Handoff(value)
    }
}

impl From<MaterializationError> for RuntimeError {
    fn from(value: MaterializationError) -> Self {
        Self::Materialization(value)
    }
}

impl From<BatchRunnerError> for RuntimeError {
    fn from(value: BatchRunnerError) -> Self {
        Self::Batch(value)
    }
}

impl From<PolymarketPaperAdapterError> for RuntimeError {
    fn from(value: PolymarketPaperAdapterError) -> Self {
        Self::Execution(value)
    }
}

impl From<PaperDecisionRunnerError> for RuntimeError {
    fn from(value: PaperDecisionRunnerError) -> Self {
        Self::PaperDecisionRunner(value)
    }
}

impl From<DashboardError> for RuntimeError {
    fn from(value: DashboardError) -> Self {
        Self::Dashboard(value)
    }
}

impl From<OperationalSummaryError> for RuntimeError {
    fn from(value: OperationalSummaryError) -> Self {
        Self::OperationalSummary(value)
    }
}

impl From<io::Error> for RuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for RuntimeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<ConfirmationPolicyLoadError> for RuntimeError {
    fn from(value: ConfirmationPolicyLoadError) -> Self {
        match value {
            ConfirmationPolicyLoadError::Io(error) => Self::Io(error),
            ConfirmationPolicyLoadError::Json(error) => Self::Json(error),
            ConfirmationPolicyLoadError::InvalidData(message) => Self::Usage(message),
        }
    }
}

pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<String>>();
    match cli_parser::parse_args(args.clone()) {
        Ok(ParseOutcome::Help) => {
            if writeln!(stdout, "{}", cli_parser::usage()).is_err() {
                let _ = writeln!(stderr, "failed to write runtime help");
                return 1;
            }
            0
        }
        Ok(ParseOutcome::Config(config)) => match command_dispatch::execute(config.clone()) {
            Ok(output) => {
                if writeln!(stdout, "{output}").is_err() {
                    let _ = writeln!(stderr, "failed to write runtime output");
                    return 1;
                }
                0
            }
            Err(error) => {
                write_runtime_error(stderr, Some(&config), &error);
                exit_code_for_error(&error)
            }
        },
        Err(error) => {
            write_runtime_error_for_args(stderr, &args, &error);
            exit_code_for_error(&error)
        }
    }
}

pub(crate) fn exit_code_for_error(error: &RuntimeError) -> i32 {
    match error {
        RuntimeError::Usage(_) => 2,
        RuntimeError::NotFound { .. } => 3,
        RuntimeError::Store(_)
        | RuntimeError::Query(_)
        | RuntimeError::Codec(_)
        | RuntimeError::Handoff(_)
        | RuntimeError::Materialization(_)
        | RuntimeError::Batch(_)
        | RuntimeError::Observability(_)
        | RuntimeError::Execution(_)
        | RuntimeError::PaperDecisionRunner(_)
        | RuntimeError::Dashboard(_)
        | RuntimeError::OperationalSummary(_)
        | RuntimeError::Io(_)
        | RuntimeError::Json(_) => 1,
    }
}

fn write_runtime_error(stderr: &mut dyn Write, config: Option<&Config>, error: &RuntimeError) {
    let use_json = config.is_some_and(|config| config.format == OutputFormat::Json);
    if use_json {
        let payload = serde_json::json!({
            "kind": "error",
            "message": error.to_string(),
            "exit_code": exit_code_for_error(error),
        });
        let _ = writeln!(
            stderr,
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
                "{\"kind\":\"error\",\"message\":\"failed to encode runtime error\"}".to_string()
            })
        );
    } else {
        let _ = writeln!(stderr, "{error}");
    }
}

fn write_runtime_error_for_args(stderr: &mut dyn Write, args: &[String], error: &RuntimeError) {
    let use_json = args.iter().any(|arg| arg == "--json");
    if use_json {
        let payload = serde_json::json!({
            "kind": "error",
            "message": error.to_string(),
            "exit_code": exit_code_for_error(error),
        });
        let _ = writeln!(
            stderr,
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
                "{\"kind\":\"error\",\"message\":\"failed to encode runtime error\"}".to_string()
            })
        );
    } else {
        let _ = writeln!(stderr, "{error}");
    }
}

#[cfg(test)]
mod tests {
    use super::{exit_code_for_error, RuntimeError};

    #[test]
    fn exit_codes_remain_stable() {
        assert_eq!(exit_code_for_error(&RuntimeError::Usage("x".into())), 2);
        assert_eq!(
            exit_code_for_error(&RuntimeError::NotFound {
                entity: "signal",
                id: "missing".into(),
            }),
            3
        );
    }
}
