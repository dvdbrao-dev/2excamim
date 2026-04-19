use std::path::PathBuf;

use crate::{
    agents::{
        advise_signal_families, compare_confirmation_quality, load_confirmed_signal_ids_from_store,
        load_generated_signals_from_store, load_snapshots_jsonl,
        materialize_confirmation_readiness, propose_confirmation_policy,
        run_confirmation_walkforward_from_store, sweep_confirmation_policy,
        write_confirmation_policy_proposal, write_confirmation_readiness_report,
        AdvisorySignalFamilyMetrics, ConfirmationComparisonReport, ConfirmationEraSplit,
        ConfirmationPolicy, ConfirmationPolicyAdvisory, ConfirmationPolicyAdvisoryConfig,
        ConfirmationPolicyProposalConfig, ConfirmationReadinessConfig, ConfirmationRunner,
        ConfirmationScorecard, ConfirmationWalkForwardConfig,
    },
    batch_runner::{run_batch, BatchRunOptions},
    commands::ConfirmSignalCommand,
    dashboard::{serve_dashboard_http, write_dashboard, DashboardConfig},
    events::{Provenance, SourceKind},
    execution::{
        project_paper_ledger, run_paper_decisions, submit_paper_order_and_map_fill,
        CliPolymarketPaperBackend, FixturePolymarketPaperBackend, PaperDecisionRunConfig,
        PaperExecutionRequest,
    },
    handoff::{ingest_research_signals_file, ResearchSignalIngestOptions},
    materialization::{
        materialize_decisions, materialize_orders, observe_fill, submit_orders,
        DecisionMaterializationOptions, FillObservationOptions, FillObservationRequest,
        OrderMaterializationOptions, OrderSubmissionOptions,
    },
    observability::summary_from_store,
    operations::{write_operational_summary, OperationalSummaryConfig, OperationalSummaryFormat},
    queries::QueryService,
    store::{JsonlEventStore, StoredEvent},
};
use serde_json::json;

use super::{
    cli_parser::usage, json_renderer, text_renderer, Command, Config, OutputFormat,
    PaperPipelineReport, RuntimeError, DEFAULT_DASHBOARD_HOST, DEFAULT_DASHBOARD_PORT,
    DEFAULT_OPERATIONAL_SUMMARY_JSON_PATH, DEFAULT_OPERATIONAL_SUMMARY_MARKDOWN_PATH,
    DEFAULT_READINESS_PATH,
};

