use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
    project_paper_ledger, read_confirmation_readiness_report, ConfirmationPolicy,
    ConfirmationPolicyLoadError, ConfirmationReadinessReport, PaperLedgerProjection,
    PaperOrderStatus, StoreError,
};

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardConfig {
    pub store_path: PathBuf,
    pub readiness_path: PathBuf,
    pub policy_path: Option<PathBuf>,
    pub pipeline_report_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardData {
    pub pipeline: Option<crate::runtime::PaperPipelineReport>,
    pub ledger: PaperLedgerProjection,
    pub readiness: Option<ConfirmationReadinessReport>,
    pub policy: Option<ConfirmationPolicy>,
    pub recent_risk_blocks: Vec<String>,
}

pub fn load_dashboard_data(config: &DashboardConfig) -> Result<DashboardData, DashboardError> {
    let store = crate::JsonlEventStore::new(&config.store_path)?;
    let events = store.read_all()?;
    let ledger = project_paper_ledger(&events)?;
    let pipeline = read_optional_pipeline_report(&config.pipeline_report_path)?;
    let readiness = read_optional_readiness_report(&config.readiness_path)?;
    let policy = read_optional_policy(config.policy_path.as_deref())?;
    let recent_risk_blocks = pipeline
        .as_ref()
        .filter(|report| report.blocked_by_risk > 0)
        .map(|report| {
            vec![format!(
                "latest_pipeline blocked_by_risk={}",
                report.blocked_by_risk
            )]
        })
        .unwrap_or_default();

    Ok(DashboardData {
        pipeline,
        ledger,
        readiness,
        policy,
        recent_risk_blocks,
    })
}

pub fn write_dashboard(config: &DashboardConfig) -> Result<(), DashboardError> {
    let data = load_dashboard_data(config)?;
    let html = render_dashboard_html(&data);
    if let Some(parent) = config.output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config.output_path, html)?;
    Ok(())
}

