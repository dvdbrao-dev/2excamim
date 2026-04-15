use std::fmt::Debug;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ConfirmationPolicyProposal;
use crate::{
    agents::{
        ConfirmationComparisonReport, ConfirmationPolicyAdvisory, ConfirmationWalkForwardReport,
        PolicySweepSummary,
    },
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
        GovernanceRefType, OrderExecutionSummary, OrderLifecycleReport, OrderPromotionReport,
        OrderSubmissionPolicyReport, PromotionNextStep, PromotionPolicyStatus,
        SignalGovernanceReport, SignalPromotionReport, SignalReadiness,
    },
    store::StoredEvent,
};

pub(crate) fn confirmation_run(
    report: &crate::ConfirmationRunReport,
    scorecard: &crate::ConfirmationScorecard,
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "confirm_signals",
        "store_path": store_path.display().to_string(),
        "total_signals_processed": report.total_signals_processed,
        "accepted": report.accepted,
        "rejected": report.rejected,
        "rejected_low_confidence": report.rejected_low_confidence,
        "rejected_stale": report.rejected_stale,
        "skipped_frozen": report.skipped_frozen,
        "skipped_already_confirmed": report.skipped_already_confirmed,
        "policy_overrides_used": report.policy_overrides_used,
        "persisted": report.persisted,
        "duplicates": report.duplicates,
        "scorecard": {
            "processed_count": scorecard.processed_count,
            "acceptance_rate": scorecard.acceptance_rate,
            "rejection_rate": scorecard.rejection_rate,
            "low_confidence_rate": scorecard.low_confidence_rate,
            "stale_rate": scorecard.stale_rate,
            "skipped_frozen_rate": scorecard.skipped_frozen_rate,
            "skipped_already_confirmed_rate": scorecard.skipped_already_confirmed_rate,
            "by_direction": scorecard.by_direction.iter().map(|item| json!({
                "direction": format!("{:?}", item.direction),
                "total": item.total,
                "accepted": item.accepted,
                "rejected_low_confidence": item.rejected_low_confidence,
                "rejected_stale": item.rejected_stale,
                "skipped_frozen": item.skipped_frozen,
                "skipped_already_confirmed": item.skipped_already_confirmed,
            })).collect::<Vec<_>>(),
            "by_source": scorecard.by_source.iter().map(|(source, count)| json!({
                "source": format!("{:?}", source),
                "count": count,
            })).collect::<Vec<_>>(),
            "by_signal_name": scorecard.by_signal_name.iter().map(|(signal_name, count)| json!({
                "signal_name": signal_name,
                "count": count,
            })).collect::<Vec<_>>(),
        },
        "items": report.items.iter().map(|item| json!({
            "signal_id": item.signal_id,
            "disposition": match item.disposition {
                crate::ConfirmationDisposition::Accepted => "accepted",
                crate::ConfirmationDisposition::RejectedLowConfidence => "rejected_low_confidence",
                crate::ConfirmationDisposition::RejectedStale => "rejected_stale",
                crate::ConfirmationDisposition::SkippedFrozen => "skipped_frozen",
                crate::ConfirmationDisposition::SkippedAlreadyConfirmed => "skipped_already_confirmed",
            },
            "persisted": item.persisted,
            "market_id": item.market_id,
            "direction": format!("{:?}", item.direction),
            "source": format!("{:?}", item.source),
            "signal_name": item.signal_name,
            "applied_confidence_threshold": item.applied_confidence_threshold,
            "policy_status": item.policy_status.map(|status| match status {
                crate::SignalPolicyStatus::Promoted => "promoted",
                crate::SignalPolicyStatus::Review => "review",
                crate::SignalPolicyStatus::Frozen => "frozen",
            }),
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn confirmation_outcome(
    report: &crate::ConfirmationOutcomeRunReport,
    scorecard: &crate::ConfirmationOutcomeScorecard,
    store_path: &PathBuf,
    snapshots_path: &PathBuf,
) -> Value {
    json!({
        "kind": "measure_confirmation_outcomes",
        "store_path": store_path.display().to_string(),
        "snapshots_path": snapshots_path.display().to_string(),
        "total_confirmed_signals_processed": report.total_confirmed_signals_processed,
        "favorable": report.favorable,
        "unfavorable": report.unfavorable,
        "neutral": report.neutral,
        "insufficient_data": report.insufficient_data,
        "scorecard": {
            "processed_count": scorecard.processed_count,
            "favorable_rate": scorecard.favorable_rate,
            "unfavorable_rate": scorecard.unfavorable_rate,
            "neutral_rate": scorecard.neutral_rate,
            "insufficient_data_rate": scorecard.insufficient_data_rate,
            "average_delta_probability": scorecard.average_delta_probability,
            "by_direction": scorecard.by_direction.iter().map(|item| json!({
                "direction": format!("{:?}", item.direction),
                "favorable": item.favorable,
                "unfavorable": item.unfavorable,
                "neutral": item.neutral,
                "insufficient_data": item.insufficient_data,
            })).collect::<Vec<_>>(),
            "by_signal_name": scorecard.by_signal_name.iter().map(|(signal_name, count)| json!({
                "signal_name": signal_name,
                "count": count,
            })).collect::<Vec<_>>(),
        },
        "outcomes": report.outcomes.iter().map(|record| json!({
            "signal_id": record.signal_id,
            "market_id": record.market_id,
            "signal_name": record.signal_name,
            "direction": format!("{:?}", record.direction),
            "confirmed_at": record.confirmed_at.to_rfc3339(),
            "evaluation_horizon_seconds": record.evaluation_horizon_seconds,
            "entry_probability": record.entry_probability,
            "exit_probability": record.exit_probability,
            "delta_probability": record.delta_probability,
            "outcome_label": match record.outcome_label {
                crate::ConfirmationOutcomeLabel::Favorable => "favorable",
                crate::ConfirmationOutcomeLabel::Unfavorable => "unfavorable",
                crate::ConfirmationOutcomeLabel::Neutral => "neutral",
                crate::ConfirmationOutcomeLabel::InsufficientData => "insufficient_data",
            },
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn confirmation_policy(
    comparison: &ConfirmationComparisonReport,
    sweep: &PolicySweepSummary,
    advisory: &[ConfirmationPolicyAdvisory],
    store_path: &PathBuf,
    snapshots_path: &PathBuf,
) -> Value {
    json!({
        "kind": "evaluate_confirmation_policy",
        "store_path": store_path.display().to_string(),
        "snapshots_path": snapshots_path.display().to_string(),
        "comparison": {
            "favorable_rate_confirmed": comparison.favorable_rate_confirmed,
            "favorable_rate_all_generated": comparison.favorable_rate_all_generated,
            "uplift_favorable_rate": comparison.uplift_favorable_rate,
            "unfavorable_rate_confirmed": comparison.unfavorable_rate_confirmed,
            "unfavorable_rate_all_generated": comparison.unfavorable_rate_all_generated,
            "average_delta_confirmed": comparison.average_delta_confirmed,
            "average_delta_all_generated": comparison.average_delta_all_generated,
            "by_signal_name": comparison.by_signal_name.iter().map(comparison_breakdown_json).collect::<Vec<_>>(),
            "by_direction": comparison.by_direction.iter().map(comparison_breakdown_json).collect::<Vec<_>>(),
            "by_source": comparison.by_source.iter().map(comparison_breakdown_json).collect::<Vec<_>>(),
        },
        "sweep": sweep.rows.iter().map(policy_sweep_row_json).collect::<Vec<_>>(),
        "advisory": advisory.iter().map(|item| json!({
            "signal_name": item.signal_name,
            "sample_count": item.sample_count,
            "favorable_rate": item.favorable_rate,
            "unfavorable_rate": item.unfavorable_rate,
            "classification": match item.classification {
                crate::ConfirmationPolicyAdvisoryClassification::PromoteCandidate => "promote_candidate",
                crate::ConfirmationPolicyAdvisoryClassification::Review => "review",
                crate::ConfirmationPolicyAdvisoryClassification::FreezeCandidate => "freeze_candidate",
            }
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn walkforward_confirmation_policy(
    report: &ConfirmationWalkForwardReport,
    store_path: &PathBuf,
    snapshots_path: &PathBuf,
) -> Value {
    json!({
        "kind": "walkforward_confirmation_policy",
        "store_path": store_path.display().to_string(),
        "snapshots_path": snapshots_path.display().to_string(),
        "eras": report.eras.iter().map(|era| json!({
            "era_id": era.era_id,
            "start_time": era.start_time.to_rfc3339(),
            "end_time": era.end_time.to_rfc3339(),
            "label": era.label,
        })).collect::<Vec<_>>(),
        "steps": report.steps.iter().map(|step| json!({
            "train_era_id": step.train_era_id,
            "validation_era_id": step.validation_era_id,
            "chosen_confidence_threshold": step.chosen_confidence_threshold,
            "chosen_horizon_seconds": step.chosen_horizon_seconds,
            "train_metrics": policy_sweep_row_json(&step.train_metrics),
            "validation_metrics": policy_sweep_row_json(&step.validation_metrics),
            "uplift_vs_baseline_validation": step.validation_metrics.uplift_vs_baseline,
            "confirmed_sample_count_validation": step.confirmed_sample_count_validation,
            "validation_advisory": step.validation_advisory.iter().map(|item| json!({
                "signal_name": item.signal_name,
                "sample_count": item.sample_count,
                "favorable_rate": item.favorable_rate,
                "unfavorable_rate": item.unfavorable_rate,
                "classification": match item.classification {
                    crate::ConfirmationPolicyAdvisoryClassification::PromoteCandidate => "promote_candidate",
                    crate::ConfirmationPolicyAdvisoryClassification::Review => "review",
                    crate::ConfirmationPolicyAdvisoryClassification::FreezeCandidate => "freeze_candidate",
                }
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "summary": {
            "average_validation_favorable_rate": report.summary.average_validation_favorable_rate,
            "average_validation_uplift": report.summary.average_validation_uplift,
            "chosen_thresholds": report.summary.chosen_thresholds.iter().map(|item| json!({
                "value": item.value,
                "count": item.count,
                "rate": item.rate,
            })).collect::<Vec<_>>(),
            "chosen_horizons": report.summary.chosen_horizons.iter().map(|item| json!({
                "value": item.value,
                "count": item.count,
                "rate": item.rate,
            })).collect::<Vec<_>>(),
            "advisory_stability": report.summary.advisory_stability.iter().map(|item| json!({
                "signal_name": item.signal_name,
                "promote_candidate_count": item.promote_candidate_count,
                "review_count": item.review_count,
                "freeze_candidate_count": item.freeze_candidate_count,
            })).collect::<Vec<_>>(),
        }
    })
}

pub(crate) fn propose_confirmation_policy(
    proposal: &ConfirmationPolicyProposal,
    output_path: &Option<PathBuf>,
) -> Value {
    json!({
        "kind": "propose_confirmation_policy",
        "output_path": output_path.as_ref().map(|path| path.display().to_string()),
        "summary": {
            "promoted_rules": proposal.summary.promoted_rules,
            "review_rules": proposal.summary.review_rules,
            "frozen_rules": proposal.summary.frozen_rules,
        },
        "policy": proposal.policy,
    })
}

pub(crate) fn research_signal_ingest(
    report: &ResearchSignalIngestReport,
    store_path: &PathBuf,
) -> Value {
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

pub(crate) fn decision_materialization(
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

pub(crate) fn order_materialization(
    report: &OrderMaterializationReport,
    store_path: &PathBuf,
) -> Value {
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

pub(crate) fn order_submission(report: &OrderSubmissionReport, store_path: &PathBuf) -> Value {
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

pub(crate) fn fill_observation(report: &FillObservationReport, store_path: &PathBuf) -> Value {
    json!({
        "kind": "observe_fill",
        "store_path": store_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "fill_id": report.fill_id,
        "order_id": report.order_id,
        "order_status_before": report.order_status_before,
        "resolved_decision_id": report.resolved_decision_id,
        "resolved_instrument": report.resolved_instrument,
        "resolved_venue": report.resolved_venue,
        "disposition": format!("{:?}", report.disposition),
        "persisted": report.persisted,
        "duplicate": report.duplicate,
        "reasons": report.reasons,
        "notes": report.notes,
    })
}

pub(crate) fn summary(summary: &ObservabilitySummary, store_path: &PathBuf) -> Value {
    json!({
        "kind": "summary",
        "store_path": store_path.display().to_string(),
        "summary": summary,
    })
}

pub(crate) fn batch_run(report: &BatchRunReport) -> Value {
    json!({
        "kind": "batch_run",
        "store_path": report.store_path.display().to_string(),
        "research_signals_path": report.research_signals_path.display().to_string(),
        "dry_run": report.dry_run,
        "batch_trace_id": report.batch_trace_id,
        "success": report.success,
        "ingest": research_signal_ingest(&report.ingest, &report.store_path),
        "materialization": decision_materialization(&report.materialization, &report.store_path),
        "final_summary": summary(&report.final_summary, &report.store_path),
    })
}

pub(crate) fn policy_only_signal(
    signal_id: &str,
    policy: &SignalPromotionReport,
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "signal_policy",
        "store_path": store_path.display().to_string(),
        "signal_id": signal_id,
        "promotion_policy": signal_promotion_json(policy),
    })
}

pub(crate) fn policy_only_decision(
    decision_id: &str,
    policy: &DecisionPromotionReport,
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "decision_policy",
        "store_path": store_path.display().to_string(),
        "decision_id": decision_id,
        "promotion_policy": decision_promotion_json(policy),
    })
}

pub(crate) fn policy_only_order(
    order_id: &str,
    policy: &OrderPromotionReport,
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "order_policy",
        "store_path": store_path.display().to_string(),
        "order_id": order_id,
        "promotion_policy": order_promotion_json(policy),
    })
}

pub(crate) fn signal(
    signal_id: &str,
    projection: Option<&SignalProjection>,
    readiness: Option<&SignalReadiness>,
    governance: Option<&SignalGovernanceReport>,
    policy: Option<&SignalPromotionReport>,
    timeline: &[StoredEvent],
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "signal",
        "store_path": store_path.display().to_string(),
        "signal_id": signal_id,
        "projection": projection.map(signal_projection_json),
        "readiness": readiness.map(signal_readiness_json),
        "governance": governance.map(signal_governance_json),
        "promotion_policy": policy.map(signal_promotion_json),
        "timeline": event_timeline_json(timeline),
    })
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
) -> Value {
    json!({
        "kind": "decision",
        "store_path": store_path.display().to_string(),
        "decision_id": decision_id,
        "projection": projection.map(decision_projection_json),
        "readiness": readiness.map(decision_readiness_json),
        "lineage": lineage.map(decision_lineage_json),
        "governance": governance.map(decision_governance_json),
        "promotion_policy": policy.map(decision_promotion_json),
        "execution_boundary": boundary.map(execution_boundary_json),
        "timeline": event_timeline_json(timeline),
    })
}

pub(crate) fn order(
    order_id: &str,
    lifecycle: Option<&OrderLifecycleReport>,
    execution: Option<&OrderExecutionSummary>,
    policy: Option<&OrderPromotionReport>,
    submission_policy: Option<&OrderSubmissionPolicyReport>,
    related_events: &[StoredEvent],
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "order",
        "store_path": store_path.display().to_string(),
        "order_id": order_id,
        "lifecycle": lifecycle.map(order_lifecycle_json),
        "execution_summary": execution.map(order_execution_json),
        "promotion_policy": policy.map(order_promotion_json),
        "submission_policy": submission_policy.map(order_submission_policy_json),
        "related_events": event_timeline_json(related_events),
    })
}

pub(crate) fn fill(
    fill_id: &str,
    readiness: Option<&FillReadiness>,
    boundary: Option<&ExecutionBoundaryReport>,
    matching_events: &[StoredEvent],
    store_path: &PathBuf,
) -> Value {
    json!({
        "kind": "fill",
        "store_path": store_path.display().to_string(),
        "fill_id": fill_id,
        "readiness": readiness.map(fill_readiness_json),
        "execution_boundary": boundary.map(execution_boundary_json),
        "matching_events": event_timeline_json(matching_events),
    })
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

fn order_execution_json(report: &OrderExecutionSummary) -> Value {
    json!({
        "order_id": report.order_id,
        "decision_id": report.decision_id,
        "ordered_quantity": report.ordered_quantity,
        "filled_quantity": report.filled_quantity,
        "remaining_quantity": report.remaining_quantity,
        "average_fill_price": report.average_fill_price,
        "fill_count": report.fill_count,
        "execution_status": report.execution_status.as_str(),
        "reasons": report.reasons,
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

fn comparison_breakdown_json(row: &crate::ComparisonBreakdownRow) -> Value {
    json!({
        "key": row.key,
        "confirmed_count": row.confirmed_count,
        "all_generated_count": row.all_generated_count,
        "favorable_rate_confirmed": row.favorable_rate_confirmed,
        "favorable_rate_all_generated": row.favorable_rate_all_generated,
        "uplift_favorable_rate": row.uplift_favorable_rate,
        "unfavorable_rate_confirmed": row.unfavorable_rate_confirmed,
        "unfavorable_rate_all_generated": row.unfavorable_rate_all_generated,
        "average_delta_confirmed": row.average_delta_confirmed,
        "average_delta_all_generated": row.average_delta_all_generated,
    })
}

fn policy_sweep_breakdown_json(row: &crate::PolicySweepBreakdownRow) -> Value {
    json!({
        "key": row.key,
        "total_signals": row.total_signals,
        "confirmed_signals": row.confirmed_signals,
        "favorable_rate": row.favorable_rate,
        "unfavorable_rate": row.unfavorable_rate,
    })
}

fn policy_sweep_row_json(row: &crate::PolicySweepResultRow) -> Value {
    json!({
        "confidence_threshold": row.confidence_threshold,
        "horizon_seconds": row.horizon_seconds,
        "total_signals": row.total_signals,
        "confirmed_signals": row.confirmed_signals,
        "acceptance_rate": row.acceptance_rate,
        "favorable_rate": row.favorable_rate,
        "unfavorable_rate": row.unfavorable_rate,
        "average_delta_probability": row.average_delta_probability,
        "uplift_vs_baseline": row.uplift_vs_baseline,
        "by_signal_name": row.by_signal_name.iter().map(policy_sweep_breakdown_json).collect::<Vec<_>>(),
        "by_direction": row.by_direction.iter().map(policy_sweep_breakdown_json).collect::<Vec<_>>(),
        "by_source": row.by_source.iter().map(policy_sweep_breakdown_json).collect::<Vec<_>>(),
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

fn debug_list<T>(items: &[T]) -> Vec<String>
where
    T: Debug,
{
    items.iter().map(|item| format!("{item:?}")).collect()
}

#[cfg(test)]
mod tests {
    use super::confirmation_run;
    use crate::{
        ConfirmationDisposition, ConfirmationRunItem, ConfirmationRunReport, ConfirmationScorecard,
    };

    #[test]
    fn confirmation_run_json_is_stable() {
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
            emitted_events: Vec::new(),
        };
        let scorecard = ConfirmationScorecard::from_report(&report);
        let value = confirmation_run(
            &report,
            &scorecard,
            &std::path::PathBuf::from("var/events.jsonl"),
        );
        assert_eq!(value["kind"], "confirm_signals");
        assert_eq!(value["accepted"], 1);
        assert_eq!(value["scorecard"]["acceptance_rate"], 1.0);
    }
}
