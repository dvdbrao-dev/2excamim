use std::path::PathBuf;

use crate::{
    agents::{
        advise_signal_families, compare_confirmation_quality, load_confirmed_signal_ids_from_store,
        load_generated_signals_from_store, load_snapshots_jsonl, propose_confirmation_policy,
        run_confirmation_walkforward_from_store, sweep_confirmation_policy,
        write_confirmation_policy_proposal, AdvisorySignalFamilyMetrics,
        ConfirmationComparisonReport, ConfirmationEraSplit, ConfirmationPolicy,
        ConfirmationPolicyAdvisory, ConfirmationPolicyAdvisoryConfig,
        ConfirmationPolicyProposalConfig, ConfirmationRunner, ConfirmationScorecard,
        ConfirmationWalkForwardConfig,
    },
    batch_runner::{run_batch, BatchRunOptions},
    handoff::{ingest_research_signals_file, ResearchSignalIngestOptions},
    materialization::{
        materialize_decisions, materialize_orders, observe_fill, submit_orders,
        DecisionMaterializationOptions, FillObservationOptions, FillObservationRequest,
        OrderMaterializationOptions, OrderSubmissionOptions,
    },
    observability::summary_from_store,
    queries::QueryService,
    store::{JsonlEventStore, StoredEvent},
};

use super::{
    cli_parser::usage, json_renderer, text_renderer, Command, Config, OutputFormat, RuntimeError,
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
                | Command::RunBatch { .. }
        )
    {
        return Err(RuntimeError::Usage(
            "--dry-run is only supported for ingest research-signals, materialize decisions, materialize orders, submit orders, observe fill and run batch"
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
        Command::ConfirmSignals => render_confirm_signals(&store, &config),
        Command::MeasureConfirmationOutcomes => {
            render_measure_confirmation_outcomes(&store, &config)
        }
        Command::EvaluateConfirmationPolicy => render_evaluate_confirmation_policy(&store, &config),
        Command::WalkForwardConfirmationPolicy => {
            render_walkforward_confirmation_policy(&store, &config)
        }
        Command::ProposeConfirmationPolicy => render_propose_confirmation_policy(&store, &config),
        Command::MaterializeDecisions => render_materialize_decisions(&query_service, &config),
        Command::MaterializeOrders => render_materialize_orders(&query_service, &config),
        Command::SubmitOrders => render_submit_orders(&query_service, &config),
        Command::RunBatch {
            ref research_signals_path,
        } => render_batch_run(&store, &config, research_signals_path),
    }
}

fn render_confirm_signals(
    store: &JsonlEventStore,
    config: &Config,
) -> Result<String, RuntimeError> {
    let runner = if let Some(policy_file) = &config.policy_file {
        ConfirmationRunner::new(store, crate::ConfirmationAgent::new(Default::default()))
            .with_policy(ConfirmationPolicy::from_file(policy_file)?)
    } else {
        ConfirmationRunner::new(store, crate::ConfirmationAgent::new(Default::default()))
    };
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
    if config.window_size_seconds.is_some() && config.eras != 3 {
        return Err(RuntimeError::Usage(format!(
            "use either --eras or --window-size for walkforward-confirmation-policy\n\n{}",
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

    let era_split = if let Some(window_size_seconds) = config.window_size_seconds {
        ConfirmationEraSplit::WindowSizeSeconds(window_size_seconds)
    } else {
        ConfirmationEraSplit::EraCount(config.eras)
    };
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
    if config.window_size_seconds.is_some() && config.eras != 3 {
        return Err(RuntimeError::Usage(format!(
            "use either --eras or --window-size for propose-confirmation-policy\n\n{}",
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

    let era_split = if let Some(window_size_seconds) = config.window_size_seconds {
        ConfirmationEraSplit::WindowSizeSeconds(window_size_seconds)
    } else {
        ConfirmationEraSplit::EraCount(config.eras)
    };
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
        })
        .unwrap();

        assert!(output.contains("Runtime Summary"));
        let _ = std::fs::remove_file(path);
    }
}