pub fn render_dashboard_html(data: &DashboardData) -> String {
    let pipeline = data.pipeline.as_ref();
    let readiness = data.readiness.as_ref();
    let policy = data.policy.as_ref();
    let promoted = readiness
        .map(|report| {
            report
                .states
                .iter()
                .filter(|state| {
                    state.readiness_status == crate::ConfirmationReadinessStatus::Promoted
                })
                .map(|state| state.signal_name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let frozen = readiness
        .map(|report| {
            report
                .states
                .iter()
                .filter(|state| {
                    state.readiness_status == crate::ConfirmationReadinessStatus::Frozen
                })
                .map(|state| state.signal_name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<title>2EXCAMIM Control Room</title>");
    html.push_str("<style>");
    html.push_str("body{font-family:system-ui,-apple-system,Segoe UI,sans-serif;margin:0;background:#f6f7f9;color:#16181d}main{max-width:1180px;margin:0 auto;padding:24px}h1{margin:0 0 4px}section{background:#fff;border:1px solid #dfe3ea;border-radius:8px;padding:16px;margin:16px 0}h2{margin:0 0 12px;font-size:18px}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:10px}.metric{border:1px solid #e6e9ef;border-radius:6px;padding:10px}.label{font-size:12px;color:#5d6675}.value{font-size:22px;font-weight:650;margin-top:4px}table{border-collapse:collapse;width:100%;font-size:14px}th,td{text-align:left;border-bottom:1px solid #eceff4;padding:8px}th{color:#4b5565}ul{margin:8px 0 0 18px;padding:0}.muted{color:#667085}.ok{color:#137333}.warn{color:#9a3412}");
    html.push_str("</style></head><body><main>");
    html.push_str("<h1>2EXCAMIM Control Room</h1>");
    html.push_str("<p class=\"muted\">Static local view over canonical events, paper ledger projection, readiness, policy, and latest pipeline output when available.</p>");

    html.push_str("<section><h2>Pipeline Summary</h2><div class=\"grid\">");
    metric(
        &mut html,
        "last pipeline run",
        if pipeline.is_some() {
            "available"
        } else {
            "missing"
        },
    );
    metric(
        &mut html,
        "signals_seen",
        &pipeline
            .map(|report| report.signals_seen.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "signals_confirmed",
        &pipeline
            .map(|report| report.signals_confirmed.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "execution_requests_sent",
        &pipeline
            .map(|report| report.execution_requests_sent.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "fills_persisted",
        &pipeline
            .map(|report| report.fills_persisted.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "blocked_by_risk",
        &pipeline
            .map(|report| report.blocked_by_risk.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    html.push_str("</div></section>");

    html.push_str("<section><h2>Paper Trading State</h2><div class=\"grid\">");
    metric(
        &mut html,
        "open_positions",
        &data.ledger.summary.open_positions.to_string(),
    );
    metric(
        &mut html,
        "closed_positions",
        &data.ledger.summary.closed_positions.to_string(),
    );
    metric(
        &mut html,
        "total_notional_spent",
        &format!("{:.8}", data.ledger.summary.total_notional_spent),
    );
    metric(
        &mut html,
        "total_notional_received",
        &format!("{:.8}", data.ledger.summary.total_notional_received),
    );
    metric(
        &mut html,
        "realized_pnl_total",
        &format!("{:.8}", data.ledger.summary.realized_pnl_total),
    );
    metric(
        &mut html,
        "unrealized_pnl_total",
        &data
            .ledger
            .summary
            .unrealized_pnl_total
            .map(|value| format!("{value:.8}"))
            .unwrap_or_else(|| "n/a".into()),
    );
    html.push_str("</div><h3>Exposure by Market</h3>");
    exposure_table(&mut html, &data.ledger.summary.exposure_by_market);
    html.push_str("<h3>Exposure by Outcome</h3>");
    exposure_table(&mut html, &data.ledger.summary.exposure_by_outcome);
    html.push_str("</section>");

    html.push_str("<section><h2>Governance / Readiness</h2><div class=\"grid\">");
    metric(
        &mut html,
        "experimental",
        &readiness
            .map(|report| report.summary.experimental.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "candidate",
        &readiness
            .map(|report| report.summary.candidate.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "promoted",
        &readiness
            .map(|report| report.summary.promoted.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "frozen",
        &readiness
            .map(|report| report.summary.frozen.to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "active policy rules",
        &policy
            .map(|policy| policy.rules.len().to_string())
            .unwrap_or_else(|| "n/a".into()),
    );
    html.push_str("</div><h3>Promoted Families</h3>");
    string_list(&mut html, &promoted);
    html.push_str("<h3>Frozen Families</h3>");
    string_list(&mut html, &frozen);
    html.push_str("</section>");

    html.push_str("<section><h2>Recent Activity</h2><h3>Recent Orders</h3>");
    orders_table(&mut html, &data.ledger.orders);
    html.push_str("<h3>Recent Fills</h3>");
    fills_table(&mut html, &data.ledger.fills);
    html.push_str("<h3>Recent Risk Blocks</h3>");
    string_list(&mut html, &data.recent_risk_blocks);
    html.push_str("</section>");

    html.push_str("</main></body></html>");
    html
}

fn metric(html: &mut String, label: &str, value: &str) {
    html.push_str("<div class=\"metric\"><div class=\"label\">");
    html.push_str(&escape_html(label));
    html.push_str("</div><div class=\"value\">");
    html.push_str(&escape_html(value));
    html.push_str("</div></div>");
}

fn exposure_table(html: &mut String, rows: &[crate::PaperExposureView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"muted\">none</p>");
        return;
    }
    html.push_str("<table><thead><tr><th>Key</th><th>Net shares</th><th>Spent</th><th>Received</th></tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&row.key));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.8}", row.net_shares));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.8}", row.notional_spent));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.8}", row.notional_received));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
}

fn orders_table(html: &mut String, rows: &[crate::PaperOrderView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"muted\">none</p>");
        return;
    }
    html.push_str("<table><thead><tr><th>Order</th><th>Instrument</th><th>Status</th><th>Fills</th></tr></thead><tbody>");
    for row in rows.iter().rev().take(10) {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&row.order_id));
        html.push_str("</td><td>");
        html.push_str(&escape_html(&row.instrument));
        html.push_str("</td><td>");
        html.push_str(match row.status {
            PaperOrderStatus::Registered => "registered",
            PaperOrderStatus::Submitted => "submitted",
            PaperOrderStatus::Filled => "filled",
        });
        html.push_str("</td><td>");
        html.push_str(&row.fill_count.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
}

fn fills_table(html: &mut String, rows: &[crate::PaperFillView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"muted\">none</p>");
        return;
    }
    html.push_str("<table><thead><tr><th>Fill</th><th>Instrument</th><th>Outcome</th><th>Side</th><th>Quantity</th><th>Price</th></tr></thead><tbody>");
    for row in rows.iter().rev().take(10) {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&row.fill_id));
        html.push_str("</td><td>");
        html.push_str(&escape_html(&row.instrument));
        html.push_str("</td><td>");
        html.push_str(&escape_html(&row.outcome));
        html.push_str("</td><td>");
        html.push_str(&format!("{:?}", row.side));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.8}", row.quantity));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.8}", row.price));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
}

fn string_list(html: &mut String, values: &[String]) {
    if values.is_empty() {
        html.push_str("<p class=\"muted\">none</p>");
        return;
    }
    html.push_str("<ul>");
    for value in values {
        html.push_str("<li>");
        html.push_str(&escape_html(value));
        html.push_str("</li>");
    }
    html.push_str("</ul>");
}

fn read_optional_pipeline_report(
    path: &Path,
) -> Result<Option<crate::runtime::PaperPipelineReport>, DashboardError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    if let Ok(report) = serde_json::from_slice::<crate::runtime::PaperPipelineReport>(&bytes) {
        return Ok(Some(report));
    }
    let wrapped = serde_json::from_slice::<PipelineReportWrapper>(&bytes)?;
    Ok(Some(wrapped.report))
}

fn read_optional_readiness_report(
    path: &Path,
) -> Result<Option<ConfirmationReadinessReport>, DashboardError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_confirmation_readiness_report(path)?))
}

fn read_optional_policy(path: Option<&Path>) -> Result<Option<ConfirmationPolicy>, DashboardError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(ConfirmationPolicy::from_file(path)?))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Debug, Deserialize)]
struct PipelineReportWrapper {
    report: crate::runtime::PaperPipelineReport,
}