pub(crate) fn execute(config: Config) -> Result<String, RuntimeError> {
    if config.dry_run
        && !matches!(
            config.command,
            Command::IngestResearchSignals { .. }
                | Command::MaterializeDecisions
                | Command::MaterializeOrders
                | Command::SubmitOrders
                | Command::ObserveFill { .. }
                | Command::SimulatePaperFill { .. }
                | Command::RunPaperDecisions
                | Command::RunPaperPipeline { .. }
                | Command::RunBatch { .. }
        )
    {
        return Err(RuntimeError::Usage(
            "--dry-run is only supported for ingest research-signals, materialize decisions, materialize orders, submit orders, observe fill, simulate-paper-fill, run-paper-decisions, run-paper-pipeline and run batch"
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
        Command::ObserveFill { ref request } => {
            render_observe_fill(&query_service, &config, request)
        }
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
        Command::ConfirmSignal {
            ref signal_id,
            ref confirmed_by,
            ref confirmation_reasons,
            ref rejection_reasons,
        } => render_confirm_signal(
            &store,
            &config,
            signal_id,
            confirmed_by,
            confirmation_reasons.as_deref(),
            rejection_reasons.as_deref(),
        ),
        Command::ConfirmSignals => render_confirm_signals(&store, &config),
        Command::MeasureConfirmationOutcomes => {
            render_measure_confirmation_outcomes(&store, &config)
        }
        Command::EvaluateConfirmationPolicy => render_evaluate_confirmation_policy(&store, &config),
        Command::WalkForwardConfirmationPolicy => {
            render_walkforward_confirmation_policy(&store, &config)
        }
        Command::ProposeConfirmationPolicy => render_propose_confirmation_policy(&store, &config),
        Command::MaterializeConfirmationReadiness => {
            render_materialize_confirmation_readiness(&store, &config)
        }
        Command::SimulatePaperFill { ref request } => {
            render_simulate_paper_fill(&query_service, &config, request)
        }
        Command::RunPaperDecisions => render_run_paper_decisions(&query_service, &config),
        Command::RunPaperPipeline {
            ref research_signals_path,
        } => render_run_paper_pipeline(&store, &query_service, &config, research_signals_path),
        Command::ServeDashboard => render_serve_dashboard(&config),
        Command::GenerateOperationalSummary => render_generate_operational_summary(&config),
        Command::ShowPaperLedger => render_show_paper_ledger(&query_service, &config),
        Command::MaterializeDecisions => render_materialize_decisions(&query_service, &config),
        Command::MaterializeOrders => render_materialize_orders(&query_service, &config),
        Command::SubmitOrders => render_submit_orders(&query_service, &config),
        Command::RunBatch {
            ref research_signals_path,
        } => render_batch_run(&store, &config, research_signals_path),
    }
}

fn render_confirm_signal(
    store: &JsonlEventStore,
    config: &Config,
    signal_id: &str,
    confirmed_by: &str,
    confirmation_reasons: Option<&[String]>,
    rejection_reasons: Option<&[String]>,
) -> Result<String, RuntimeError> {
    let generated = store
        .read_all()?
        .into_iter()
        .rev()
        .find(|event| {
            event.event_type.as_str() == "signal.generated"
                && event
                    .payload
                    .get("signal_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(signal_id)
        })
        .ok_or_else(|| RuntimeError::NotFound {
            entity: "signal",
            id: signal_id.to_string(),
        })?;

    let command = ConfirmSignalCommand {
        produced_by: "runtime.agent.confirmation".into(),
        provenance: Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: None,
            producer_run_id: None,
            actor: Some(confirmed_by.to_string()),
            trace_id: Some(format!("runtime.confirm.signal:{signal_id}")),
            notes: Some("confirmed via runtime CLI".into()),
        },
        aggregate_key: generated.aggregate_key.clone(),
        signal_id: signal_id.to_string(),
        hypothesis_id: generated.linkage.hypothesis_id.clone(),
        confirmed_by: confirmed_by.to_string(),
        confirmation_reason: Some("confirmed via runtime CLI".into()),
        confirmation_score: generated
            .payload
            .get("strength")
            .and_then(serde_json::Value::as_f64),
        parent_event_id: Some(generated.event_id.clone()),
        correlation_id: generated.linkage.correlation_id.clone(),
    };

    let envelope = command
        .execute()
        .map_err(|error| RuntimeError::Usage(error.to_string()))?;
    let mut stored =
        StoredEvent::try_from(&envelope).map_err(|error| RuntimeError::Usage(error.to_string()))?;
    if confirmation_reasons.is_some() || rejection_reasons.is_some() {
        if let Some(payload) = stored.payload.as_object_mut() {
            if let Some(reasons) = confirmation_reasons {
                payload.insert("confirmation_reasons".into(), json!(reasons));
            }
            if let Some(reasons) = rejection_reasons {
                payload.insert("rejection_reasons".into(), json!(reasons));
            }
        }
    }
    let persisted = if config.dry_run {
        false
    } else {
        store.append_event(&stored)?
    };

    match config.format {
        OutputFormat::Text => Ok(format!(
            "Confirm Signal\nsignal_id: {signal_id}\nconfirmed_by: {confirmed_by}\npersisted: {persisted}\nidempotency_key: {}",
            stored.idempotency_key
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&serde_json::json!({
            "kind": "confirm_signal",
            "signal_id": signal_id,
            "confirmed_by": confirmed_by,
            "confirmation_reasons": confirmation_reasons,
            "rejection_reasons": rejection_reasons,
            "persisted": persisted,
            "idempotency_key": stored.idempotency_key
        }))?),
    }
}

fn render_confirm_signals(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let runner = confirmation_runner_from_config(store, config)?;
    let report = runner.run()?;
    let scorecard = ConfirmationScorecard::from_report(&report);

    match config.format {
        OutputFormat::Text => Ok(text_renderer::confirm_signals(
            &report,
            &scorecard,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::confirmation_run(&report, &scorecard, &config.store_path),
        )?),
    }
}

fn confirmation_runner_from_config<'a>(
    store: &'a JsonlEventStore,
    config: &Config,
) -> Result<ConfirmationRunner<'a>, RuntimeError> {
    let runner = ConfirmationRunner::new(store, crate::ConfirmationAgent::new(Default::default()))
        .with_dry_run(config.dry_run);
    if let Some(policy_file) = &config.policy_file {
        Ok(runner.with_policy(ConfirmationPolicy::from_file(policy_file)?))
    } else {
        Ok(runner)
    }
}

