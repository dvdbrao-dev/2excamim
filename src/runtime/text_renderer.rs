use std::fmt::Debug;
use std::path::PathBuf;

use crate::ConfirmationPolicyProposal;
use crate::{
    agents::{ConfirmationComparisonReport, ConfirmationPolicyAdvisory, PolicySweepSummary},
    batch_runner::BatchRunReport,
    handoff::ResearchSignalIngestReport,
    materialization::{
        DecisionMaterializationReport, FillObservationReport, OrderMaterializationReport,
        OrderSubmissionReport,
    },
    observability::ObservabilitySummary,
    projections::{DecisionProjection, SignalProjection},
    queries::{
        DecisionGovernanceReport, DecisionLineageReport, DecisionPromotionReport,
        DecisionReadiness, ExecutionBoundaryReport, FillReadiness, GovernanceRef,
        OrderExecutionSummary, OrderLifecycleReport, OrderPromotionReport,
        OrderSubmissionPolicyReport, PromotionNextStep, PromotionPolicyStatus,
        SignalGovernanceReport, SignalPromotionReport, SignalReadiness,
    },
    store::StoredEvent,
};

pub(crate) fn confirm_signals(
    report: &crate::ConfirmationRunReport,
    scorecard: &crate::ConfirmationScorecard,
    store_path: &PathBuf,
) -> String {
    [
        "Signal Confirmation".to_string(),
        format!("store_path: {}", store_path.display()),
        format!(
            "total_signals_processed: {}",
            report.total_signals_processed
        ),
        format!("accepted: {}", report.accepted),
        format!("rejected: {}", report.rejected),
        format!(
            "rejected_low_confidence: {}",
            report.rejected_low_confidence
        ),
        format!("rejected_stale: {}", report.rejected_stale),
        format!("skipped_frozen: {}", report.skipped_frozen),
        format!(
            "skipped_already_confirmed: {}",
            report.skipped_already_confirmed
        ),
        format!("policy_overrides_used: {}", report.policy_overrides_used),
        format!("persisted: {}", report.persisted),
        format!("duplicates: {}", report.duplicates),
        format!("acceptance_rate: {:.4}", scorecard.acceptance_rate),
        format!("rejection_rate: {:.4}", scorecard.rejection_rate),
        format!("low_confidence_rate: {:.4}", scorecard.low_confidence_rate),
        format!("stale_rate: {:.4}", scorecard.stale_rate),
        format!("skipped_frozen_rate: {:.4}", scorecard.skipped_frozen_rate),
    ]
    .join("\n")
}

pub(crate) fn measure_confirmation_outcomes(
    report: &crate::ConfirmationOutcomeRunReport,
    scorecard: &crate::ConfirmationOutcomeScorecard,
    store_path: &PathBuf,
    snapshots_path: &PathBuf,
) -> String {
    [
        "Confirmation Outcomes".to_string(),
        format!("store_path: {}", store_path.display()),
        format!("snapshots_path: {}", snapshots_path.display()),
        format!(
            "total_confirmed_signals_processed: {}",
            report.total_confirmed_signals_processed
        ),
        format!("favorable: {}", report.favorable),
        format!("unfavorable: {}", report.unfavorable),
        format!("neutral: {}", report.neutral),
        format!("insufficient_data: {}", report.insufficient_data),
        format!("favorable_rate: {:.4}", scorecard.favorable_rate),
        format!("unfavorable_rate: {:.4}", scorecard.unfavorable_rate),
        format!("neutral_rate: {:.4}", scorecard.neutral_rate),
        format!(
            "insufficient_data_rate: {:.4}",
            scorecard.insufficient_data_rate
        ),
        format!(
            "average_delta_probability: {}",
            display_option_number(scorecard.average_delta_probability)
        ),
    ]
    .join("\n")
}

