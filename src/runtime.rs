use std::fmt::Debug;
use std::io::{self, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::{
    batch_runner::{run_batch, BatchRunOptions, BatchRunReport, BatchRunnerError},
    handoff::{
        ingest_research_signals_file, HandoffError, ResearchSignalIngestOptions,
        ResearchSignalIngestReport,
    },
    materialization::{
        materialize_decisions, materialize_orders, submit_orders, DecisionMaterializationOptions,
        DecisionMaterializationReport, MaterializationError, OrderMaterializationOptions,
        OrderMaterializationReport, OrderSubmissionOptions, OrderSubmissionReport,
    },
    observability::{summary_from_store, ObservabilityError, ObservabilitySummary},
    projections::{DecisionProjection, SignalProjection},
    queries::{
        DecisionGovernanceReport, DecisionLineageReport, DecisionPromotionReport,
        DecisionReadiness, ExecutionBoundaryReport, FillReadiness, GovernanceRef,
        GovernanceRefType, OrderLifecycleReport, OrderPromotionReport, OrderSubmissionPolicyReport,
        PromotionNextStep, PromotionPolicyStatus, QueryError, QueryService, SignalGovernanceReport,
        SignalPromotionReport, SignalReadiness,
    },
    store::{JsonlEventStore, StoreError, StoredEvent},
};

const DEFAULT_STORE_PATH: &str = "./var/events.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Summary,
    Signal { signal_id: String },
    Decision { decision_id: String },
    Order { order_id: String },
    Fill { fill_id: String },
    PolicySignal { signal_id: String },
    PolicyDecision { decision_id: String },
    PolicyOrder { order_id: String },
    IngestResearchSignals { input_path: PathBuf },
    MaterializeDecisions,
    MaterializeOrders,
    SubmitOrders,
    RunBatch { research_signals_path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    command: Command,
    store_path: PathBuf,
    format: OutputFormat,
    dry_run: bool,
}

enum ParseOutcome {
    Config(Config),
    Help,
}

#[derive(Debug)]
enum RuntimeError {
    Usage(String),
    NotFound { entity: &'static str, id: String },
    Store(StoreError),
    Query(QueryError),
    Handoff(HandoffError),
    Materialization(MaterializationError),
    Batch(BatchRunnerError),
    Observability(ObservabilityError),
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
            Self::Handoff(error) => write!(f, "{error}"),
            Self::Materialization(error) => write!(f, "{error}"),
            Self::Batch(error) => write!(f, "{error}"),
            Self::Observability(error) => write!(f, "{error}"),
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

pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<String>>();
    match parse_args(args.clone()) {
        Ok(ParseOutcome::Help) => {
            if writeln!(stdout, "{}", usage()).is_err() {
                let _ = writeln!(stderr, "failed to write runtime help");
                return 1;
            }
            0
        }
        Ok(ParseOutcome::Config(config)) => match execute(config.clone()) {
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

fn parse_args(args: Vec<String>) -> Result<ParseOutcome, RuntimeError> {
    let mut args = args.into_iter();
    let _program_name = args.next();

    let mut positionals = Vec::new();
    let mut store_path = PathBuf::from(DEFAULT_STORE_PATH);
    let mut format = OutputFormat::Text;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--store" => {
                let path = args.next().ok_or_else(|| {
                    RuntimeError::Usage("missing value for --store\n\n".to_string() + &usage())
                })?;
                store_path = PathBuf::from(path);
            }
            "--json" => {
                format = OutputFormat::Json;
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--research-signals" => {
                positionals.push(arg);
                let path = args.next().ok_or_else(|| {
                    RuntimeError::Usage(
                        "missing value for --research-signals\n\n".to_string() + &usage(),
                    )
                })?;
                positionals.push(path);
            }
            "-h" | "--help" => {
                return Ok(ParseOutcome::Help);
            }
            _ if arg.starts_with('-') => {
                return Err(RuntimeError::Usage(format!(
                    "unknown flag: {arg}\n\n{}",
                    usage()
                )));
            }
            _ => positionals.push(arg),
        }
    }

    let command = match positionals.as_slice() {
        [] => Command::Summary,
        [single] if single == "summary" => Command::Summary,
        [single, id] if single == "signal" => Command::Signal {
            signal_id: id.clone(),
        },
        [single, id] if single == "decision" => Command::Decision {
            decision_id: id.clone(),
        },
        [single, id] if single == "order" => Command::Order {
            order_id: id.clone(),
        },
        [single, id] if single == "fill" => Command::Fill {
            fill_id: id.clone(),
        },
        [inspect, entity, id] if inspect == "inspect" && entity == "signal" => Command::Signal {
            signal_id: id.clone(),
        },
        [inspect, entity, id] if inspect == "inspect" && entity == "decision" => {
            Command::Decision {
                decision_id: id.clone(),
            }
        }
        [inspect, entity, id] if inspect == "inspect" && entity == "order" => Command::Order {
            order_id: id.clone(),
        },
        [inspect, entity, id] if inspect == "inspect" && entity == "fill" => Command::Fill {
            fill_id: id.clone(),
        },
        [policy, entity, id] if policy == "policy" && entity == "signal" => Command::PolicySignal {
            signal_id: id.clone(),
        },
        [policy, entity, id] if policy == "policy" && entity == "decision" => {
            Command::PolicyDecision {
                decision_id: id.clone(),
            }
        }
        [policy, entity, id] if policy == "policy" && entity == "order" => Command::PolicyOrder {
            order_id: id.clone(),
        },
        [ingest, kind, path] if ingest == "ingest" && kind == "research-signals" => {
            Command::IngestResearchSignals {
                input_path: PathBuf::from(path),
            }
        }
        [materialize, entity] if materialize == "materialize" && entity == "decisions" => {
            Command::MaterializeDecisions
        }
        [materialize, entity] if materialize == "materialize" && entity == "orders" => {
            Command::MaterializeOrders
        }
        [submit, entity] if submit == "submit" && entity == "orders" => Command::SubmitOrders,
        [run, batch, flag, path]
            if run == "run" && batch == "batch" && flag == "--research-signals" =>
        {
            Command::RunBatch {
                research_signals_path: PathBuf::from(path),
            }
        }
        _ => {
            return Err(RuntimeError::Usage(format!(
                "invalid command\n\n{}",
                usage()
            )));
        }
    };

    Ok(ParseOutcome::Config(Config {
        command,
        store_path,
        format,
        dry_run,
    }))
}

fn usage() -> String {
    format!(
        "2EXCAMIM Runtime CLI\n\nUsage:\n  twoexcamim summary [--store PATH] [--json]\n  twoexcamim inspect <signal|decision|order|fill> <id> [--store PATH] [--json]\n  twoexcamim policy <signal|decision|order> <id> [--store PATH] [--json]\n  twoexcamim ingest research-signals <input.parquet> [--store PATH] [--dry-run] [--json]\n  twoexcamim materialize decisions [--store PATH] [--dry-run] [--json]\n  twoexcamim materialize orders [--store PATH] [--dry-run] [--json]\n  twoexcamim submit orders [--store PATH] [--dry-run] [--json]\n  twoexcamim run batch --research-signals <input.parquet> [--store PATH] [--dry-run] [--json]\n\nAliases:\n  twoexcamim signal <id>\n  twoexcamim decision <id>\n  twoexcamim order <id>\n  twoexcamim fill <id>\n\nExamples:\n  twoexcamim summary\n  twoexcamim inspect signal sig-1 --store ./var/events.jsonl\n  twoexcamim policy signal sig-1 --json\n  twoexcamim ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --dry-run\n  twoexcamim materialize decisions --dry-run --json\n  twoexcamim materialize orders --dry-run --json\n  twoexcamim submit orders --dry-run --json\n  twoexcamim run batch --research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --dry-run\n\nDefault store path: {DEFAULT_STORE_PATH}"
    )
}

fn execute(config: Config) -> Result<String, RuntimeError> {
    if config.dry_run
        && !matches!(
            config.command,
            Command::IngestResearchSignals { .. }
                | Command::MaterializeDecisions
                | Command::MaterializeOrders
                | Command::SubmitOrders
                | Command::RunBatch { .. }
        )
    {
        return Err(RuntimeError::Usage(
            "--dry-run is only supported for ingest research-signals, materialize decisions, materialize orders, submit orders and run batch"
                .to_string(),
        ));
    }

    let store = JsonlEventStore::new(&config.store_path)?;
    let query_service = QueryService::new(&store);

    match config.command {
        Command::Summary => render_summary(&store, &config),
        Command::Signal { ref signal_id } => render_signal(&query_service, &config, signal_id),
        Command::Decision { ref decision_id } => {
            render_decision(&query_service, &config, decision_id)
        }
        Command::Order { ref order_id } => render_order(&query_service, &config, order_id),
        Command::Fill { ref fill_id } => render_fill(&query_service, &config, fill_id),
        Command::PolicySignal { ref signal_id } => {
            render_signal_policy_only(&query_service, &config, signal_id)
        }
        Command::PolicyDecision { ref decision_id } => {
            render_decision_policy_only(&query_service, &config, decision_id)
        }
        Command::PolicyOrder { ref order_id } => {
            render_order_policy_only(&query_service, &config, order_id)
        }
        Command::IngestResearchSignals { ref input_path } => {
            render_research_signal_ingest(&store, &config, input_path)
        }
        Command::MaterializeDecisions => render_materialize_decisions(&query_service, &config),
        Command::MaterializeOrders => render_materialize_orders(&query_service, &config),
        Command::SubmitOrders => render_submit_orders(&query_service, &config),
        Command::RunBatch {
            ref research_signals_path,
        } => render_batch_run(&store, &config, research_signals_path),
    }
}

fn render_batch_run(
    store: &JsonlEventStore,
    config: &Config,
    research_signals_path: &PathBuf,
) -> Result<String, RuntimeError> {
    let report = run_batch(
        store,
        research_signals_path,
        BatchRunOptions {
            dry_run: config.dry_run,
        },
    )?;

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                "Batch Run".to_string(),
                format!("store_path: {}", report.store_path.display()),
                format!(
                    "research_signals_path: {}",
                    report.research_signals_path.display()
                ),
                format!("dry_run: {}", report.dry_run),
                format!("batch_trace_id: {}", report.batch_trace_id),
                format!("success: {}", report.success),
                "ingest:".to_string(),
                format!("  rows_read: {}", report.ingest.rows_read),
                format!("  rows_valid: {}", report.ingest.rows_valid),
                format!("  rows_invalid: {}", report.ingest.rows_invalid),
                format!("  events_written: {}", report.ingest.events_written),
                format!("  duplicates: {}", report.ingest.duplicates),
                "materialization:".to_string(),
                format!(
                    "  signals_inspected: {}",
                    report.materialization.signals_inspected
                ),
                format!("  eligible: {}", report.materialization.eligible),
                format!("  skipped: {}", report.materialization.skipped),
                format!("  blocked: {}", report.materialization.blocked),
                format!("  inconsistent: {}", report.materialization.inconsistent),
                format!(
                    "  decisions_materialized: {}",
                    report.materialization.decisions_materialized
                ),
                format!("  duplicates: {}", report.materialization.duplicates),
            ];

            lines.extend(render_summary_section(
                "final_summary:",
                &report.final_summary,
            ));

            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&batch_run_json(&report))?),
    }
}