fn render_measure_confirmation_outcomes(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let runner = crate::ConfirmationOutcomeRunner::new(
        store,
        &config.snapshots_path,
        crate::ConfirmationOutcomeConfig {
            evaluation_horizon_seconds: config.horizon_seconds,
            delta_threshold: config.delta_threshold,
        },
    );
    let report = runner.run()?;
    let scorecard = crate::ConfirmationOutcomeScorecard::from_records(&report.outcomes);

    match config.format {
        OutputFormat::Text => Ok(text_renderer::measure_confirmation_outcomes(
            &report,
            &scorecard,
            &config.store_path,
            &config.snapshots_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::confirmation_outcome(
                &report,
                &scorecard,
                &config.store_path,
                &config.snapshots_path,
            ),
        )?),
    }
}

fn render_evaluate_confirmation_policy(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let generated = load_generated_signals_from_store(store)?;
    let confirmed_ids = load_confirmed_signal_ids_from_store(store)?;
    let snapshots = load_snapshots_jsonl(&config.snapshots_path)?;
    let comparison = compare_confirmation_quality(
        &generated,
        &confirmed_ids,
        &snapshots,
        &crate::ConfirmationOutcomeConfig {
            evaluation_horizon_seconds: config.horizon_seconds,
            delta_threshold: config.delta_threshold,
        },
    );
    let sweep = sweep_confirmation_policy(
        &generated,
        &snapshots,
        &config.confidence_thresholds,
        &config.horizons,
        config.delta_threshold,
    );
    let advisory = build_policy_advisory(&comparison);

    match config.format {
        OutputFormat::Text => Ok(text_renderer::evaluate_confirmation_policy(
            &comparison,
            &sweep,
            &advisory,
            &config.store_path,
            &config.snapshots_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::confirmation_policy(
                &comparison,
                &sweep,
                &advisory,
                &config.store_path,
                &config.snapshots_path,
            ),
        )?),
    }
}

fn render_walkforward_confirmation_policy(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let era_split = era_split_from_config(config, "walkforward-confirmation-policy")?;
    let report = run_confirmation_walkforward_from_store(
        store,
        &config.snapshots_path,
        &ConfirmationWalkForwardConfig {
            era_split,
            confidence_thresholds: config.confidence_thresholds.clone(),
            horizons: config.horizons.clone(),
            delta_threshold: config.delta_threshold,
            advisory_config: ConfirmationPolicyAdvisoryConfig::default(),
        },
    )?;

    match config.format {
        OutputFormat::Text => Ok(text_renderer::walkforward_confirmation_policy(
            &report,
            &config.store_path,
            &config.snapshots_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::walkforward_confirmation_policy(
                &report,
                &config.store_path,
                &config.snapshots_path,
            ),
        )?),
    }
}

fn render_propose_confirmation_policy(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let era_split = era_split_from_config(config, "propose-confirmation-policy")?;
    let walkforward = run_confirmation_walkforward_from_store(
        store,
        &config.snapshots_path,
        &ConfirmationWalkForwardConfig {
            era_split,
            confidence_thresholds: config.confidence_thresholds.clone(),
            horizons: config.horizons.clone(),
            delta_threshold: config.delta_threshold,
            advisory_config: ConfirmationPolicyAdvisoryConfig::default(),
        },
    )?;
    let source_analysis = format!(
        "propose-confirmation-policy --eras={} --window_size_seconds={} --confidence_thresholds={:?} --horizons={:?} --delta_threshold={}",
        config.eras,
        config.window_size_seconds.unwrap_or_default(),
        config.confidence_thresholds,
        config.horizons,
        config.delta_threshold
    );
    let proposal = propose_confirmation_policy(
        &walkforward,
        &source_analysis,
        &ConfirmationPolicyProposalConfig::default(),
    );

    if let Some(output_path) = &config.output_path {
        write_confirmation_policy_proposal(&proposal.policy, output_path)?;
    }

    match config.format {
        OutputFormat::Text => Ok(text_renderer::propose_confirmation_policy(
            &proposal,
            &config.output_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::propose_confirmation_policy(&proposal, &config.output_path),
        )?),
    }
}