#[derive(Debug)]
pub enum DashboardError {
    Store(StoreError),
    Codec(crate::CodecError),
    Policy(ConfirmationPolicyLoadError),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for DashboardError {
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

impl std::error::Error for DashboardError {}

impl From<StoreError> for DashboardError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<crate::CodecError> for DashboardError {
    fn from(value: crate::CodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<ConfirmationPolicyLoadError> for DashboardError {
    fn from(value: ConfirmationPolicyLoadError) -> Self {
        Self::Policy(value)
    }
}

impl From<std::io::Error> for DashboardError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for DashboardError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{load_dashboard_data, render_dashboard_html, DashboardConfig};
    use crate::{
        EventEnvelope, FillReceived, FillSide, JsonlEventStore, Linkage, PaperLedgerProjection,
        PaperLedgerSummary, Provenance, SourceKind, StoredEvent,
    };

    fn temp_path(name: &str, suffix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("twoexcamim-dashboard-{name}-{nanos}.{suffix}"))
    }

    fn config(store_path: std::path::PathBuf) -> DashboardConfig {
        DashboardConfig {
            store_path,
            readiness_path: temp_path("missing-readiness", "json"),
            policy_path: None,
            pipeline_report_path: temp_path("missing-pipeline", "json"),
            output_path: temp_path("dashboard", "html"),
        }
    }

    #[test]
    fn data_loading_uses_canonical_events_and_tolerates_missing_optional_artifacts() {
        let store_path = temp_path("store", "jsonl");
        let store = JsonlEventStore::new(&store_path).unwrap();
        let event = EventEnvelope::new_fill_received(
            "test",
            Some("market-1".into()),
            Linkage::default(),
            Provenance {
                source_kind: SourceKind::Runtime,
                source_ref: None,
                producer_run_id: None,
                actor: None,
                trace_id: None,
                notes: None,
            },
            FillReceived {
                fill_id: "fill-1".into(),
                decision_id: None,
                order_id: "pm-paper-order-yes-sig-1".into(),
                instrument: "market-1".into(),
                side: FillSide::Buy,
                quantity: 10.0,
                price: 0.5,
                venue: crate::POLYMARKET_PAPER_VENUE.into(),
                executed_at: chrono::Utc::now(),
            },
        )
        .unwrap();
        store
            .append_event(&StoredEvent::try_from(event).unwrap())
            .unwrap();

        let data = load_dashboard_data(&config(store_path.clone())).unwrap();

        assert_eq!(data.ledger.summary.total_fills, 1);
        assert!(data.readiness.is_none());
        assert!(data.policy.is_none());
        assert!(data.pipeline.is_none());
        let _ = std::fs::remove_file(store_path);
    }

    #[test]
    fn rendering_contains_core_sections() {
        let data = super::DashboardData {
            pipeline: None,
            ledger: PaperLedgerProjection {
                summary: PaperLedgerSummary {
                    total_orders: 0,
                    total_fills: 0,
                    open_positions: 0,
                    closed_positions: 0,
                    total_notional_spent: 0.0,
                    total_notional_received: 0.0,
                    realized_pnl_total: 0.0,
                    unrealized_pnl_total: Some(0.0),
                    exposure_by_market: Vec::new(),
                    exposure_by_outcome: Vec::new(),
                    backend_accounts_seen: Vec::new(),
                },
                orders: Vec::new(),
                fills: Vec::new(),
                open_positions: Vec::new(),
                closed_positions: Vec::new(),
            },
            readiness: None,
            policy: None,
            recent_risk_blocks: Vec::new(),
        };

        let html = render_dashboard_html(&data);

        assert!(html.contains("Pipeline Summary"));
        assert!(html.contains("Paper Trading State"));
        assert!(html.contains("Governance / Readiness"));
        assert!(html.contains("Recent Activity"));
        assert!(html.contains("last pipeline run"));
    }
}