pub(crate) fn evaluate_confirmation_policy(
    comparison: &ConfirmationComparisonReport,
    sweep: &PolicySweepSummary,
    advisory: &[ConfirmationPolicyAdvisory],
    store_path: &PathBuf,
    snapshots_path: &PathBuf,
) -> String {
    let mut lines = vec![
        "Confirmation Policy Evaluation".to_string(),
        format!("store_path: {}", store_path.display()),
        format!("snapshots_path: {}", snapshots_path.display()),
        format!(
            "favorable_rate_confirmed: {:.4}",
            comparison.favorable_rate_confirmed
        ),
        format!(
            "favorable_rate_all_generated: {:.4}",
            comparison.favorable_rate_all_generated
        ),
        format!(
            "uplift_favorable_rate: {:.4}",
            comparison.uplift_favorable_rate
        ),
        format!(
            "unfavorable_rate_confirmed: {:.4}",
            comparison.unfavorable_rate_confirmed
        ),
        format!(
            "unfavorable_rate_all_generated: {:.4}",
            comparison.unfavorable_rate_all_generated
        ),
        format!(
            "average_delta_confirmed: {}",
            display_option_number(comparison.average_delta_confirmed)
        ),
        format!(
            "average_delta_all_generated: {}",
            display_option_number(comparison.average_delta_all_generated)
        ),
        "advisory:".to_string(),
    ];
    if advisory.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in advisory {
            lines.push(format!(
                "- signal_name={} classification={:?} sample_count={} favorable_rate={:.4} unfavorable_rate={:.4}",
                item.signal_name, item.classification, item.sample_count, item.favorable_rate, item.unfavorable_rate
            ));
        }
    }
    lines.push("sweep:".to_string());
    if sweep.rows.is_empty() {
        lines.push("- none".to_string());
    } else {
        for row in &sweep.rows {
            lines.push(format!(
                "- confidence_threshold={:.3} horizon_seconds={} confirmed_signals={} acceptance_rate={:.4} favorable_rate={:.4} unfavorable_rate={:.4} uplift_vs_baseline={}",
                row.confidence_threshold,
                row.horizon_seconds,
                row.confirmed_signals,
                row.acceptance_rate,
                row.favorable_rate,
                row.unfavorable_rate,
                display_option_number(row.uplift_vs_baseline)
            ));
        }
    }
    lines.join("\n")
}

pub(crate) fn walkforward_confirmation_policy(
    report: &crate::ConfirmationWalkForwardReport,
    store_path: &PathBuf,
    snapshots_path: &PathBuf,
) -> String {
    let mut lines = vec![
        "Walk-Forward Confirmation Policy".to_string(),
        format!("store_path: {}", store_path.display()),
        format!("snapshots_path: {}", snapshots_path.display()),
        format!("eras: {}", report.eras.len()),
        format!("steps: {}", report.steps.len()),
        format!(
            "average_validation_favorable_rate: {:.4}",
            report.summary.average_validation_favorable_rate
        ),
        format!(
            "average_validation_uplift: {:.4}",
            report.summary.average_validation_uplift
        ),
        "threshold_consistency:".to_string(),
    ];
    if report.summary.chosen_thresholds.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in &report.summary.chosen_thresholds {
            lines.push(format!(
                "- threshold={:.3} count={} rate={:.4}",
                item.value, item.count, item.rate
            ));
        }
    }
    lines.push("horizon_consistency:".to_string());
    if report.summary.chosen_horizons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in &report.summary.chosen_horizons {
            lines.push(format!(
                "- horizon_seconds={} count={} rate={:.4}",
                item.value, item.count, item.rate
            ));
        }
    }
    lines.push("steps:".to_string());
    if report.steps.is_empty() {
        lines.push("- none".to_string());
    } else {
        for step in &report.steps {
            lines.push(format!(
                "- train_era_id={} validation_era_id={} chosen_confidence_threshold={:.3} chosen_horizon_seconds={} validation_favorable_rate={:.4} validation_uplift={} confirmed_sample_count_validation={}",
                step.train_era_id,
                step.validation_era_id,
                step.chosen_confidence_threshold,
                step.chosen_horizon_seconds,
                step.validation_metrics.favorable_rate,
                display_option_number(step.validation_metrics.uplift_vs_baseline),
                step.confirmed_sample_count_validation
            ));
        }
    }
    lines.push("advisory_stability:".to_string());
    if report.summary.advisory_stability.is_empty() {
        lines.push("- none".to_string());
    } else {
        for item in &report.summary.advisory_stability {
            lines.push(format!(
                "- signal_name={} promote_candidate_count={} review_count={} freeze_candidate_count={}",
                item.signal_name,
                item.promote_candidate_count,
                item.review_count,
                item.freeze_candidate_count
            ));
        }
    }
    lines.join("\n")
}