fn render_materialize_confirmation_readiness(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let era_split = era_split_from_config(config, "materialize-confirmation-readiness")?;
    let generated = load_generated_signals_from_store(store)?;
    let walkforward = run_confirmation_walkforward_from_store(
        store,
        &config.snapshots_path,
        &ConfirmationWalkForwardConfig {
            era_split,
            confidence_thresholds: config.confidence_thresholds.clone(),
            horizons: config.horizons.clone(),
            delta_threshold: config.delta_threshold,
            advisory_config: ConfirmationPolicyAdvisoryConfig::default(),
        },
    )?;
    let source_analysis = format!(
        "materialize-confirmation-readiness --eras={} --window_size_seconds={} --confidence_thresholds={:?} --horizons={:?} --delta_threshold={}",
        config.eras,
        config.window_size_seconds.unwrap_or_default(),
        config.confidence_thresholds,
        config.horizons,
        config.delta_threshold
    );
    let proposal = propose_confirmation_policy(
        &walkforward,
        &source_analysis,
        &ConfirmationPolicyProposalConfig::default(),
    );
    let readiness = materialize_confirmation_readiness(
        &walkforward,
        &generated,
        Some(&proposal),
        &source_analysis,
        &ConfirmationReadinessConfig::default(),
    );
    let output_path = config
        .output_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_READINESS_PATH));
    write_confirmation_readiness_report(&readiness, &output_path)?;

    match config.format {
        OutputFormat::Text => Ok(text_renderer::materialize_confirmation_readiness(
            &readiness,
            &config.store_path,
            &config.snapshots_path,
            &output_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::materialize_confirmation_readiness(
                &readiness,
                &config.store_path,
                &config.snapshots_path,
                &output_path,
            ),
        )?),
    }
}

fn render_simulate_paper_fill(
    query_service: &QueryService<'_>,
    config: &Config,
    request: &PaperExecutionRequest,
) -> Result<String, RuntimeError> {
    let import_report = if let Some(trades_path) = &config.backend_trades_json {
        let backend = FixturePolymarketPaperBackend {
            trades_path: trades_path.clone(),
        };
        submit_paper_order_and_map_fill(&backend, request, &Default::default())?
    } else {
        let backend = CliPolymarketPaperBackend::default();
        submit_paper_order_and_map_fill(&backend, request, &Default::default())?
    };

    let observation_report = if let Some(fill_result) = &import_report.fill_result {
        Some(observe_fill(
            query_service,
            &fill_result.to_fill_observation_request(),
            FillObservationOptions {
                dry_run: config.dry_run,
            },
        )?)
    } else {
        None
    };

    match config.format {
        OutputFormat::Text => Ok(text_renderer::simulate_paper_fill(
            &import_report,
            observation_report.as_ref(),
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::simulate_paper_fill(
                &import_report,
                observation_report.as_ref(),
                &config.store_path,
            ),
        )?),
    }
}

fn render_run_paper_decisions(
    query_service: &QueryService<'_>,
    config: &Config,
) -> Result<String, RuntimeError> {
    let run_config = PaperDecisionRunConfig {
        usd_size: config.usd_size,
        backend_account: config.backend_account.clone(),
        backend_data_dir: config.backend_data_dir.clone(),
        risk: config.paper_risk.clone(),
        dry_run: config.dry_run,
    };
    let report = if let Some(trades_path) = &config.backend_trades_json {
        let backend = FixturePolymarketPaperBackend {
            trades_path: trades_path.clone(),
        };
        run_paper_decisions(query_service, &backend, &run_config)?
    } else {
        let backend = CliPolymarketPaperBackend::default();
        run_paper_decisions(query_service, &backend, &run_config)?
    };

    match config.format {
        OutputFormat::Text => Ok(text_renderer::run_paper_decisions(
            &report,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::run_paper_decisions(&report, &config.store_path),
        )?),
    }
}