fn render_materialize_decisions(
    query_service: &QueryService<'_>,
    config: &Config,
) -> Result<String, RuntimeError> {
    let report = materialize_decisions(
        query_service,
        DecisionMaterializationOptions {
            dry_run: config.dry_run,
        },
    )?;

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                "Decision Materialization".to_string(),
                format!("store_path: {}", config.store_path.display()),
                format!("dry_run: {}", report.dry_run),
                format!("batch_trace_id: {}", report.batch_trace_id),
                format!("signals_inspected: {}", report.signals_inspected),
                format!("eligible: {}", report.eligible),
                format!("skipped: {}", report.skipped),
                format!("blocked: {}", report.blocked),
                format!("inconsistent: {}", report.inconsistent),
                format!("decisions_materialized: {}", report.decisions_materialized),
                format!("duplicates: {}", report.duplicates),
                "items:".to_string(),
            ];

            if report.items.is_empty() {
                lines.push("- none".to_string());
            } else {
                for item in &report.items {
                    lines.push(format!(
                        "- signal_id={} disposition={:?} policy_status={} decision_id={} persisted={}",
                        item.signal_id,
                        item.disposition,
                        item.policy_status,
                        display_option(item.candidate_decision_id.as_deref()),
                        item.persisted
                    ));
                    for reason in &item.reasons {
                        lines.push(format!("  reason={reason}"));
                    }
                }
            }

            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &decision_materialization_json(&report, &config.store_path),
        )?),
    }
}