pub(crate) fn propose_confirmation_policy(
    proposal: &ConfirmationPolicyProposal,
    output_path: &Option<PathBuf>,
) -> String {
    let mut lines = vec![
        "Proposed Confirmation Policy".to_string(),
        format!(
            "generated_at: {}",
            proposal
                .policy
                .metadata
                .as_ref()
                .map(|metadata| metadata.generated_at.to_rfc3339())
                .unwrap_or_else(|| "none".into())
        ),
        format!(
            "eras_analyzed: {}",
            proposal
                .policy
                .metadata
                .as_ref()
                .map(|metadata| metadata.eras_analyzed)
                .unwrap_or(0)
        ),
        format!("promoted_rules: {}", proposal.summary.promoted_rules),
        format!("review_rules: {}", proposal.summary.review_rules),
        format!("frozen_rules: {}", proposal.summary.frozen_rules),
        format!(
            "output_path: {}",
            output_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".into())
        ),
        "rules:".to_string(),
    ];
    if proposal.policy.rules.is_empty() {
        lines.push("- none".to_string());
    } else {
        for rule in &proposal.policy.rules {
            lines.push(format!(
                "- signal_name={} status={:?} confidence_threshold={} horizon_seconds={}",
                rule.signal_name,
                rule.status,
                display_option_number(rule.confidence_threshold),
                rule.horizon_seconds
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".into())
            ));
        }
    }
    lines.join("\n")
}

pub(crate) fn batch_run(report: &BatchRunReport) -> String {
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
    lines.join("\n")
}

pub(crate) fn materialize_decisions(
    report: &DecisionMaterializationReport,
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        "Decision Materialization".to_string(),
        format!("store_path: {}", store_path.display()),
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

    lines.join("\n")
}

pub(crate) fn materialize_orders(
    report: &OrderMaterializationReport,
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        "Order Materialization".to_string(),
        format!("store_path: {}", store_path.display()),
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

    lines.join("\n")
}