// This command is the operational base that a future Dashboard / Control Room v1
// should summarize visually; it deliberately reuses the canonical runtime stages.
fn render_run_paper_pipeline(
    store: &JsonlEventStore,
    query_service: &QueryService<'_>,
    config: &Config,
    research_signals_path: &Option<PathBuf>,
) -> Result<String, RuntimeError> {
    let mut stages = Vec::new();
    let (signals_seen, signals_generated) =
        if let Some(research_signals_path) = research_signals_path {
            stages.push("ingest_research_signals".to_string());
            let ingest = ingest_research_signals_file(
                store,
                research_signals_path,
                ResearchSignalIngestOptions {
                    dry_run: config.dry_run,
                },
            )?;
            (ingest.rows_valid, ingest.events_written)
        } else {
            stages.push("load_existing_signals".to_string());
            (load_generated_signals_from_store(store)?.len(), 0)
        };

    stages.push("confirm_signals".to_string());
    let confirmation = confirmation_runner_from_config(store, config)?.run()?;

    stages.push("run_paper_decisions".to_string());
    let run_config = PaperDecisionRunConfig {
        usd_size: config.usd_size,
        backend_account: config.backend_account.clone(),
        backend_data_dir: config.backend_data_dir.clone(),
        risk: config.paper_risk.clone(),
        dry_run: config.dry_run,
    };
    let decisions = if let Some(trades_path) = &config.backend_trades_json {
        let backend = FixturePolymarketPaperBackend {
            trades_path: trades_path.clone(),
        };
        run_paper_decisions(query_service, &backend, &run_config)?
    } else {
        let backend = CliPolymarketPaperBackend::default();
        run_paper_decisions(query_service, &backend, &run_config)?
    };

    stages.push("project_paper_ledger".to_string());
    let ledger = project_paper_ledger(&query_service.all_events()?)?;

    let readiness_states_materialized = if config.materialize_readiness {
        stages.push("materialize_confirmation_readiness".to_string());
        let readiness = build_confirmation_readiness(store, config, "run-paper-pipeline")?;
        if !config.dry_run {
            let output_path = config
                .output_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(DEFAULT_READINESS_PATH));
            write_confirmation_readiness_report(&readiness, &output_path)?;
        }
        Some(readiness.states.len())
    } else {
        None
    };

    let report = PaperPipelineReport {
        dry_run: config.dry_run,
        stages,
        signals_seen,
        signals_generated,
        signals_confirmed: confirmation.persisted,
        execution_requests_sent: decisions.execution_requests_sent,
        fills_persisted: decisions.fills_persisted,
        blocked_by_risk: decisions.blocked_by_risk,
        open_positions: ledger.summary.open_positions,
        total_notional_spent: ledger.summary.total_notional_spent,
        total_notional_received: ledger.summary.total_notional_received,
        readiness_states_materialized,
    };
    if !config.dry_run {
        write_latest_pipeline_report(config, &report)?;
        write_latest_operational_summary(config)?;
        write_latest_dashboard(config)?;
    }

    match config.format {
        OutputFormat::Text => Ok(text_renderer::run_paper_pipeline(
            &report,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::run_paper_pipeline(&report, &config.store_path),
        )?),
    }
}

fn render_serve_dashboard(config: &Config) -> Result<String, RuntimeError> {
    let output_path = config
        .output_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(super::DEFAULT_DASHBOARD_PATH));
    let dashboard_config = DashboardConfig {
        store_path: config.store_path.clone(),
        readiness_path: PathBuf::from(DEFAULT_READINESS_PATH),
        policy_path: config.policy_file.clone(),
        pipeline_report_path: latest_pipeline_report_path(config),
        output_path: output_path.clone(),
    };
    let host = config
        .dashboard_host
        .clone()
        .unwrap_or_else(|| DEFAULT_DASHBOARD_HOST.to_string());
    let port = config.dashboard_port.unwrap_or(DEFAULT_DASHBOARD_PORT);
    if config.dashboard_host.is_some() || config.dashboard_port.is_some() {
        if config.dashboard_host.is_none() || config.dashboard_port.is_none() {
            return Err(RuntimeError::Usage(
                "serve-dashboard requires both --host and --port when serving over HTTP\n\n"
                    .to_string()
                    + &usage(),
            ));
        }
        serve_dashboard_http(dashboard_config, host, port)?;
        unreachable!("serve_dashboard_http only returns on error");
    }
    write_dashboard(&dashboard_config)?;

    match config.format {
        OutputFormat::Text => Ok(format!(
            "Dashboard\noutput_path: {}\ndata_sources: events, paper ledger projection, readiness artifact if present, policy file if provided, latest pipeline report if present, latest operational summary if present",
            output_path.display()
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&serde_json::json!({
            "kind": "serve_dashboard",
            "output_path": output_path.display().to_string(),
            "data_sources": [
                "events",
                "paper_ledger_projection",
                "readiness_artifact",
                "policy_file",
                "latest_pipeline_report",
                "latest_operational_summary"
            ]
        }))?),
    }
}