fn render_materialize_orders(
    query_service: &QueryService<'_>,
    config: &Config,
) -> Result<String, RuntimeError> {
    let report = materialize_orders(
        query_service,
        OrderMaterializationOptions {
            dry_run: config.dry_run,
        },
    )?;

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                "Order Materialization".to_string(),
                format!("store_path: {}", config.store_path.display()),
                format!("dry_run: {}", report.dry_run),
                format!("batch_trace_id: {}", report.batch_trace_id),
                format!("decisions_inspected: {}", report.decisions_inspected),
                format!("eligible: {}", report.eligible),
                format!("skipped: {}", report.skipped),
                format!("blocked: {}", report.blocked),
                format!("inconsistent: {}", report.inconsistent),
                format!("orders_registered: {}", report.orders_registered),
                format!("duplicates: {}", report.duplicates),
                "items:".to_string(),
            ];

            if report.items.is_empty() {
                lines.push("- none".to_string());
            } else {
                for item in &report.items {
                    lines.push(format!(
                        "- decision_id={} disposition={:?} policy_status={} order_id={} persisted={}",
                        item.decision_id,
                        item.disposition,
                        item.policy_status,
                        display_option(item.candidate_order_id.as_deref()),
                        item.persisted
                    ));
                    for reason in &item.reasons {
                        lines.push(format!("  reason={reason}"));
                    }
                }
            }

            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&order_materialization_json(
            &report,
            &config.store_path,
        ))?),
    }
}

fn render_submit_orders(
    query_service: &QueryService<'_>,
    config: &Config,
) -> Result<String, RuntimeError> {
    let report = submit_orders(
        query_service,
        OrderSubmissionOptions {
            dry_run: config.dry_run,
        },
    )?;

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                "Order Submission".to_string(),
                format!("store_path: {}", config.store_path.display()),
                format!("dry_run: {}", report.dry_run),
                format!("batch_trace_id: {}", report.batch_trace_id),
                format!("orders_inspected: {}", report.orders_inspected),
                format!("eligible: {}", report.eligible),
                format!("submitted: {}", report.submitted),
                format!("skipped: {}", report.skipped),
                format!("blocked: {}", report.blocked),
                format!("inconsistent: {}", report.inconsistent),
                format!("duplicates: {}", report.duplicates),
                "items:".to_string(),
            ];

            if report.items.is_empty() {
                lines.push("- none".to_string());
            } else {
                for item in &report.items {
                    lines.push(format!(
                        "- order_id={} disposition={:?} policy_status={} persisted={}",
                        item.order_id, item.disposition, item.policy_status, item.persisted
                    ));
                    for reason in &item.reasons {
                        lines.push(format!("  reason={reason}"));
                    }
                }
            }

            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&order_submission_json(
            &report,
            &config.store_path,
        ))?),
    }
}

fn render_signal_policy_only(
    query_service: &QueryService<'_>,
    config: &Config,
    signal_id: &str,
) -> Result<String, RuntimeError> {
    let policy = query_service.signal_promotion_policy(signal_id)?;
    let Some(policy) = policy else {
        return Err(RuntimeError::NotFound {
            entity: "signal",
            id: signal_id.to_string(),
        });
    };

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Signal Policy {}", signal_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_policy_section(
                "Promotion Policy",
                policy.status,
                policy.next_step,
                &policy.reasons,
                &policy.supporting_refs,
                &policy.blocking_refs,
                &policy.notes,
            ));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "signal_policy",
            "store_path": config.store_path.display().to_string(),
            "signal_id": signal_id,
            "promotion_policy": signal_promotion_json(&policy),
        }))?),
    }
}

fn render_decision_policy_only(
    query_service: &QueryService<'_>,
    config: &Config,
    decision_id: &str,
) -> Result<String, RuntimeError> {
    let policy = query_service.decision_promotion_policy(decision_id)?;
    let Some(policy) = policy else {
        return Err(RuntimeError::NotFound {
            entity: "decision",
            id: decision_id.to_string(),
        });
    };

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Decision Policy {}", decision_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_policy_section(
                "Promotion Policy",
                policy.status,
                policy.next_step,
                &policy.reasons,
                &policy.supporting_refs,
                &policy.blocking_refs,
                &policy.notes,
            ));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "decision_policy",
            "store_path": config.store_path.display().to_string(),
            "decision_id": decision_id,
            "promotion_policy": decision_promotion_json(&policy),
        }))?),
    }
}

fn render_order_policy_only(
    query_service: &QueryService<'_>,
    config: &Config,
    order_id: &str,
) -> Result<String, RuntimeError> {
    let policy = query_service.order_promotion_policy(order_id)?;
    let Some(policy) = policy else {
        return Err(RuntimeError::NotFound {
            entity: "order",
            id: order_id.to_string(),
        });
    };

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Order Policy {}", order_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_policy_section(
                "Promotion Policy",
                policy.status,
                policy.next_step,
                &policy.reasons,
                &policy.supporting_refs,
                &policy.blocking_refs,
                &policy.notes,
            ));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "order_policy",
            "store_path": config.store_path.display().to_string(),
            "order_id": order_id,
            "promotion_policy": order_promotion_json(&policy),
        }))?),
    }
}

fn render_research_signal_ingest(
    store: &JsonlEventStore,
    config: &Config,
    input_path: &PathBuf,
) -> Result<String, RuntimeError> {
    let report = ingest_research_signals_file(
        store,
        input_path,
        ResearchSignalIngestOptions {
            dry_run: config.dry_run,
        },
    )?;

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                "Research Signal Ingest".to_string(),
                format!("store_path: {}", config.store_path.display()),
                format!("handoff_schema_version: {}", report.handoff_schema_version),
                format!("input_path: {}", report.input_path.display()),
                format!("dry_run: {}", report.dry_run),
                format!("batch_trace_id: {}", report.batch_trace_id),
                format!("input_file_size_bytes: {}", report.input_file_size_bytes),
                format!("rows_read: {}", report.rows_read),
                format!("rows_valid: {}", report.rows_valid),
                format!("rows_invalid: {}", report.rows_invalid),
                format!("events_written: {}", report.events_written),
                format!("duplicates: {}", report.duplicates),
                format!(
                    "generated_event_types: {}",
                    display_list(&report.generated_event_types)
                ),
                "rejected_reasons:".to_string(),
            ];

            if report.rejected_reasons.is_empty() {
                lines.push("- none".to_string());
            } else {
                for rejected_reason in &report.rejected_reasons {
                    lines.push(format!(
                        "- count={} reason={}",
                        rejected_reason.count, rejected_reason.reason
                    ));
                }
            }

            lines.extend(["ingested_signals:".to_string()]);

            if report.ingested_signals.is_empty() {
                lines.push("- none".to_string());
            } else {
                for signal in &report.ingested_signals {
                    lines.push(format!(
                        "- row={} signal_id={} event_type={} event_written={} deduplicated={}",
                        signal.row_number,
                        signal.signal_id,
                        signal.event_type,
                        signal.event_written,
                        signal.deduplicated
                    ));
                }
            }

            lines.push("rejections:".to_string());
            if report.rejections.is_empty() {
                lines.push("- none".to_string());
            } else {
                for rejection in &report.rejections {
                    lines.push(format!(
                        "- row={} market_id={} reason={}",
                        rejection.row_number,
                        display_option(rejection.market_id.as_deref()),
                        rejection.reason
                    ));
                }
            }

            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&research_signal_ingest_json(
            &report,
            &config.store_path,
        ))?),
    }
}