pub(crate) fn submit_orders(report: &OrderSubmissionReport, store_path: &PathBuf) -> String {
    let mut lines = vec![
        "Order Submission".to_string(),
        format!("store_path: {}", store_path.display()),
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

    lines.join("\n")
}

pub(crate) fn observe_fill(report: &FillObservationReport, store_path: &PathBuf) -> String {
    [
        "Execution Observation".to_string(),
        format!("store_path: {}", store_path.display()),
        format!("dry_run: {}", report.dry_run),
        format!("batch_trace_id: {}", report.batch_trace_id),
        format!("fill_id: {}", report.fill_id),
        format!("order_id: {}", report.order_id),
        format!(
            "order_status_before: {}",
            display_option(report.order_status_before.as_deref())
        ),
        format!(
            "resolved_decision_id: {}",
            display_option(report.resolved_decision_id.as_deref())
        ),
        format!("resolved_instrument: {}", report.resolved_instrument),
        format!("resolved_venue: {}", report.resolved_venue),
        format!("disposition: {:?}", report.disposition),
        format!("persisted: {}", report.persisted),
        format!("duplicate: {}", report.duplicate),
        "reasons:".to_string(),
    ]
    .into_iter()
    .chain(if report.reasons.is_empty() {
        vec!["- none".to_string()]
    } else {
        report
            .reasons
            .iter()
            .map(|reason| format!("- {reason}"))
            .collect()
    })
    .chain(
        ["notes:".to_string()]
            .into_iter()
            .chain(if report.notes.is_empty() {
                vec!["- none".to_string()]
            } else {
                report
                    .notes
                    .iter()
                    .map(|note| format!("- {note}"))
                    .collect()
            }),
    )
    .collect::<Vec<_>>()
    .join("\n")
}

pub(crate) fn policy_only_signal(
    signal_id: &str,
    policy: &SignalPromotionReport,
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Signal Policy {}", signal_id),
        format!("store_path: {}", store_path.display()),
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
    lines.join("\n")
}

pub(crate) fn policy_only_decision(
    decision_id: &str,
    policy: &DecisionPromotionReport,
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Decision Policy {}", decision_id),
        format!("store_path: {}", store_path.display()),
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
    lines.join("\n")
}

pub(crate) fn policy_only_order(
    order_id: &str,
    policy: &OrderPromotionReport,
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Order Policy {}", order_id),
        format!("store_path: {}", store_path.display()),
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
    lines.join("\n")
}

pub(crate) fn research_signal_ingest(
    report: &ResearchSignalIngestReport,
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        "Research Signal Ingest".to_string(),
        format!("store_path: {}", store_path.display()),
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

    lines.join("\n")
}

pub(crate) fn summary(summary: &ObservabilitySummary, store_path: &PathBuf) -> String {
    let mut lines = vec![
        "Runtime Summary".to_string(),
        format!("store_path: {}", store_path.display()),
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

    for (event_type, count) in &summary.event_counts_by_type {
        lines.push(format!("- {event_type}: {count}"));
    }

    lines.join("\n")
}

pub(crate) fn signal(
    signal_id: &str,
    projection: Option<&SignalProjection>,
    readiness: Option<&SignalReadiness>,
    governance: Option<&SignalGovernanceReport>,
    policy: Option<&SignalPromotionReport>,
    timeline: &[StoredEvent],
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Signal {}", signal_id),
        format!("store_path: {}", store_path.display()),
    ];
    lines.extend(render_signal_projection_text(projection));
    lines.extend(render_signal_readiness_text(readiness));
    lines.extend(render_signal_governance_text(governance));
    lines.extend(render_signal_policy_text(policy));
    lines.extend(render_timeline_text("Timeline", timeline));
    lines.join("\n")
}

pub(crate) fn decision(
    decision_id: &str,
    projection: Option<&DecisionProjection>,
    readiness: Option<&DecisionReadiness>,
    lineage: Option<&DecisionLineageReport>,
    governance: Option<&DecisionGovernanceReport>,
    policy: Option<&DecisionPromotionReport>,
    boundary: Option<&ExecutionBoundaryReport>,
    timeline: &[StoredEvent],
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Decision {}", decision_id),
        format!("store_path: {}", store_path.display()),
    ];
    lines.extend(render_decision_projection_text(projection));
    lines.extend(render_decision_readiness_text(readiness));
    lines.extend(render_decision_lineage_text(lineage));
    lines.extend(render_decision_governance_text(governance));
    lines.extend(render_decision_policy_text(policy));
    lines.extend(render_execution_boundary_text(
        "Execution Boundary",
        boundary,
    ));
    lines.extend(render_timeline_text("Timeline", timeline));
    lines.join("\n")
}

pub(crate) fn order(
    order_id: &str,
    lifecycle: Option<&OrderLifecycleReport>,
    execution: Option<&OrderExecutionSummary>,
    policy: Option<&OrderPromotionReport>,
    submission_policy: Option<&OrderSubmissionPolicyReport>,
    related_events: &[StoredEvent],
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Order {}", order_id),
        format!("store_path: {}", store_path.display()),
    ];
    lines.extend(render_order_lifecycle_text(lifecycle));
    lines.extend(render_order_execution_text(execution));
    lines.extend(render_order_policy_text(policy));
    lines.extend(render_order_submission_policy_text(submission_policy));
    lines.extend(render_timeline_text("Related Events", related_events));
    lines.join("\n")
}