fn render_generate_operational_summary(config: &Config) -> Result<String, RuntimeError> {
    let output_path = config
        .output_path
        .clone()
        .unwrap_or_else(|| default_operational_summary_path(config.operational_summary_format));
    let summary_config = OperationalSummaryConfig {
        store_path: config.store_path.clone(),
        readiness_path: PathBuf::from(DEFAULT_READINESS_PATH),
        policy_path: config.policy_file.clone(),
        pipeline_report_path: latest_pipeline_report_path(config),
        output_path: output_path.clone(),
        format: config.operational_summary_format,
    };
    write_operational_summary(&summary_config)?;

    match config.format {
        OutputFormat::Text => Ok(format!(
            "Operational Summary\noutput_path: {}\nformat: {}\ndata_sources: latest pipeline report if present, canonical paper ledger projection, readiness artifact if present, policy file if provided",
            output_path.display(),
            config.operational_summary_format.as_str()
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&serde_json::json!({
            "kind": "generate_operational_summary",
            "output_path": output_path.display().to_string(),
            "format": config.operational_summary_format.as_str(),
            "data_sources": [
                "latest_pipeline_report",
                "paper_ledger_projection",
                "readiness_artifact",
                "policy_file"
            ]
        }))?),
    }
}

fn write_latest_pipeline_report(
    config: &Config,
    report: &PaperPipelineReport,
) -> Result<(), RuntimeError> {
    let path = latest_pipeline_report_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

fn latest_pipeline_report_path(config: &Config) -> PathBuf {
    config
        .store_path
        .parent()
        .map(|parent| parent.join("dashboard/latest_pipeline.json"))
        .unwrap_or_else(|| PathBuf::from("./var/dashboard/latest_pipeline.json"))
}

fn write_latest_operational_summary(config: &Config) -> Result<(), RuntimeError> {
    let summary_config = OperationalSummaryConfig {
        store_path: config.store_path.clone(),
        readiness_path: PathBuf::from(DEFAULT_READINESS_PATH),
        policy_path: config.policy_file.clone(),
        pipeline_report_path: latest_pipeline_report_path(config),
        output_path: latest_operational_summary_path(config),
        format: OperationalSummaryFormat::Json,
    };
    write_operational_summary(&summary_config)?;
    Ok(())
}

fn write_latest_dashboard(config: &Config) -> Result<(), RuntimeError> {
    let dashboard_config = DashboardConfig {
        store_path: config.store_path.clone(),
        readiness_path: PathBuf::from(DEFAULT_READINESS_PATH),
        policy_path: config.policy_file.clone(),
        pipeline_report_path: latest_pipeline_report_path(config),
        output_path: latest_dashboard_path(config),
    };
    write_dashboard(&dashboard_config)?;
    Ok(())
}

fn latest_operational_summary_path(config: &Config) -> PathBuf {
    config
        .store_path
        .parent()
        .map(|parent| parent.join("operations/latest_summary.json"))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OPERATIONAL_SUMMARY_JSON_PATH))
}

fn default_operational_summary_path(format: OperationalSummaryFormat) -> PathBuf {
    PathBuf::from(match format {
        OperationalSummaryFormat::Json => DEFAULT_OPERATIONAL_SUMMARY_JSON_PATH,
        OperationalSummaryFormat::Markdown => DEFAULT_OPERATIONAL_SUMMARY_MARKDOWN_PATH,
    })
}

fn latest_dashboard_path(config: &Config) -> PathBuf {
    config
        .store_path
        .parent()
        .map(|parent| parent.join("dashboard/control_room.html"))
        .unwrap_or_else(|| PathBuf::from(super::DEFAULT_DASHBOARD_PATH))
}

fn render_show_paper_ledger(
    query_service: &QueryService<'_>,
    config: &Config,
) -> Result<String, RuntimeError> {
    let events = query_service.all_events()?;
    let ledger = project_paper_ledger(&events)?;

    match config.format {
        OutputFormat::Text => Ok(text_renderer::show_paper_ledger(
            &ledger,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::show_paper_ledger(&ledger, &config.store_path),
        )?),
    }
}