fn render_summary(store: &JsonlEventStore, config: &Config) -> Result<String, RuntimeError> {
    let summary = summary_from_store(store)?;

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                "Runtime Summary".to_string(),
                format!("store_path: {}", config.store_path.display()),
                format!("total_events: {}", summary.total_events),
                format!("total_signals: {}", summary.total_signals),
                format!("total_decisions: {}", summary.total_decisions),
                format!("confirmed_signals: {}", summary.confirmed_signals),
                format!("vetoed_signals: {}", summary.vetoed_signals),
                format!("decisions_with_fills: {}", summary.decisions_with_fills),
                format!(
                    "decisions_without_fills: {}",
                    summary.decisions_without_fills
                ),
                format!("total_fills: {}", summary.total_fills),
                format!("total_filled_quantity: {}", summary.total_filled_quantity),
                format!("unique_correlation_ids: {}", summary.unique_correlation_ids),
                "event_counts_by_type:".to_string(),
            ];

            for (event_type, count) in summary.event_counts_by_type {
                lines.push(format!("- {event_type}: {count}"));
            }

            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&summary_json(
            &summary,
            &config.store_path,
        ))?),
    }
}

fn render_signal(
    query_service: &QueryService<'_>,
    config: &Config,
    signal_id: &str,
) -> Result<String, RuntimeError> {
    let projection = query_service.signal_projection(signal_id)?;
    let readiness = query_service.signal_readiness(signal_id)?;
    let governance = query_service.signal_governance(signal_id)?;
    let policy = query_service.signal_promotion_policy(signal_id)?;
    let timeline = query_service.timeline_for_signal(signal_id)?;

    if projection.is_none() && readiness.is_none() && governance.is_none() && policy.is_none() {
        return Err(RuntimeError::NotFound {
            entity: "signal",
            id: signal_id.to_string(),
        });
    }

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Signal {}", signal_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_signal_projection_text(projection.as_ref()));
            lines.extend(render_signal_readiness_text(readiness.as_ref()));
            lines.extend(render_signal_governance_text(governance.as_ref()));
            lines.extend(render_signal_policy_text(policy.as_ref()));
            lines.extend(render_timeline_text("Timeline", &timeline));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "signal",
            "store_path": config.store_path.display().to_string(),
            "signal_id": signal_id,
            "projection": projection.as_ref().map(signal_projection_json),
            "readiness": readiness.as_ref().map(signal_readiness_json),
            "governance": governance.as_ref().map(signal_governance_json),
            "promotion_policy": policy.as_ref().map(signal_promotion_json),
            "timeline": event_timeline_json(&timeline),
        }))?),
    }
}

fn render_decision(
    query_service: &QueryService<'_>,
    config: &Config,
    decision_id: &str,
) -> Result<String, RuntimeError> {
    let projection = query_service.decision_projection(decision_id)?;
    let readiness = query_service.decision_readiness(decision_id)?;
    let lineage = query_service.decision_lineage(decision_id)?;
    let governance = query_service.decision_governance(decision_id)?;
    let policy = query_service.decision_promotion_policy(decision_id)?;
    let boundary = query_service.decision_execution_boundary(decision_id)?;
    let timeline = query_service.timeline_for_decision(decision_id)?;

    if projection.is_none()
        && readiness.is_none()
        && lineage.is_none()
        && governance.is_none()
        && policy.is_none()
        && boundary.is_none()
    {
        return Err(RuntimeError::NotFound {
            entity: "decision",
            id: decision_id.to_string(),
        });
    }

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Decision {}", decision_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_decision_projection_text(projection.as_ref()));
            lines.extend(render_decision_readiness_text(readiness.as_ref()));
            lines.extend(render_decision_lineage_text(lineage.as_ref()));
            lines.extend(render_decision_governance_text(governance.as_ref()));
            lines.extend(render_decision_policy_text(policy.as_ref()));
            lines.extend(render_execution_boundary_text(
                "Execution Boundary",
                boundary.as_ref(),
            ));
            lines.extend(render_timeline_text("Timeline", &timeline));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "decision",
            "store_path": config.store_path.display().to_string(),
            "decision_id": decision_id,
            "projection": projection.as_ref().map(decision_projection_json),
            "readiness": readiness.as_ref().map(decision_readiness_json),
            "lineage": lineage.as_ref().map(decision_lineage_json),
            "governance": governance.as_ref().map(decision_governance_json),
            "promotion_policy": policy.as_ref().map(decision_promotion_json),
            "execution_boundary": boundary.as_ref().map(execution_boundary_json),
            "timeline": event_timeline_json(&timeline),
        }))?),
    }
}

fn render_order(
    query_service: &QueryService<'_>,
    config: &Config,
    order_id: &str,
) -> Result<String, RuntimeError> {
    let lifecycle = query_service.order_lifecycle(order_id)?;
    let policy = query_service.order_promotion_policy(order_id)?;
    let submission_policy = query_service.order_submission_policy(order_id)?;
    let all_events = query_service.all_events()?;
    let related_events = events_for_order(&all_events, order_id);

    if lifecycle.is_none()
        && policy.is_none()
        && submission_policy.is_none()
        && related_events.is_empty()
    {
        return Err(RuntimeError::NotFound {
            entity: "order",
            id: order_id.to_string(),
        });
    }

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Order {}", order_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_order_lifecycle_text(lifecycle.as_ref()));
            lines.extend(render_order_policy_text(policy.as_ref()));
            lines.extend(render_order_submission_policy_text(
                submission_policy.as_ref(),
            ));
            lines.extend(render_timeline_text("Related Events", &related_events));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "order",
            "store_path": config.store_path.display().to_string(),
            "order_id": order_id,
            "lifecycle": lifecycle.as_ref().map(order_lifecycle_json),
            "promotion_policy": policy.as_ref().map(order_promotion_json),
            "submission_policy": submission_policy.as_ref().map(order_submission_policy_json),
            "related_events": event_timeline_json(&related_events),
        }))?),
    }
}