pub(crate) fn fill(
    fill_id: &str,
    readiness: Option<&FillReadiness>,
    boundary: Option<&ExecutionBoundaryReport>,
    matching_events: &[StoredEvent],
    store_path: &PathBuf,
) -> String {
    let mut lines = vec![
        format!("Fill {}", fill_id),
        format!("store_path: {}", store_path.display()),
    ];
    lines.extend(render_fill_readiness_text(readiness));
    lines.extend(render_execution_boundary_text(
        "Execution Boundary",
        boundary,
    ));
    lines.extend(render_timeline_text(
        "Matching Fill Events",
        matching_events,
    ));
    lines.join("\n")
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

fn render_order_execution_text(report: Option<&OrderExecutionSummary>) -> Vec<String> {
    let Some(report) = report else {
        return vec!["Execution Summary: none".to_string()];
    };
    let mut lines = vec![
        "Execution Summary".to_string(),
        format!("status: {}", report.execution_status.as_str()),
        format!(
            "decision_id: {}",
            display_option(report.decision_id.as_deref())
        ),
        format!(
            "ordered_quantity: {}",
            display_option_number(report.ordered_quantity)
        ),
        format!("filled_quantity: {}", report.filled_quantity),
        format!(
            "remaining_quantity: {}",
            display_option_number(report.remaining_quantity)
        ),
        format!(
            "average_fill_price: {}",
            display_option_number(report.average_fill_price)
        ),
        format!("fill_count: {}", report.fill_count),
        "reasons:".to_string(),
    ];
    if report.reasons.is_empty() {
        lines.push("- none".to_string());
    } else {
        for reason in &report.reasons {
            lines.push(format!("- {reason}"));
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

#[cfg(test)]
mod tests {
    use super::confirm_signals;
    use crate::{
        events::{EventEnvelope, Linkage, Provenance, SignalGenerated, SignalSide, SourceKind},
        store::StoredEvent,
        ConfirmationDisposition, ConfirmationRunItem, ConfirmationRunReport, ConfirmationScorecard,
    };
    use chrono::{TimeZone, Utc};

    fn stored_event<T: serde::Serialize>(event: EventEnvelope<T>) -> StoredEvent {
        StoredEvent::try_from(event).unwrap()
    }

    #[test]
    fn confirm_signals_text_is_stable() {
        let report = ConfirmationRunReport {
            total_signals_processed: 1,
            accepted: 1,
            rejected: 0,
            rejected_low_confidence: 0,
            rejected_stale: 0,
            skipped_frozen: 0,
            skipped_already_confirmed: 0,
            policy_overrides_used: 0,
            persisted: 1,
            duplicates: 0,
            items: vec![ConfirmationRunItem {
                signal_id: "sig-1".into(),
                disposition: ConfirmationDisposition::Accepted,
                persisted: true,
                market_id: "market-1".into(),
                direction: market_domain::MarketSignalDirection::Yes,
                source: market_domain::MarketSource::Synthetic,
                signal_name: "odds_jump".into(),
                applied_confidence_threshold: None,
                policy_status: None,
            }],
            emitted_events: vec![stored_event(
                EventEnvelope::new_signal_generated(
                    "signal-engine",
                    Some("market-1".into()),
                    Linkage::default(),
                    Provenance {
                        source_kind: SourceKind::Derived,
                        source_ref: None,
                        producer_run_id: None,
                        actor: None,
                        trace_id: None,
                        notes: None,
                    },
                    SignalGenerated {
                        signal_id: "sig-1".into(),
                        hypothesis_id: None,
                        instrument: "market-1".into(),
                        timeframe: "odds_jump".into(),
                        side: SignalSide::Long,
                        strength: 0.8,
                        rationale: None,
                    },
                )
                .unwrap(),
            )],
        };
        let scorecard = ConfirmationScorecard::from_report(&report);
        let rendered = confirm_signals(
            &report,
            &scorecard,
            &std::path::PathBuf::from("var/events.jsonl"),
        );
        assert!(rendered.contains("Signal Confirmation"));
        assert!(rendered.contains("acceptance_rate: 1.0000"));
        let _ = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();
    }
}