fn build_confirmation_readiness(
    store: &JsonlEventStore,
    config: &Config,
    command_name: &str,
) -> Result<crate::ConfirmationReadinessReport, RuntimeError> {
    let era_split = era_split_from_config(config, command_name)?;
    let generated = load_generated_signals_from_store(store)?;
    let walkforward = run_confirmation_walkforward_from_store(
        store,
        &config.snapshots_path,
        &ConfirmationWalkForwardConfig {
            era_split,
            confidence_thresholds: config.confidence_thresholds.clone(),
            horizons: config.horizons.clone(),
            delta_threshold: config.delta_threshold,
            advisory_config: ConfirmationPolicyAdvisoryConfig::default(),
        },
    )?;
    let source_analysis = format!(
        "{command_name} --eras={} --window_size_seconds={} --confidence_thresholds={:?} --horizons={:?} --delta_threshold={}",
        config.eras,
        config.window_size_seconds.unwrap_or_default(),
        config.confidence_thresholds,
        config.horizons,
        config.delta_threshold
    );
    let proposal = propose_confirmation_policy(
        &walkforward,
        &source_analysis,
        &ConfirmationPolicyProposalConfig::default(),
    );
    Ok(materialize_confirmation_readiness(
        &walkforward,
        &generated,
        Some(&proposal),
        &source_analysis,
        &ConfirmationReadinessConfig::default(),
    ))
}

fn era_split_from_config(
    config: &Config,
    command_name: &str,
) -> Result<ConfirmationEraSplit, RuntimeError> {
    if config.window_size_seconds.is_some() && config.eras != 3 {
        return Err(RuntimeError::Usage(format!(
            "use either --eras or --window-size for {command_name}\n\n{}",
            usage()
        )));
    }
    if config.eras == 0 {
        return Err(RuntimeError::Usage(format!(
            "--eras must be greater than zero\n\n{}",
            usage()
        )));
    }
    if config.window_size_seconds.is_some_and(|value| value <= 0) {
        return Err(RuntimeError::Usage(format!(
            "--window-size must be greater than zero\n\n{}",
            usage()
        )));
    }

    Ok(
        if let Some(window_size_seconds) = config.window_size_seconds {
            ConfirmationEraSplit::WindowSizeSeconds(window_size_seconds)
        } else {
            ConfirmationEraSplit::EraCount(config.eras)
        },
    )
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
        OutputFormat::Text => Ok(text_renderer::batch_run(&report)),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_renderer::batch_run(
            &report,
        ))?),
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
        OutputFormat::Text => Ok(text_renderer::materialize_decisions(
            &report,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::decision_materialization(&report, &config.store_path),
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
        OutputFormat::Text => Ok(text_renderer::materialize_orders(
            &report,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::order_materialization(&report, &config.store_path),
        )?),
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
        OutputFormat::Text => Ok(text_renderer::submit_orders(&report, &config.store_path)),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::order_submission(&report, &config.store_path),
        )?),
    }
}

fn render_observe_fill(
    query_service: &QueryService<'_>,
    config: &Config,
    request: &FillObservationRequest,
) -> Result<String, RuntimeError> {
    let report = observe_fill(
        query_service,
        request,
        FillObservationOptions {
            dry_run: config.dry_run,
        },
    )?;

    match config.format {
        OutputFormat::Text => Ok(text_renderer::observe_fill(&report, &config.store_path)),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::fill_observation(&report, &config.store_path),
        )?),
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
        OutputFormat::Text => Ok(text_renderer::policy_only_signal(
            signal_id,
            &policy,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::policy_only_signal(signal_id, &policy, &config.store_path),
        )?),
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
        OutputFormat::Text => Ok(text_renderer::policy_only_decision(
            decision_id,
            &policy,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::policy_only_decision(decision_id, &policy, &config.store_path),
        )?),
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
        OutputFormat::Text => Ok(text_renderer::policy_only_order(
            order_id,
            &policy,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::policy_only_order(order_id, &policy, &config.store_path),
        )?),
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
        OutputFormat::Text => Ok(text_renderer::research_signal_ingest(
            &report,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(
            &json_renderer::research_signal_ingest(&report, &config.store_path),
        )?),
    }
}