fn render_fill(
    query_service: &QueryService<'_>,
    config: &Config,
    fill_id: &str,
) -> Result<String, RuntimeError> {
    let readiness = query_service.fill_readiness(fill_id)?;
    let boundary = query_service.fill_execution_boundary(fill_id)?;
    let all_events = query_service.all_events()?;
    let matching_events = events_for_fill(&all_events, fill_id);

    if readiness.is_none() && boundary.is_none() && matching_events.is_empty() {
        return Err(RuntimeError::NotFound {
            entity: "fill",
            id: fill_id.to_string(),
        });
    }

    match config.format {
        OutputFormat::Text => {
            let mut lines = vec![
                format!("Fill {}", fill_id),
                format!("store_path: {}", config.store_path.display()),
            ];
            lines.extend(render_fill_readiness_text(readiness.as_ref()));
            lines.extend(render_execution_boundary_text(
                "Execution Boundary",
                boundary.as_ref(),
            ));
            lines.extend(render_timeline_text(
                "Matching Fill Events",
                &matching_events,
            ));
            Ok(lines.join("\n"))
        }
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json!({
            "kind": "fill",
            "store_path": config.store_path.display().to_string(),
            "fill_id": fill_id,
            "readiness": readiness.as_ref().map(fill_readiness_json),
            "execution_boundary": boundary.as_ref().map(execution_boundary_json),
            "matching_events": event_timeline_json(&matching_events),
        }))?),
    }
}

fn render_signal_projection_text(projection: Option<&SignalProjection>) -> Vec<String> {
    let Some(projection) = projection else {
        return vec!["Projection: none".to_string()];
    };

    vec![
        "Projection".to_string(),
        format!("status.generated: {}", projection.generated),
        format!("status.confirmed: {}", projection.confirmed),
        format!("status.vetoed: {}", projection.vetoed),
        format!(
            "hypothesis_id: {}",
            display_option(projection.hypothesis_id.as_deref())
        ),
        format!(
            "instrument: {}",
            display_option(projection.instrument.as_deref())
        ),
        format!(
            "timeframe: {}",
            display_option(projection.timeframe.as_deref())
        ),
        format!(
            "correlation_id: {}",
            display_option(projection.correlation_id.as_deref())
        ),
        format!("last_event_type: {}", projection.last_event_type.as_str()),
        format!("decision_ids: {}", display_list(&projection.decision_ids)),
    ]
}

fn render_decision_projection_text(projection: Option<&DecisionProjection>) -> Vec<String> {
    let Some(projection) = projection else {
        return vec!["Projection: none".to_string()];
    };

    vec![
        "Projection".to_string(),
        format!("status.formed: {}", projection.formed),
        format!("status.vetoed: {}", projection.vetoed),
        format!(
            "instrument: {}",
            display_option(projection.instrument.as_deref())
        ),
        format!(
            "action: {}",
            display_debug_option(projection.action.as_ref())
        ),
        format!("side: {}", display_debug_option(projection.side.as_ref())),
        format!("fills_count: {}", projection.fills_count),
        format!("filled_quantity: {}", projection.filled_quantity),
        format!(
            "average_fill_price: {}",
            display_option_number(projection.average_fill_price)
        ),
        format!(
            "correlation_id: {}",
            display_option(projection.correlation_id.as_deref())
        ),
        format!("last_event_type: {}", projection.last_event_type.as_str()),
        format!("order_ids: {}", display_list(&projection.order_ids)),
    ]
}

fn render_signal_readiness_text(report: Option<&SignalReadiness>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Readiness: none".to_string()];
    };

    render_reason_section("Readiness", &report.status, &report.reasons)
}

fn render_decision_readiness_text(report: Option<&DecisionReadiness>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Readiness: none".to_string()];
    };

    render_reason_section("Readiness", &report.status, &report.reasons)
}

fn render_fill_readiness_text(report: Option<&FillReadiness>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Readiness: none".to_string()];
    };

    render_reason_section("Readiness", &report.status, &report.reasons)
}

