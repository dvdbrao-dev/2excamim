use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runtime::PaperPipelineReport;
use crate::{
    project_paper_ledger, read_confirmation_readiness_report, ConfirmationPolicy,
    ConfirmationPolicyLoadError, ConfirmationReadinessReport, ConfirmationReadinessStatus,
    FillSide, JsonlEventStore, PaperFillView, PaperLedgerProjection, PaperOrderView, StoreError,
};

const RECENT_ACTIVITY_LIMIT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationalSummaryFormat {
    Json,
    Markdown,
}

impl OperationalSummaryFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalSummary {
    pub pipeline: OperationalPipelineRecap,
    pub paper: OperationalPaperState,
    pub governance: OperationalGovernanceSnapshot,
    pub recent_activity: OperationalRecentActivity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalPipelineRecap {
    pub report_available: bool,
    pub signals_seen: Option<usize>,
    pub signals_confirmed: Option<usize>,
    pub execution_requests_sent: Option<usize>,
    pub fills_persisted: Option<usize>,
    pub blocked_by_risk: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalPaperState {
    pub open_positions: usize,
    pub closed_positions: usize,
    pub total_notional_spent: f64,
    pub total_notional_received: f64,
    pub realized_pnl_total: f64,
    pub unrealized_pnl_total: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalGovernanceSnapshot {
    pub readiness_available: bool,
    pub experimental: Option<usize>,
    pub candidate: Option<usize>,
    pub promoted: Option<usize>,
    pub frozen: Option<usize>,
    pub promoted_families: Vec<String>,
    pub frozen_families: Vec<String>,
    pub policy_available: bool,
    pub policy_rules: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalRecentActivity {
    pub fills: Vec<OperationalRecentFill>,
    pub orders: Vec<OperationalRecentOrder>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalRecentFill {
    pub fill_id: String,
    pub order_id: String,
    pub instrument: String,
    pub outcome: String,
    pub side: FillSide,
    pub quantity: f64,
    pub price: f64,
    pub notional: f64,
    pub executed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalRecentOrder {
    pub order_id: String,
    pub decision_id: Option<String>,
    pub signal_id: Option<String>,
    pub instrument: String,
    pub venue: String,
    pub status: String,
    pub fill_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationalSummaryConfig {
    pub store_path: PathBuf,
    pub readiness_path: PathBuf,
    pub policy_path: Option<PathBuf>,
    pub pipeline_report_path: PathBuf,
    pub output_path: PathBuf,
    pub format: OperationalSummaryFormat,
}

pub fn build_operational_summary(
    config: &OperationalSummaryConfig,
) -> Result<OperationalSummary, OperationalSummaryError> {
    let store = JsonlEventStore::new(&config.store_path)?;
    let events = store.read_all()?;
    let ledger = project_paper_ledger(&events)?;
    let pipeline = read_optional_pipeline_report(&config.pipeline_report_path)?;
    let readiness = read_optional_readiness_report(&config.readiness_path)?;
    let policy = read_optional_policy(config.policy_path.as_deref())?;

    Ok(OperationalSummary {
        pipeline: pipeline_recap(pipeline.as_ref()),
        paper: paper_state(&ledger),
        governance: governance_snapshot(readiness.as_ref(), policy.as_ref()),
        recent_activity: recent_activity(&ledger),
    })
}

pub fn write_operational_summary(
    config: &OperationalSummaryConfig,
) -> Result<OperationalSummary, OperationalSummaryError> {
    let summary = build_operational_summary(config)?;
    if let Some(parent) = config.output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match config.format {
        OperationalSummaryFormat::Json => {
            let file = std::fs::File::create(&config.output_path)?;
            serde_json::to_writer_pretty(file, &summary)?;
        }
        OperationalSummaryFormat::Markdown => {
            std::fs::write(
                &config.output_path,
                render_operational_summary_markdown(&summary),
            )?;
        }
    }
    Ok(summary)
}

pub fn render_operational_summary_markdown(summary: &OperationalSummary) -> String {
    let mut markdown = String::new();
    markdown.push_str("# 2EXCAMIM Operational Summary\n\n");

    markdown.push_str("## Pipeline Recap\n\n");
    markdown.push_str(&format!(
        "- report_available: {}\n",
        summary.pipeline.report_available
    ));
    markdown.push_str(&optional_usize_line(
        "signals_seen",
        summary.pipeline.signals_seen,
    ));
    markdown.push_str(&optional_usize_line(
        "signals_confirmed",
        summary.pipeline.signals_confirmed,
    ));
    markdown.push_str(&optional_usize_line(
        "execution_requests_sent",
        summary.pipeline.execution_requests_sent,
    ));
    markdown.push_str(&optional_usize_line(
        "fills_persisted",
        summary.pipeline.fills_persisted,
    ));
    markdown.push_str(&optional_usize_line(
        "blocked_by_risk",
        summary.pipeline.blocked_by_risk,
    ));

    markdown.push_str("\n## Paper State\n\n");
    markdown.push_str(&format!(
        "- open_positions: {}\n- closed_positions: {}\n- total_notional_spent: {:.8}\n- total_notional_received: {:.8}\n- realized_pnl_total: {:.8}\n- unrealized_pnl_total: {}\n",
        summary.paper.open_positions,
        summary.paper.closed_positions,
        summary.paper.total_notional_spent,
        summary.paper.total_notional_received,
        summary.paper.realized_pnl_total,
        summary.paper
            .unrealized_pnl_total
            .map(|value| format!("{value:.8}"))
            .unwrap_or_else(|| "null".to_string())
    ));

    markdown.push_str("\n## Governance Snapshot\n\n");
    markdown.push_str(&format!(
        "- readiness_available: {}\n",
        summary.governance.readiness_available
    ));
    markdown.push_str(&optional_usize_line(
        "experimental",
        summary.governance.experimental,
    ));
    markdown.push_str(&optional_usize_line(
        "candidate",
        summary.governance.candidate,
    ));
    markdown.push_str(&optional_usize_line(
        "promoted",
        summary.governance.promoted,
    ));
    markdown.push_str(&optional_usize_line("frozen", summary.governance.frozen));
    markdown.push_str(&format!(
        "- policy_available: {}\n",
        summary.governance.policy_available
    ));
    markdown.push_str(&optional_usize_line(
        "policy_rules",
        summary.governance.policy_rules,
    ));
    append_markdown_list(
        &mut markdown,
        "Promoted Families",
        &summary.governance.promoted_families,
    );
    append_markdown_list(
        &mut markdown,
        "Frozen Families",
        &summary.governance.frozen_families,
    );

    markdown.push_str("\n## Recent Activity\n\n");
    markdown.push_str("### Recent Fills\n\n");
    if summary.recent_activity.fills.is_empty() {
        markdown.push_str("- none\n");
    } else {
        for fill in &summary.recent_activity.fills {
            markdown.push_str(&format!(
                "- {} {} {} {} @ {:.8} notional {:.8} at {}\n",
                fill.fill_id,
                fill_side_label(fill.side),
                fill.quantity,
                fill.instrument,
                fill.price,
                fill.notional,
                fill.executed_at.to_rfc3339()
            ));
        }
    }
    markdown.push_str("\n### Recent Orders\n\n");
    if summary.recent_activity.orders.is_empty() {
        markdown.push_str("- none\n");
    } else {
        for order in &summary.recent_activity.orders {
            markdown.push_str(&format!(
                "- {} {} {} fills:{}\n",
                order.order_id, order.status, order.instrument, order.fill_count
            ));
        }
    }

    markdown
}

fn pipeline_recap(pipeline: Option<&PaperPipelineReport>) -> OperationalPipelineRecap {
    OperationalPipelineRecap {
        report_available: pipeline.is_some(),
        signals_seen: pipeline.map(|report| report.signals_seen),
        signals_confirmed: pipeline.map(|report| report.signals_confirmed),
        execution_requests_sent: pipeline.map(|report| report.execution_requests_sent),
        fills_persisted: pipeline.map(|report| report.fills_persisted),
        blocked_by_risk: pipeline.map(|report| report.blocked_by_risk),
    }
}

fn paper_state(ledger: &PaperLedgerProjection) -> OperationalPaperState {
    OperationalPaperState {
        open_positions: ledger.summary.open_positions,
        closed_positions: ledger.summary.closed_positions,
        total_notional_spent: ledger.summary.total_notional_spent,
        total_notional_received: ledger.summary.total_notional_received,
        realized_pnl_total: ledger.summary.realized_pnl_total,
        unrealized_pnl_total: ledger.summary.unrealized_pnl_total,
    }
}

fn governance_snapshot(
    readiness: Option<&ConfirmationReadinessReport>,
    policy: Option<&ConfirmationPolicy>,
) -> OperationalGovernanceSnapshot {
    let (experimental, candidate, promoted, frozen, promoted_families, frozen_families) =
        if let Some(readiness) = readiness {
            (
                Some(readiness.summary.experimental),
                Some(readiness.summary.candidate),
                Some(readiness.summary.promoted),
                Some(readiness.summary.frozen),
                readiness
                    .states
                    .iter()
                    .filter(|state| state.readiness_status == ConfirmationReadinessStatus::Promoted)
                    .map(readiness_family_key)
                    .collect(),
                readiness
                    .states
                    .iter()
                    .filter(|state| state.readiness_status == ConfirmationReadinessStatus::Frozen)
                    .map(readiness_family_key)
                    .collect(),
            )
        } else {
            (None, None, None, None, Vec::new(), Vec::new())
        };

    OperationalGovernanceSnapshot {
        readiness_available: readiness.is_some(),
        experimental,
        candidate,
        promoted,
        frozen,
        promoted_families,
        frozen_families,
        policy_available: policy.is_some(),
        policy_rules: policy.map(|policy| policy.rules.len()),
    }
}

fn recent_activity(ledger: &PaperLedgerProjection) -> OperationalRecentActivity {
    let mut fills = ledger.fills.clone();
    fills.sort_by(|left, right| {
        right
            .executed_at
            .cmp(&left.executed_at)
            .then_with(|| left.fill_id.cmp(&right.fill_id))
    });
    let fills = fills
        .iter()
        .take(RECENT_ACTIVITY_LIMIT)
        .map(recent_fill)
        .collect();

    let mut orders = ledger.orders.clone();
    orders.sort_by(|left, right| {
        right
            .submitted_at
            .or(right.registered_at)
            .cmp(&left.submitted_at.or(left.registered_at))
            .then_with(|| left.order_id.cmp(&right.order_id))
    });
    let orders = orders
        .iter()
        .take(RECENT_ACTIVITY_LIMIT)
        .map(recent_order)
        .collect();

    OperationalRecentActivity { fills, orders }
}

fn recent_fill(fill: &PaperFillView) -> OperationalRecentFill {
    OperationalRecentFill {
        fill_id: fill.fill_id.clone(),
        order_id: fill.order_id.clone(),
        instrument: fill.instrument.clone(),
        outcome: fill.outcome.clone(),
        side: fill.side,
        quantity: fill.quantity,
        price: fill.price,
        notional: fill.notional,
        executed_at: fill.executed_at,
    }
}

fn recent_order(order: &PaperOrderView) -> OperationalRecentOrder {
    OperationalRecentOrder {
        order_id: order.order_id.clone(),
        decision_id: order.decision_id.clone(),
        signal_id: order.signal_id.clone(),
        instrument: order.instrument.clone(),
        venue: order.venue.clone(),
        status: format!("{:?}", order.status),
        fill_count: order.fill_count,
    }
}

fn readiness_family_key(state: &crate::ConfirmationReadinessState) -> String {
    let mut key = state.signal_name.clone();
    if let Some(direction) = state.direction.as_ref() {
        key.push_str(" / ");
        key.push_str(&format!("{direction:?}"));
    }
    if let Some(source) = state.source.as_ref() {
        key.push_str(" / ");
        key.push_str(&format!("{source:?}"));
    }
    key
}

fn fill_side_label(side: FillSide) -> &'static str {
    match side {
        FillSide::Buy => "buy",
        FillSide::Sell => "sell",
    }
}

fn append_markdown_list(markdown: &mut String, title: &str, values: &[String]) {
    markdown.push_str(&format!("\n### {title}\n\n"));
    if values.is_empty() {
        markdown.push_str("- none\n");
    } else {
        for value in values {
            markdown.push_str(&format!("- {value}\n"));
        }
    }
}

fn optional_usize_line(label: &str, value: Option<usize>) -> String {
    format!(
        "- {label}: {}\n",
        value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string())
    )
}

fn read_optional_pipeline_report(
    path: &Path,
) -> Result<Option<PaperPipelineReport>, OperationalSummaryError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    if let Ok(report) = serde_json::from_slice::<PaperPipelineReport>(&bytes) {
        return Ok(Some(report));
    }
    let wrapped = serde_json::from_slice::<PipelineReportWrapper>(&bytes)?;
    Ok(Some(wrapped.report))
}

fn read_optional_readiness_report(
    path: &Path,
) -> Result<Option<ConfirmationReadinessReport>, OperationalSummaryError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_confirmation_readiness_report(path)?))
}

fn read_optional_policy(
    path: Option<&Path>,
) -> Result<Option<ConfirmationPolicy>, OperationalSummaryError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(ConfirmationPolicy::from_file(path)?))
}