fn render_summary(store: &JsonlEventStore, config: &Config) -> Result<String, RuntimeError> {
    let summary = summary_from_store(store)?;

    match config.format {
        OutputFormat::Text => Ok(text_renderer::summary(&summary, &config.store_path)),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_renderer::summary(
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
        OutputFormat::Text => Ok(text_renderer::signal(
            signal_id,
            projection.as_ref(),
            readiness.as_ref(),
            governance.as_ref(),
            policy.as_ref(),
            &timeline,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_renderer::signal(
            signal_id,
            projection.as_ref(),
            readiness.as_ref(),
            governance.as_ref(),
            policy.as_ref(),
            &timeline,
            &config.store_path,
        ))?),
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
        OutputFormat::Text => Ok(text_renderer::decision(
            decision_id,
            projection.as_ref(),
            readiness.as_ref(),
            lineage.as_ref(),
            governance.as_ref(),
            policy.as_ref(),
            boundary.as_ref(),
            &timeline,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_renderer::decision(
            decision_id,
            projection.as_ref(),
            readiness.as_ref(),
            lineage.as_ref(),
            governance.as_ref(),
            policy.as_ref(),
            boundary.as_ref(),
            &timeline,
            &config.store_path,
        ))?),
    }
}

fn render_order(
    query_service: &QueryService<'_>,
    config: &Config,
    order_id: &str,
) -> Result<String, RuntimeError> {
    let lifecycle = query_service.order_lifecycle(order_id)?;
    let execution = query_service.order_execution_summary(order_id)?;
    let policy = query_service.order_promotion_policy(order_id)?;
    let submission_policy = query_service.order_submission_policy(order_id)?;
    let all_events = query_service.all_events()?;
    let related_events = events_for_order(&all_events, order_id);

    if lifecycle.is_none()
        && execution.is_none()
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
        OutputFormat::Text => Ok(text_renderer::order(
            order_id,
            lifecycle.as_ref(),
            execution.as_ref(),
            policy.as_ref(),
            submission_policy.as_ref(),
            &related_events,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_renderer::order(
            order_id,
            lifecycle.as_ref(),
            execution.as_ref(),
            policy.as_ref(),
            submission_policy.as_ref(),
            &related_events,
            &config.store_path,
        ))?),
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
        OutputFormat::Text => Ok(text_renderer::fill(
            fill_id,
            readiness.as_ref(),
            boundary.as_ref(),
            &matching_events,
            &config.store_path,
        )),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(&json_renderer::fill(
            fill_id,
            readiness.as_ref(),
            boundary.as_ref(),
            &matching_events,
            &config.store_path,
        ))?),
    }
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
            event
                .payload
                .get("fill_id")
                .and_then(serde_json::Value::as_str)
                == Some(fill_id)
                && event.event_type.as_str() == "fill.received"
        })
        .cloned()
        .collect()
}

fn build_policy_advisory(
    comparison: &ConfirmationComparisonReport,
) -> Vec<ConfirmationPolicyAdvisory> {
    let metrics = comparison
        .by_signal_name
        .iter()
        .map(|row| AdvisorySignalFamilyMetrics {
            signal_name: row.key.clone(),
            sample_count: row.confirmed_count,
            favorable_rate: row.favorable_rate_confirmed,
            unfavorable_rate: row.unfavorable_rate_confirmed,
        })
        .collect::<Vec<_>>();
    advise_signal_families(&metrics, &ConfirmationPolicyAdvisoryConfig::default())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::execute;
    use crate::runtime::{Command, Config, OutputFormat};

    #[test]
    fn dispatches_summary_command() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("twoexcamim-dispatch-{nanos}.jsonl"));
        let output = execute(Config {
            command: Command::Summary,
            store_path: path.clone(),
            snapshots_path: std::env::temp_dir()
                .join(format!("twoexcamim-snapshots-{nanos}.jsonl")),
            format: OutputFormat::Text,
            dry_run: false,
            horizon_seconds: 3600,
            delta_threshold: 0.02,
            horizons: vec![3600],
            confidence_thresholds: vec![0.6],
            eras: 3,
            window_size_seconds: None,
            policy_file: None,
            output_path: None,
            dashboard_host: None,
            dashboard_port: None,
            operational_summary_format: crate::OperationalSummaryFormat::Json,
            backend_trades_json: None,
            materialize_readiness: false,
            usd_size: 100.0,
            backend_account: "default".into(),
            backend_data_dir: PathBuf::from(crate::DEFAULT_POLYMARKET_PAPER_DATA_DIR),
            paper_risk: Default::default(),
        })
        .unwrap();

        assert!(output.contains("Runtime Summary"));
        let _ = std::fs::remove_file(path);
    }
}