fn render_signal_governance_text(report: Option<&SignalGovernanceReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Governance: none".to_string()];
    };

    render_report_with_refs(
        "Governance",
        &report.status,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn render_decision_governance_text(report: Option<&DecisionGovernanceReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Governance: none".to_string()];
    };

    render_report_with_refs(
        "Governance",
        &report.status,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn render_signal_policy_text(report: Option<&SignalPromotionReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Promotion Policy: none".to_string()];
    };

    render_policy_section(
        "Promotion Policy",
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn render_decision_policy_text(report: Option<&DecisionPromotionReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Promotion Policy: none".to_string()];
    };

    render_policy_section(
        "Promotion Policy",
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn render_order_policy_text(report: Option<&OrderPromotionReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Promotion Policy: none".to_string()];
    };

    render_policy_section(
        "Promotion Policy",
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn render_order_submission_policy_text(
    report: Option<&OrderSubmissionPolicyReport>,
) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Submission Policy: none".to_string()];
    };

    render_policy_section(
        "Submission Policy",
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn render_decision_lineage_text(report: Option<&DecisionLineageReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Lineage: none".to_string()];
    };

    let mut lines = vec![
        "Lineage".to_string(),
        format!("status: {:?}", report.status),
        format!(
            "upstream.signal_ids: {}",
            display_list(&report.upstream_refs.signal_ids)
        ),
        format!(
            "upstream.hypothesis_ids: {}",
            display_list(&report.upstream_refs.hypothesis_ids)
        ),
        format!(
            "upstream.decision_vetoes: {}",
            display_veto_list(&report.upstream_refs.decision_vetoes)
        ),
        format!(
            "upstream.signal_vetoes: {}",
            display_veto_list(&report.upstream_refs.signal_vetoes)
        ),
        format!(
            "downstream.local_order_ids: {}",
            display_list(&report.downstream_refs.local_order_ids)
        ),
        format!(
            "downstream.submitted_order_ids: {}",
            display_list(&report.downstream_refs.submitted_order_ids)
        ),
        format!(
            "downstream.fill_ids: {}",
            display_list(&report.downstream_refs.fill_ids)
        ),
        format!(
            "downstream.order_ids: {}",
            display_list(&report.downstream_refs.order_ids)
        ),
        "reasons:".to_string(),
    ];

    if report.reasons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for reason in &report.reasons {
            lines.push(format!("- {reason:?}"));
        }
    }

    lines.push("notes:".to_string());
    if report.notes.is_empty() {
        lines.push("- none".to_string());
    } else {
        for note in &report.notes {
            lines.push(format!("- {note}"));
        }
    }

    lines
}

fn render_order_lifecycle_text(report: Option<&OrderLifecycleReport>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Lifecycle: none".to_string()];
    };

    let mut lines = vec![
        "Lifecycle".to_string(),
        format!("status: {:?}", report.status),
        format!("decision_refs: {}", display_list(&report.decision_refs)),
        format!(
            "observed_fill_ids: {}",
            display_list(&report.observed_fill_ids)
        ),
        format!("venue_refs: {}", display_list(&report.venue_refs)),
        "reasons:".to_string(),
    ];

    if report.reasons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for reason in &report.reasons {
            lines.push(format!("- {reason:?}"));
        }
    }

    lines.push("notes:".to_string());
    if report.notes.is_empty() {
        lines.push("- none".to_string());
    } else {
        for note in &report.notes {
            lines.push(format!("- {note}"));
        }
    }

    lines
}

fn render_execution_boundary_text(
    title: &str,
    report: Option<&ExecutionBoundaryReport>,
) -> Vec<String> {
    let Some(report) = report else {
        return vec![format!("{title}: none")];
    };

    let mut lines = vec![
        title.to_string(),
        format!("status: {:?}", report.status),
        format!("decision_refs: {}", display_list(&report.decision_refs)),
        format!(
            "observed_order_ids: {}",
            display_list(&report.observed_order_ids)
        ),
        format!(
            "submitted_order_ids: {}",
            display_list(&report.submitted_order_ids)
        ),
        format!(
            "observed_fill_ids: {}",
            display_list(&report.observed_fill_ids)
        ),
        "reasons:".to_string(),
    ];

    if report.reasons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for reason in &report.reasons {
            lines.push(format!("- {reason:?}"));
        }
    }

    lines.push("notes:".to_string());
    if report.notes.is_empty() {
        lines.push("- none".to_string());
    } else {
        for note in &report.notes {
            lines.push(format!("- {note}"));
        }
    }

    lines
}

fn render_reason_section<TStatus, TReason>(
    title: &str,
    status: &TStatus,
    reasons: &[TReason],
) -> Vec<String>
where
    TStatus: Debug,
    TReason: Debug,
{
    let mut lines = vec![
        title.to_string(),
        format!("status: {status:?}"),
        "reasons:".to_string(),
    ];

    if reasons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for reason in reasons {
            lines.push(format!("- {reason:?}"));
        }
    }

    lines
}

fn render_report_with_refs<TStatus, TReason>(
    title: &str,
    status: &TStatus,
    reasons: &[TReason],
    supporting_refs: &[GovernanceRef],
    blocking_refs: &[GovernanceRef],
    notes: &[String],
) -> Vec<String>
where
    TStatus: Debug,
    TReason: Debug,
{
    let mut lines = render_reason_section(title, status, reasons);
    lines.push(format!(
        "supporting_refs: {}",
        display_ref_list(supporting_refs)
    ));
    lines.push(format!(
        "blocking_refs: {}",
        display_ref_list(blocking_refs)
    ));
    lines.push("notes:".to_string());

    if notes.is_empty() {
        lines.push("- none".to_string());
    } else {
        for note in notes {
            lines.push(format!("- {note}"));
        }
    }

    lines
}

fn render_policy_section(
    title: &str,
    status: PromotionPolicyStatus,
    next_step: Option<PromotionNextStep>,
    reasons: &[String],
    supporting_refs: &[GovernanceRef],
    blocking_refs: &[GovernanceRef],
    notes: &[String],
) -> Vec<String> {
    let mut lines = vec![
        title.to_string(),
        format!("status: {:?}", status),
        format!("next_step: {}", display_debug_option(next_step.as_ref())),
        "reasons:".to_string(),
    ];

    if reasons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for reason in reasons {
            lines.push(format!("- {reason}"));
        }
    }

    lines.push(format!(
        "supporting_refs: {}",
        display_ref_list(supporting_refs)
    ));
    lines.push(format!(
        "blocking_refs: {}",
        display_ref_list(blocking_refs)
    ));
    lines.push("notes:".to_string());

    if notes.is_empty() {
        lines.push("- none".to_string());
    } else {
        for note in notes {
            lines.push(format!("- {note}"));
        }
    }

    lines
}

fn render_timeline_text(title: &str, events: &[StoredEvent]) -> Vec<String> {
    let mut lines = vec![title.to_string()];

    if events.is_empty() {
        lines.push("- none".to_string());
        return lines;
    }

    for event in events {
        lines.push(format!("- {}", render_event_summary(event)));
    }

    lines
}

fn render_event_summary(event: &StoredEvent) -> String {
    format!(
        "{} | {} | event_id={} | signal_id={} | decision_id={} | order_id={} | correlation_id={}",
        event.occurred_at.to_rfc3339(),
        event.event_type.as_str(),
        event.event_id,
        display_option(event.linkage.signal_id.as_deref()),
        display_option(event.linkage.decision_id.as_deref()),
        display_option(event.linkage.order_id.as_deref()),
        display_option(event.linkage.correlation_id.as_deref()),
    )
}

fn signal_projection_json(projection: &SignalProjection) -> Value {
    json!({
        "signal_id": projection.signal_id,
        "hypothesis_id": projection.hypothesis_id,
        "instrument": projection.instrument,
        "timeframe": projection.timeframe,
        "generated": projection.generated,
        "confirmed": projection.confirmed,
        "vetoed": projection.vetoed,
        "decision_ids": projection.decision_ids,
        "last_event_type": projection.last_event_type.as_str(),
        "correlation_id": projection.correlation_id,
    })
}

fn decision_projection_json(projection: &DecisionProjection) -> Value {
    json!({
        "decision_id": projection.decision_id,
        "instrument": projection.instrument,
        "action": projection.action.as_ref().map(|value| format!("{value:?}")),
        "side": projection.side.as_ref().map(|value| format!("{value:?}")),
        "formed": projection.formed,
        "vetoed": projection.vetoed,
        "fills_count": projection.fills_count,
        "filled_quantity": projection.filled_quantity,
        "average_fill_price": projection.average_fill_price,
        "order_ids": projection.order_ids,
        "last_event_type": projection.last_event_type.as_str(),
        "correlation_id": projection.correlation_id,
    })
}