#[derive(Debug, Deserialize)]
struct PipelineReportWrapper {
    report: PaperPipelineReport,
}

#[derive(Debug)]
pub enum OperationalSummaryError {
    Store(StoreError),
    Codec(crate::CodecError),
    Policy(ConfirmationPolicyLoadError),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for OperationalSummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Codec(error) => write!(f, "{error}"),
            Self::Policy(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for OperationalSummaryError {}

impl From<StoreError> for OperationalSummaryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<crate::CodecError> for OperationalSummaryError {
    fn from(value: crate::CodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<ConfirmationPolicyLoadError> for OperationalSummaryError {
    fn from(value: ConfirmationPolicyLoadError) -> Self {
        Self::Policy(value)
    }
}

impl From<std::io::Error> for OperationalSummaryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for OperationalSummaryError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::{
        EventEnvelope, FillReceived, Linkage, OrderRegistered, OrderSubmitted, Provenance,
        SourceKind, StoredEvent, POLYMARKET_PAPER_VENUE,
    };

    fn temp_path(name: &str, suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("twoexcamim-summary-{name}-{nanos}.{suffix}"))
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    fn provenance() -> Provenance {
        Provenance {
            source_kind: SourceKind::Derived,
            source_ref: Some("summary-test".into()),
            producer_run_id: Some("summary-run".into()),
            actor: Some("tests".into()),
            trace_id: Some("summary-trace".into()),
            notes: None,
        }
    }

    fn linkage() -> Linkage {
        Linkage {
            signal_id: Some("sig-summary".into()),
            decision_id: Some("dec-summary".into()),
            order_id: Some("ord-summary".into()),
            correlation_id: Some("corr-summary".into()),
            ..Linkage::default()
        }
    }

    fn stored<T>(event: EventEnvelope<T>) -> StoredEvent
    where
        T: serde::Serialize,
    {
        StoredEvent::try_from(event).unwrap()
    }

    fn append_paper_fixture(store: &JsonlEventStore) {
        let linkage = linkage();
        store
            .append_events(&[
                stored(
                    EventEnvelope::new_order_registered(
                        "paper-runner",
                        Some("market-summary".into()),
                        linkage.clone(),
                        provenance(),
                        OrderRegistered {
                            order_id: "ord-summary".into(),
                            decision_id: Some("dec-summary".into()),
                            instrument: "market-summary".into(),
                            venue: POLYMARKET_PAPER_VENUE.into(),
                        },
                    )
                    .unwrap(),
                ),
                stored(
                    EventEnvelope::new_order_submitted(
                        "paper-runner",
                        Some("market-summary".into()),
                        linkage.clone(),
                        provenance(),
                        OrderSubmitted {
                            order_id: "ord-summary".into(),
                            decision_id: Some("dec-summary".into()),
                            instrument: "market-summary".into(),
                            venue: POLYMARKET_PAPER_VENUE.into(),
                        },
                    )
                    .unwrap(),
                ),
                stored(
                    EventEnvelope::new_fill_received(
                        "paper-runner",
                        Some("market-summary".into()),
                        linkage,
                        provenance(),
                        FillReceived {
                            fill_id: "fill-summary".into(),
                            decision_id: Some("dec-summary".into()),
                            order_id: "ord-summary".into(),
                            instrument: "market-summary".into(),
                            side: FillSide::Buy,
                            quantity: 10.0,
                            price: 0.4,
                            venue: POLYMARKET_PAPER_VENUE.into(),
                            executed_at: Utc.with_ymd_and_hms(2026, 4, 14, 12, 0, 0).unwrap(),
                        },
                    )
                    .unwrap(),
                ),
            ])
            .unwrap();
    }

    #[test]
    fn summary_generation_uses_canonical_fixtures() {
        let store_path = temp_path("canonical", "jsonl");
        let pipeline_path = temp_path("pipeline", "json");
        let output_path = temp_path("output", "json");
        let store = JsonlEventStore::new(&store_path).unwrap();
        append_paper_fixture(&store);
        std::fs::write(
            &pipeline_path,
            serde_json::to_string(&PaperPipelineReport {
                dry_run: false,
                stages: vec!["load_existing_signals".into()],
                signals_seen: 2,
                signals_generated: 0,
                signals_confirmed: 1,
                execution_requests_sent: 1,
                fills_persisted: 1,
                blocked_by_risk: 0,
                open_positions: 1,
                total_notional_spent: 4.0,
                total_notional_received: 0.0,
                readiness_states_materialized: None,
            })
            .unwrap(),
        )
        .unwrap();

        let summary = write_operational_summary(&OperationalSummaryConfig {
            store_path: store_path.clone(),
            readiness_path: temp_path("missing-readiness", "json"),
            policy_path: None,
            pipeline_report_path: pipeline_path.clone(),
            output_path: output_path.clone(),
            format: OperationalSummaryFormat::Json,
        })
        .unwrap();
        let loaded: OperationalSummary =
            serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();

        assert_eq!(summary, loaded);
        assert_eq!(summary.pipeline.signals_seen, Some(2));
        assert_eq!(summary.pipeline.fills_persisted, Some(1));
        assert_eq!(summary.paper.open_positions, 1);
        assert_eq!(summary.paper.total_notional_spent, 4.0);
        assert_eq!(summary.recent_activity.fills[0].fill_id, "fill-summary");
        assert_eq!(summary.recent_activity.orders[0].order_id, "ord-summary");

        cleanup(&store_path);
        cleanup(&pipeline_path);
        cleanup(&output_path);
    }

    #[test]
    fn summary_is_safe_when_optional_artifacts_are_missing() {
        let store_path = temp_path("missing", "jsonl");
        let output_path = temp_path("missing-output", "json");
        let store = JsonlEventStore::new(&store_path).unwrap();
        append_paper_fixture(&store);

        let summary = write_operational_summary(&OperationalSummaryConfig {
            store_path: store_path.clone(),
            readiness_path: temp_path("missing-readiness", "json"),
            policy_path: Some(temp_path("missing-policy", "json")),
            pipeline_report_path: temp_path("missing-pipeline", "json"),
            output_path: output_path.clone(),
            format: OperationalSummaryFormat::Json,
        })
        .unwrap();

        assert!(!summary.pipeline.report_available);
        assert_eq!(summary.pipeline.signals_seen, None);
        assert!(!summary.governance.readiness_available);
        assert_eq!(summary.governance.promoted_families, Vec::<String>::new());
        assert!(!summary.governance.policy_available);

        cleanup(&store_path);
        cleanup(&output_path);
    }

    #[test]
    fn markdown_rendering_contains_core_sections() {
        let summary = OperationalSummary {
            pipeline: OperationalPipelineRecap {
                report_available: true,
                signals_seen: Some(1),
                signals_confirmed: Some(1),
                execution_requests_sent: Some(1),
                fills_persisted: Some(1),
                blocked_by_risk: Some(0),
            },
            paper: OperationalPaperState {
                open_positions: 1,
                closed_positions: 0,
                total_notional_spent: 4.0,
                total_notional_received: 0.0,
                realized_pnl_total: 0.0,
                unrealized_pnl_total: None,
            },
            governance: OperationalGovernanceSnapshot {
                readiness_available: false,
                experimental: None,
                candidate: None,
                promoted: None,
                frozen: None,
                promoted_families: Vec::new(),
                frozen_families: Vec::new(),
                policy_available: false,
                policy_rules: None,
            },
            recent_activity: OperationalRecentActivity {
                fills: Vec::new(),
                orders: Vec::new(),
            },
        };

        let markdown = render_operational_summary_markdown(&summary);

        assert!(markdown.contains("# 2EXCAMIM Operational Summary"));
        assert!(markdown.contains("## Pipeline Recap"));
        assert!(markdown.contains("## Paper State"));
        assert!(markdown.contains("## Governance Snapshot"));
        assert!(markdown.contains("## Recent Activity"));
        assert!(markdown.contains("- signals_seen: 1"));
    }
}