fn signal_readiness_json(report: &SignalReadiness) -> Value {
    json!({
        "signal_id": report.signal_id,
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
    })
}

fn decision_readiness_json(report: &DecisionReadiness) -> Value {
    json!({
        "decision_id": report.decision_id,
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
    })
}

fn fill_readiness_json(report: &FillReadiness) -> Value {
    json!({
        "fill_id": report.fill_id,
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
    })
}

fn signal_governance_json(report: &SignalGovernanceReport) -> Value {
    json!({
        "ref_id": report.ref_id,
        "ref_type": format!("{:?}", report.ref_type),
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
        "supporting_refs": governance_refs_json(&report.supporting_refs),
        "blocking_refs": governance_refs_json(&report.blocking_refs),
        "notes": report.notes,
    })
}

fn decision_governance_json(report: &DecisionGovernanceReport) -> Value {
    json!({
        "ref_id": report.ref_id,
        "ref_type": format!("{:?}", report.ref_type),
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
        "supporting_refs": governance_refs_json(&report.supporting_refs),
        "blocking_refs": governance_refs_json(&report.blocking_refs),
        "notes": report.notes,
    })
}

fn signal_promotion_json(report: &SignalPromotionReport) -> Value {
    promotion_json(
        &report.ref_id,
        report.ref_type,
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn decision_promotion_json(report: &DecisionPromotionReport) -> Value {
    promotion_json(
        &report.ref_id,
        report.ref_type,
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn order_promotion_json(report: &OrderPromotionReport) -> Value {
    promotion_json(
        &report.ref_id,
        report.ref_type,
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn order_submission_policy_json(report: &OrderSubmissionPolicyReport) -> Value {
    promotion_json(
        &report.ref_id,
        report.ref_type,
        report.status,
        report.next_step,
        &report.reasons,
        &report.supporting_refs,
        &report.blocking_refs,
        &report.notes,
    )
}

fn promotion_json(
    ref_id: &str,
    ref_type: GovernanceRefType,
    status: PromotionPolicyStatus,
    next_step: Option<PromotionNextStep>,
    reasons: &[String],
    supporting_refs: &[GovernanceRef],
    blocking_refs: &[GovernanceRef],
    notes: &[String],
) -> Value {
    json!({
        "ref_id": ref_id,
        "ref_type": format!("{:?}", ref_type),
        "status": format!("{:?}", status),
        "next_step": next_step.map(|step| format!("{step:?}")),
        "reasons": reasons,
        "supporting_refs": governance_refs_json(supporting_refs),
        "blocking_refs": governance_refs_json(blocking_refs),
        "notes": notes,
    })
}

fn decision_lineage_json(report: &DecisionLineageReport) -> Value {
    json!({
        "decision_id": report.decision_id,
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
        "upstream_refs": {
            "signal_ids": report.upstream_refs.signal_ids,
            "hypothesis_ids": report.upstream_refs.hypothesis_ids,
            "decision_vetoes": report.upstream_refs.decision_vetoes.iter().map(|veto| json!({
                "veto_id": veto.veto_id,
                "target_id": veto.target_id,
                "reason_code": veto.reason_code,
            })).collect::<Vec<_>>(),
            "signal_vetoes": report.upstream_refs.signal_vetoes.iter().map(|veto| json!({
                "veto_id": veto.veto_id,
                "target_id": veto.target_id,
                "reason_code": veto.reason_code,
            })).collect::<Vec<_>>(),
        },
        "downstream_refs": {
            "order_ids": report.downstream_refs.order_ids,
            "local_order_ids": report.downstream_refs.local_order_ids,
            "submitted_order_ids": report.downstream_refs.submitted_order_ids,
            "fill_ids": report.downstream_refs.fill_ids,
        },
        "notes": report.notes,
    })
}

fn execution_boundary_json(report: &ExecutionBoundaryReport) -> Value {
    json!({
        "primary_ref_id": report.primary_ref_id,
        "primary_ref_type": format!("{:?}", report.primary_ref_type),
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
        "decision_refs": report.decision_refs,
        "observed_order_ids": report.observed_order_ids,
        "submitted_order_ids": report.submitted_order_ids,
        "observed_fill_ids": report.observed_fill_ids,
        "notes": report.notes,
    })
}

fn order_lifecycle_json(report: &OrderLifecycleReport) -> Value {
    json!({
        "order_id": report.order_id,
        "status": format!("{:?}", report.status),
        "reasons": debug_list(&report.reasons),
        "decision_refs": report.decision_refs,
        "observed_fill_ids": report.observed_fill_ids,
        "venue_refs": report.venue_refs,
        "notes": report.notes,
    })
}

fn governance_refs_json(refs: &[GovernanceRef]) -> Value {
    Value::Array(
        refs.iter()
            .map(|governance_ref| {
                json!({
                    "ref_id": governance_ref.ref_id,
                    "ref_type": format!("{:?}", governance_ref.ref_type),
                })
            })
            .collect(),
    )
}

fn event_timeline_json(events: &[StoredEvent]) -> Value {
    Value::Array(
        events
            .iter()
            .map(|event| {
                json!({
                    "event_id": event.event_id,
                    "event_type": event.event_type.as_str(),
                    "occurred_at": event.occurred_at,
                    "produced_by": event.produced_by,
                    "idempotency_key": event.idempotency_key,
                    "aggregate_key": event.aggregate_key,
                    "linkage": event.linkage,
                    "provenance": event.provenance,
                    "payload": event.payload,
                })
            })
            .collect(),
    )
}

fn research_signal_ingest_json(report: &ResearchSignalIngestReport, store_path: &PathBuf) -> Value {
    json!({
        "kind": "ingest_research_signals",
        "store_path": store_path.display().to_string(),
        "handoff_schema_version": report.handoff_schema_version,
        "input_path": report.input_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "input_file_size_bytes": report.input_file_size_bytes,
        "rows_read": report.rows_read,
        "rows_valid": report.rows_valid,
        "rows_invalid": report.rows_invalid,
        "events_written": report.events_written,
        "duplicates": report.duplicates,
        "generated_event_types": report.generated_event_types,
        "rejected_reasons": report.rejected_reasons,
        "ingested_signals": report.ingested_signals,
        "rejections": report.rejections,
    })
}

fn decision_materialization_json(
    report: &DecisionMaterializationReport,
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "materialize_decisions",
        "store_path": store_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "signals_inspected": report.signals_inspected,
        "eligible": report.eligible,
        "skipped": report.skipped,
        "blocked": report.blocked,
        "inconsistent": report.inconsistent,
        "decisions_materialized": report.decisions_materialized,
        "duplicates": report.duplicates,
        "items": report.items.iter().map(decision_materialization_item_json).collect::<Vec<_>>(),
    })
}

fn order_materialization_json(report: &OrderMaterializationReport, store_path: &PathBuf) -> Value {
    json!({
        "kind": "materialize_orders",
        "store_path": store_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "decisions_inspected": report.decisions_inspected,
        "eligible": report.eligible,
        "skipped": report.skipped,
        "blocked": report.blocked,
        "inconsistent": report.inconsistent,
        "orders_registered": report.orders_registered,
        "duplicates": report.duplicates,
        "items": report.items.iter().map(order_materialization_item_json).collect::<Vec<_>>(),
    })
}

fn order_submission_json(report: &OrderSubmissionReport, store_path: &PathBuf) -> Value {
    json!({
        "kind": "submit_orders",
        "store_path": store_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "orders_inspected": report.orders_inspected,
        "eligible": report.eligible,
        "submitted": report.submitted,
        "skipped": report.skipped,
        "blocked": report.blocked,
        "inconsistent": report.inconsistent,
        "duplicates": report.duplicates,
        "items": report.items.iter().map(order_submission_item_json).collect::<Vec<_>>(),
    })
}

fn summary_json(summary: &ObservabilitySummary, store_path: &PathBuf) -> Value {
    json!({
        "kind": "summary",
        "store_path": store_path.display().to_string(),
        "summary": summary,
    })
}

fn batch_run_json(report: &BatchRunReport) -> Value {
    json!({
        "kind": "batch_run",
        "store_path": report.store_path.display().to_string(),
        "research_signals_path": report.research_signals_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "success": report.success,
        "ingest": research_signal_ingest_json(&report.ingest, &report.store_path),
        "materialization": decision_materialization_json(&report.materialization, &report.store_path),
        "final_summary": summary_json(&report.final_summary, &report.store_path),
    })
}

fn decision_materialization_item_json(
    item: &crate::materialization::DecisionMaterializationItem,
) -> Value {
    json!({
        "signal_id": item.signal_id,
        "policy_status": item.policy_status,
        "disposition": format!("{:?}", item.disposition),
        "candidate_decision_id": item.candidate_decision_id,
        "persisted": item.persisted,
        "reasons": item.reasons,
        "notes": item.notes,
    })
}

fn order_materialization_item_json(
    item: &crate::materialization::OrderMaterializationItem,
) -> Value {
    json!({
        "decision_id": item.decision_id,
        "policy_status": item.policy_status,
        "disposition": format!("{:?}", item.disposition),
        "candidate_order_id": item.candidate_order_id,
        "persisted": item.persisted,
        "reasons": item.reasons,
        "notes": item.notes,
    })
}

fn order_submission_item_json(item: &crate::materialization::OrderSubmissionItem) -> Value {
    json!({
        "order_id": item.order_id,
        "policy_status": item.policy_status,
        "disposition": format!("{:?}", item.disposition),
        "persisted": item.persisted,
        "reasons": item.reasons,
        "notes": item.notes,
    })
}

fn events_for_order(events: &[StoredEvent], order_id: &str) -> Vec<StoredEvent> {
    events
        .iter()
        .filter(|event| event.linkage.order_id.as_deref() == Some(order_id))
        .cloned()
        .collect()
}

fn events_for_fill(events: &[StoredEvent], fill_id: &str) -> Vec<StoredEvent> {
    events
        .iter()
        .filter(|event| {
            event.payload.get("fill_id").and_then(Value::as_str) == Some(fill_id)
                && event.event_type.as_str() == "fill.received"
        })
        .cloned()
        .collect()
}

fn debug_list<T>(items: &[T]) -> Vec<String>
where
    T: Debug,
{
    items.iter().map(|item| format!("{item:?}")).collect()
}

fn display_option(value: Option<&str>) -> String {
    value.unwrap_or("none").to_string()
}

fn display_option_number(value: Option<f64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".into())
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn display_ref_list(values: &[GovernanceRef]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }

    values
        .iter()
        .map(|value| format!("{:?}:{}", value.ref_type, value.ref_id))
        .collect::<Vec<_>>()
        .join(", ")
}

fn display_veto_list(values: &[crate::queries::LineageVetoRef]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }

    values
        .iter()
        .map(|value| {
            format!(
                "{}:{}:{}",
                value.veto_id, value.target_id, value.reason_code
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn display_debug_option<T>(value: Option<&T>) -> String
where
    T: Debug,
{
    value
        .map(|value| format!("{value:?}"))
        .unwrap_or_else(|| "none".to_string())
}

fn render_summary_section(title: &str, summary: &ObservabilitySummary) -> Vec<String> {
    let mut lines = vec![
        title.to_string(),
        format!("  total_events: {}", summary.total_events),
        format!("  total_signals: {}", summary.total_signals),
        format!("  total_decisions: {}", summary.total_decisions),
        format!("  confirmed_signals: {}", summary.confirmed_signals),
        format!("  vetoed_signals: {}", summary.vetoed_signals),
        format!("  decisions_with_fills: {}", summary.decisions_with_fills),
        format!(
            "  decisions_without_fills: {}",
            summary.decisions_without_fills
        ),
        format!("  total_fills: {}", summary.total_fills),
        format!("  total_filled_quantity: {}", summary.total_filled_quantity),
        format!(
            "  unique_correlation_ids: {}",
            summary.unique_correlation_ids
        ),
        "  event_counts_by_type:".to_string(),
    ];

    for (event_type, count) in &summary.event_counts_by_type {
        lines.push(format!("  - {event_type}: {count}"));
    }

    lines
}

fn exit_code_for_error(error: &RuntimeError) -> i32 {
    match error {
        RuntimeError::Usage(_) => 2,
        RuntimeError::NotFound { .. } => 3,
        RuntimeError::Store(_)
        | RuntimeError::Query(_)
        | RuntimeError::Handoff(_)
        | RuntimeError::Materialization(_)
        | RuntimeError::Batch(_)
        | RuntimeError::Observability(_)
        | RuntimeError::Io(_)
        | RuntimeError::Json(_) => 1,
    }
}

fn write_runtime_error(stderr: &mut dyn Write, config: Option<&Config>, error: &RuntimeError) {
    let use_json = config.is_some_and(|config| config.format == OutputFormat::Json);
    if use_json {
        let payload = json!({
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
        let payload = json!({
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
