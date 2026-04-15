use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
    project_paper_ledger, read_confirmation_readiness_report, ConfirmationPolicy,
    ConfirmationPolicyLoadError, ConfirmationReadinessReport, PaperLedgerProjection,
    PaperOrderStatus, PaperPositionLifecycle, SignalPolicyStatus, StoreError,
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
    pub summary: Option<crate::OperationalSummary>,
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
    let summary =
        read_optional_operational_summary_report(&operational_summary_path(&config.store_path))?;
    let readiness = read_optional_readiness_report(&config.readiness_path)?;
    let policy = read_optional_policy(config.policy_path.as_deref())?;
    let blocked_by_risk = summary
        .as_ref()
        .and_then(|report| report.pipeline.blocked_by_risk)
        .or_else(|| pipeline.as_ref().map(|report| report.blocked_by_risk))
        .unwrap_or(0);
    let recent_risk_blocks = if blocked_by_risk > 0 {
        vec![format!(
            "latest run-paper-pipeline blocked {} request(s) with the paper risk guard",
            blocked_by_risk
        )]
    } else {
        Vec::new()
    };

    Ok(DashboardData {
        pipeline,
        summary,
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
    let summary = data.summary.as_ref();
    let readiness = data.readiness.as_ref();
    let policy = data.policy.as_ref();
    let readiness_rows = readiness.map(build_readiness_rows).unwrap_or_default();
    let policy_status_counts = policy.map(count_policy_statuses);
    let pipeline_view = build_pipeline_view(pipeline, summary);
    let governance_rows = build_governance_rows(summary, readiness);
    let recent_orders = recent_orders(summary, &data.ledger);
    let recent_fills = recent_fills(summary, &data.ledger);

    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<title>2EXCAMIM Control Room</title>");
    html.push_str("<style>");
    html.push_str("body{font-family:system-ui,-apple-system,Segoe UI,sans-serif;margin:0;background:linear-gradient(180deg,#f5f7fb 0,#eef2f7 100%);color:#16181d}main{max-width:1280px;margin:0 auto;padding:24px}h1{margin:0 0 4px;font-size:28px;letter-spacing:-.02em}section{background:#fff;border:1px solid #dfe3ea;border-radius:12px;padding:18px;margin:18px 0;box-shadow:0 1px 2px rgba(16,24,40,.04)}h2{margin:0 0 10px;font-size:18px}.section-head{display:flex;align-items:flex-start;justify-content:space-between;gap:12px;flex-wrap:wrap}.section-copy{max-width:780px}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:10px}.metric{border:1px solid #e6e9ef;border-radius:10px;padding:10px;background:#fbfcfe}.label{font-size:12px;color:#667085;text-transform:uppercase;letter-spacing:.02em}.value{font-size:20px;font-weight:650;margin-top:4px;line-height:1.2;word-break:break-word}.small{font-size:13px}.muted{color:#667085}.note{color:#52606d;font-size:13px;line-height:1.45;margin:0}.note + .note{margin-top:6px}.stack{display:grid;gap:10px}.mini-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:8px}.pill{display:inline-flex;align-items:center;gap:6px;padding:3px 10px;border-radius:999px;font-size:12px;font-weight:650;line-height:1.2}.pill.good{background:#e8f5e9;color:#137333}.pill.warn{background:#fff4e5;color:#9a3412}.pill.neutral{background:#eef2ff;color:#3730a3}.pill.muted{background:#eef1f5;color:#4b5565}.pill.soft{background:#f4f6f8;color:#3f4a5a}.stages{list-style:none;margin:10px 0 0;padding:0;display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:8px}.stage{border:1px solid #e6e9ef;border-radius:10px;padding:10px;background:#fbfcfe}.stage-order{font-size:12px;color:#667085;font-variant-numeric:tabular-nums;margin-bottom:4px}.stage-name{font-size:14px;font-weight:600;word-break:break-word}.stage-note{margin-top:6px;color:#667085;font-size:12px}.status-breakdown{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:8px}.status-card{border:1px solid #e6e9ef;border-radius:10px;padding:10px;background:#fbfcfe}.status-card h3{margin:0 0 4px;font-size:15px}.status-count{font-size:26px;font-weight:700;line-height:1.1;margin:0 0 4px}.status-families{font-size:13px;line-height:1.45;color:#52606d;word-break:break-word}table{border-collapse:collapse;width:100%;font-size:14px}th,td{text-align:left;border-bottom:1px solid #eceff4;padding:8px;vertical-align:top}th{color:#4b5565;background:#fafbfc}.table-wrap{overflow-x:auto;border:1px solid #e6e9ef;border-radius:10px}.table-wrap table{border:0}.table-wrap th,.table-wrap td{border-bottom:1px solid #edf1f7}.table-wrap tr:last-child td{border-bottom:0}.table-wrap td.nowrap,.table-wrap th.nowrap{white-space:nowrap}ul{margin:8px 0 0 18px;padding:0}.empty{padding:10px 0;color:#667085}.positive{color:#137333}.negative{color:#9a3412}.definition{display:grid;grid-template-columns:auto 1fr;gap:4px 10px;font-size:13px;color:#52606d}.definition dt{font-weight:650;color:#324154}.definition dd{margin:0}");
    html.push_str("</style></head><body><main>");
    html.push_str("<h1>2EXCAMIM Control Room</h1>");
    html.push_str("<p class=\"muted\">Static local view over canonical events, paper ledger projection, readiness, policy, and the latest paper pipeline run when available.</p>");

    html.push_str("<section><div class=\"section-head\"><div class=\"section-copy\"><h2>Pipeline Recap</h2><p class=\"note\">Latest <code>run-paper-pipeline</code> output, stage order, and the counters that matter for the last paper pass.</p></div>");
    html.push_str("<div class=\"pill ");
    html.push_str(if pipeline_view.report_available {
        "good"
    } else {
        "warn"
    });
    html.push_str("\">");
    html.push_str(if pipeline_view.report_available {
        "pipeline available"
    } else {
        "pipeline missing"
    });
    html.push_str("</div></div>");
    html.push_str("<div class=\"grid\">");
    metric(
        &mut html,
        "Dry run",
        pipeline_view.dry_run.map(yes_no).unwrap_or("n/a"),
    );
    metric(
        &mut html,
        "Signals seen",
        &display_option_usize(pipeline_view.signals_seen),
    );
    metric(
        &mut html,
        "Signals generated",
        &display_option_usize(pipeline_view.signals_generated),
    );
    metric(
        &mut html,
        "Signals confirmed",
        &display_option_usize(pipeline_view.signals_confirmed),
    );
    metric(
        &mut html,
        "Execution requests",
        &display_option_usize(pipeline_view.execution_requests_sent),
    );
    metric(
        &mut html,
        "Fills persisted",
        &display_option_usize(pipeline_view.fills_persisted),
    );
    metric(
        &mut html,
        "Blocked by risk",
        &display_option_usize(pipeline_view.blocked_by_risk),
    );
    metric(
        &mut html,
        "Open positions",
        &display_option_usize(pipeline_view.open_positions),
    );
    metric(
        &mut html,
        "Readiness states",
        &display_option_usize(pipeline_view.readiness_states_materialized),
    );
    metric(
        &mut html,
        "Operational summary",
        if summary.is_some() {
            "available"
        } else {
            "missing"
        },
    );
    html.push_str("</div>");
    if summary.is_some() {
        html.push_str("<p class=\"note\">Latest operational summary artifact is available beside the event store.</p>");
    }
    if let Some(report) = pipeline {
        html.push_str("<h3>Stage Ordering</h3><ol class=\"stages\">");
        for (index, stage) in report.stages.iter().enumerate() {
            html.push_str("<li class=\"stage\"><div class=\"stage-order\">step ");
            html.push_str(&(index + 1).to_string());
            html.push_str("</div><div class=\"stage-name\">");
            html.push_str(&escape_html(stage));
            html.push_str("</div></li>");
        }
        html.push_str("</ol>");
    } else {
        html.push_str("<p class=\"empty\">No pipeline artifact found yet. The dashboard still renders the canonical ledger and governance views.</p>");
    }
    html.push_str("</section>");

    html.push_str("<section><div class=\"section-head\"><div class=\"section-copy\"><h2>Positions</h2><p class=\"note\">Canonical paper ledger projection with a focused open-position table and a compact closed-position summary.</p></div><div class=\"pill neutral\">ledger projection</div></div><div class=\"grid\">");
    metric(
        &mut html,
        "Open positions",
        &data.ledger.summary.open_positions.to_string(),
    );
    metric(
        &mut html,
        "Closed positions",
        &data.ledger.summary.closed_positions.to_string(),
    );
    metric(
        &mut html,
        "Realized PnL total",
        &format!("{:.8}", data.ledger.summary.realized_pnl_total),
    );
    metric(
        &mut html,
        "Unrealized PnL total",
        &data
            .ledger
            .summary
            .unrealized_pnl_total
            .map(|value| format!("{value:.8}"))
            .unwrap_or_else(|| "n/a".into()),
    );
    metric(
        &mut html,
        "Total notional spent",
        &format!("{:.8}", data.ledger.summary.total_notional_spent),
    );
    metric(
        &mut html,
        "Total notional received",
        &format!("{:.8}", data.ledger.summary.total_notional_received),
    );
    html.push_str("</div><h3>Open Positions</h3>");
    html.push_str("<p class=\"note\">Open and partially closed positions, sorted by canonical instrument and outcome.</p>");
    open_positions_table(&mut html, &data.ledger.open_positions);
    html.push_str("<h3>Closed Positions Summary</h3>");
    html.push_str("<p class=\"note\">Flat positions with realized PnL and cash flow totals.</p>");
    closed_positions_table(&mut html, &data.ledger.closed_positions);
    html.push_str("<h3>Exposure by Market</h3>");
    exposure_table(&mut html, &data.ledger.summary.exposure_by_market);
    html.push_str("<h3>Exposure by Outcome</h3>");
    exposure_table(&mut html, &data.ledger.summary.exposure_by_outcome);
    html.push_str("</section>");

    html.push_str("<section><div class=\"section-head\"><div class=\"section-copy\"><h2>Recent Activity</h2><p class=\"note\">Latest orders, fills, and any risk blocks captured by the latest paper run.</p></div>");
    html.push_str("<div class=\"pill muted\">event log</div></div>");
    html.push_str("<h3>Recent Orders</h3>");
    orders_table(&mut html, &recent_orders);
    html.push_str("<h3>Recent Fills</h3>");
    fills_table(&mut html, &recent_fills);
    html.push_str("<h3>Recent Risk Blocks</h3>");
    if data.recent_risk_blocks.is_empty() {
        html.push_str("<p class=\"empty\">none</p>");
    } else {
        string_list(&mut html, &data.recent_risk_blocks);
    }
    html.push_str("</section>");

    html.push_str("<section><div class=\"section-head\"><div class=\"section-copy\"><h2>Governance</h2><p class=\"note\">Readiness breakdown and active confirmation policy summary when present.</p></div>");
    html.push_str("<div class=\"pill ");
    html.push_str(if governance_rows.is_some() || readiness.is_some() {
        "good"
    } else {
        "muted"
    });
    html.push_str("\">");
    html.push_str(if governance_rows.is_some() || readiness.is_some() {
        "governance available"
    } else {
        "governance missing"
    });
    html.push_str("</div></div>");
    if let Some(rows) = governance_rows.as_ref() {
        html.push_str("<div class=\"status-breakdown\">");
        for row in rows {
            html.push_str("<div class=\"status-card\"><h3>");
            html.push_str(&escape_html(&row.label));
            html.push_str("</h3><div class=\"status-count\">");
            html.push_str(&row.count.to_string());
            html.push_str("</div><div class=\"status-families\">");
            html.push_str(&display_families(&row.families));
            html.push_str("</div></div>");
        }
        html.push_str("</div>");
    } else {
        html.push_str("<p class=\"empty\">No readiness artifact found yet.</p>");
    }
    if readiness.is_some() {
        html.push_str("<h3>Readiness Breakdown</h3>");
        html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Status</th><th class=\"nowrap\">Count</th><th>Families</th></tr></thead><tbody>");
        for row in readiness_rows {
            html.push_str("<tr><td>");
            html.push_str(&status_badge(&row.status));
            html.push_str("</td><td class=\"nowrap\">");
            html.push_str(&row.count.to_string());
            html.push_str("</td><td>");
            html.push_str(&display_families(&row.families));
            html.push_str("</td></tr>");
        }
        html.push_str("</tbody></table></div>");
    }
    html.push_str("<h3>Policy Summary</h3>");
    if let Some(policy) = policy {
        let counts = policy_status_counts.unwrap_or(PolicyStatusCounts {
            promoted: 0,
            review: 0,
            frozen: 0,
        });
        html.push_str("<div class=\"mini-grid\">");
        metric(
            &mut html,
            "Active policy rules",
            &policy.rules.len().to_string(),
        );
        metric(&mut html, "Promoted rules", &counts.promoted.to_string());
        metric(&mut html, "Review rules", &counts.review.to_string());
        metric(&mut html, "Frozen rules", &counts.frozen.to_string());
        html.push_str("</div>");
        if let Some(metadata) = &policy.metadata {
            html.push_str("<dl class=\"definition\"><dt>Generated at</dt><dd>");
            html.push_str(&escape_html(&metadata.generated_at.to_rfc3339()));
            html.push_str("</dd><dt>Eras analyzed</dt><dd>");
            html.push_str(&metadata.eras_analyzed.to_string());
            html.push_str("</dd><dt>Selection criteria</dt><dd>");
            html.push_str(&escape_html(&metadata.selection_criteria_summary));
            html.push_str("</dd><dt>Source analysis</dt><dd>");
            html.push_str(&escape_html(&metadata.source_analysis));
            html.push_str("</dd></dl>");
        } else {
            html.push_str("<p class=\"empty\">Active policy file supplied, but no metadata block was present.</p>");
        }
    } else {
        html.push_str("<p class=\"empty\">No active confirmation policy file was supplied.</p>");
    }
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
        html.push_str("<p class=\"empty\">none</p>");
        return;
    }
    html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Key</th><th>Net shares</th><th>Spent</th><th>Received</th></tr></thead><tbody>");
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
    html.push_str("</tbody></table></div>");
}

fn open_positions_table(html: &mut String, rows: &[crate::PaperPositionView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"empty\">none</p>");
        return;
    }
    html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Position</th><th>Status</th><th class=\"nowrap\">Net shares</th><th class=\"nowrap\">Avg entry</th><th class=\"nowrap\">Mark</th><th class=\"nowrap\">Realized PnL</th><th class=\"nowrap\">Unrealized PnL</th><th class=\"nowrap\">Fills</th></tr></thead><tbody>");
    for row in rows.iter().take(10) {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&format!("{}/{}", row.instrument, row.outcome)));
        html.push_str("</td><td><span class=\"pill ");
        html.push_str(position_lifecycle_class(row.lifecycle));
        html.push_str("\">");
        html.push_str(position_lifecycle_label(row.lifecycle));
        html.push_str("</span></td><td class=\"nowrap\">");
        html.push_str(&format!("{:.8}", row.net_shares));
        html.push_str("</td><td class=\"nowrap\">");
        html.push_str(
            &row.average_entry_price
                .map(|value| format!("{value:.8}"))
                .unwrap_or_else(|| "n/a".into()),
        );
        html.push_str("</td><td class=\"nowrap\">");
        html.push_str(
            &row.current_mark_price
                .map(|value| format!("{value:.8}"))
                .unwrap_or_else(|| "n/a".into()),
        );
        html.push_str("</td><td class=\"nowrap ");
        html.push_str(value_class(row.realized_pnl));
        html.push_str("\">");
        html.push_str(&format!("{:.8}", row.realized_pnl));
        html.push_str("</td><td class=\"nowrap ");
        html.push_str(value_class(row.unrealized_pnl.unwrap_or(0.0)));
        html.push_str("\">");
        html.push_str(
            &row.unrealized_pnl
                .map(|value| format!("{value:.8}"))
                .unwrap_or_else(|| "n/a".into()),
        );
        html.push_str("</td><td class=\"nowrap\">");
        html.push_str(&row.fill_count.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></div>");
}

fn closed_positions_table(html: &mut String, rows: &[crate::PaperPositionView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"empty\">none</p>");
        return;
    }
    html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Position</th><th class=\"nowrap\">Realized PnL</th><th class=\"nowrap\">Spent</th><th class=\"nowrap\">Received</th><th class=\"nowrap\">Fills</th></tr></thead><tbody>");
    for row in rows.iter().take(10) {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&format!("{}/{}", row.instrument, row.outcome)));
        html.push_str("</td><td class=\"nowrap ");
        html.push_str(value_class(row.realized_pnl));
        html.push_str("\">");
        html.push_str(&format!("{:.8}", row.realized_pnl));
        html.push_str("</td><td class=\"nowrap\">");
        html.push_str(&format!("{:.8}", row.notional_spent));
        html.push_str("</td><td class=\"nowrap\">");
        html.push_str(&format!("{:.8}", row.notional_received));
        html.push_str("</td><td class=\"nowrap\">");
        html.push_str(&row.fill_count.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></div>");
}

fn orders_table(html: &mut String, rows: &[crate::PaperOrderView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"empty\">none</p>");
        return;
    }
    let mut rows = rows.to_vec();
    rows.sort_by(|left, right| {
        order_visible_timestamp(right)
            .cmp(&order_visible_timestamp(left))
            .then_with(|| right.order_id.cmp(&left.order_id))
    });
    html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Order</th><th>Decision</th><th>Instrument</th><th>Status</th><th>Fills</th><th>Registered</th><th>Submitted</th></tr></thead><tbody>");
    for row in rows.iter().take(10) {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&row.order_id));
        html.push_str("</td><td>");
        html.push_str(&escape_html(row.decision_id.as_deref().unwrap_or("n/a")));
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
        html.push_str("</td><td>");
        html.push_str(&format_option_datetime(row.registered_at.as_ref()));
        html.push_str("</td><td>");
        html.push_str(&format_option_datetime(row.submitted_at.as_ref()));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></div>");
}

fn fills_table(html: &mut String, rows: &[crate::PaperFillView]) {
    if rows.is_empty() {
        html.push_str("<p class=\"empty\">none</p>");
        return;
    }
    html.push_str("<div class=\"table-wrap\"><table><thead><tr><th>Fill</th><th>Order</th><th>Instrument</th><th>Outcome</th><th>Side</th><th>Quantity</th><th>Price</th><th>Notional</th><th>Executed</th></tr></thead><tbody>");
    for row in rows.iter().rev().take(10) {
        html.push_str("<tr><td>");
        html.push_str(&escape_html(&row.fill_id));
        html.push_str("</td><td>");
        html.push_str(&escape_html(&row.order_id));
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
        html.push_str("</td><td>");
        html.push_str(&format!("{:.8}", row.notional));
        html.push_str("</td><td>");
        html.push_str(&escape_html(&row.executed_at.to_rfc3339()));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></div>");
}

fn string_list(html: &mut String, values: &[String]) {
    if values.is_empty() {
        html.push_str("<p class=\"empty\">none</p>");
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

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn display_option_usize(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".into())
}

fn format_option_datetime(value: Option<&chrono::DateTime<chrono::Utc>>) -> String {
    value
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "n/a".into())
}

fn order_visible_timestamp(row: &crate::PaperOrderView) -> chrono::DateTime<chrono::Utc> {
    row.submitted_at.or(row.registered_at).unwrap_or_else(|| {
        chrono::TimeZone::timestamp_opt(&chrono::Utc, 0, 0)
            .single()
            .unwrap()
    })
}

fn position_lifecycle_label(lifecycle: PaperPositionLifecycle) -> &'static str {
    match lifecycle {
        PaperPositionLifecycle::Open => "open",
        PaperPositionLifecycle::PartiallyClosed => "partially closed",
        PaperPositionLifecycle::Closed => "closed",
    }
}

fn position_lifecycle_class(lifecycle: PaperPositionLifecycle) -> &'static str {
    match lifecycle {
        PaperPositionLifecycle::Open => "good",
        PaperPositionLifecycle::PartiallyClosed => "neutral",
        PaperPositionLifecycle::Closed => "muted",
    }
}

fn value_class(value: f64) -> &'static str {
    if value > 0.0 {
        "positive"
    } else if value < 0.0 {
        "negative"
    } else {
        "muted"
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PipelineView<'a> {
    report_available: bool,
    dry_run: Option<bool>,
    signals_seen: Option<usize>,
    signals_generated: Option<usize>,
    signals_confirmed: Option<usize>,
    execution_requests_sent: Option<usize>,
    fills_persisted: Option<usize>,
    blocked_by_risk: Option<usize>,
    open_positions: Option<usize>,
    readiness_states_materialized: Option<usize>,
    stages: Option<&'a [String]>,
}

fn build_pipeline_view<'a>(
    pipeline: Option<&'a crate::runtime::PaperPipelineReport>,
    summary: Option<&'a crate::OperationalSummary>,
) -> PipelineView<'a> {
    PipelineView {
        report_available: pipeline.is_some()
            || summary
                .map(|report| report.pipeline.report_available)
                .unwrap_or(false),
        dry_run: pipeline.map(|report| report.dry_run),
        signals_seen: summary
            .and_then(|report| report.pipeline.signals_seen)
            .or_else(|| pipeline.map(|report| report.signals_seen)),
        signals_generated: pipeline.map(|report| report.signals_generated),
        signals_confirmed: summary
            .and_then(|report| report.pipeline.signals_confirmed)
            .or_else(|| pipeline.map(|report| report.signals_confirmed)),
        execution_requests_sent: summary
            .and_then(|report| report.pipeline.execution_requests_sent)
            .or_else(|| pipeline.map(|report| report.execution_requests_sent)),
        fills_persisted: summary
            .and_then(|report| report.pipeline.fills_persisted)
            .or_else(|| pipeline.map(|report| report.fills_persisted)),
        blocked_by_risk: summary
            .and_then(|report| report.pipeline.blocked_by_risk)
            .or_else(|| pipeline.map(|report| report.blocked_by_risk)),
        open_positions: pipeline.map(|report| report.open_positions),
        readiness_states_materialized: pipeline
            .and_then(|report| report.readiness_states_materialized),
        stages: pipeline.map(|report| report.stages.as_slice()),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct GovernanceRow {
    label: String,
    count: usize,
    families: Vec<String>,
}

fn build_governance_rows(
    summary: Option<&crate::OperationalSummary>,
    readiness: Option<&ConfirmationReadinessReport>,
) -> Option<Vec<GovernanceRow>> {
    if let Some(summary) = summary {
        let experimental_families = readiness
            .map(|report| {
                readiness_families(
                    &report.states,
                    crate::ConfirmationReadinessStatus::Experimental,
                )
            })
            .unwrap_or_default();
        let candidate_families = readiness
            .map(|report| {
                readiness_families(
                    &report.states,
                    crate::ConfirmationReadinessStatus::Candidate,
                )
            })
            .unwrap_or_default();
        return Some(vec![
            GovernanceRow {
                label: "Experimental".into(),
                count: summary.governance.experimental.unwrap_or(0),
                families: experimental_families,
            },
            GovernanceRow {
                label: "Candidate".into(),
                count: summary.governance.candidate.unwrap_or(0),
                families: candidate_families,
            },
            GovernanceRow {
                label: "Promoted".into(),
                count: summary.governance.promoted.unwrap_or(0),
                families: summary.governance.promoted_families.clone(),
            },
            GovernanceRow {
                label: "Frozen".into(),
                count: summary.governance.frozen.unwrap_or(0),
                families: summary.governance.frozen_families.clone(),
            },
        ]);
    }

    readiness.map(|report| {
        vec![
            GovernanceRow {
                label: "Experimental".into(),
                count: report.summary.experimental,
                families: readiness_families(
                    &report.states,
                    crate::ConfirmationReadinessStatus::Experimental,
                ),
            },
            GovernanceRow {
                label: "Candidate".into(),
                count: report.summary.candidate,
                families: readiness_families(
                    &report.states,
                    crate::ConfirmationReadinessStatus::Candidate,
                ),
            },
            GovernanceRow {
                label: "Promoted".into(),
                count: report.summary.promoted,
                families: readiness_families(
                    &report.states,
                    crate::ConfirmationReadinessStatus::Promoted,
                ),
            },
            GovernanceRow {
                label: "Frozen".into(),
                count: report.summary.frozen,
                families: readiness_families(
                    &report.states,
                    crate::ConfirmationReadinessStatus::Frozen,
                ),
            },
        ]
    })
}

#[derive(Debug, Clone, PartialEq)]
struct ReadinessRow {
    status: String,
    count: usize,
    families: Vec<String>,
}

fn build_readiness_rows(report: &ConfirmationReadinessReport) -> Vec<ReadinessRow> {
    vec![
        ReadinessRow {
            status: "experimental".into(),
            count: report.summary.experimental,
            families: readiness_families(
                &report.states,
                crate::ConfirmationReadinessStatus::Experimental,
            ),
        },
        ReadinessRow {
            status: "candidate".into(),
            count: report.summary.candidate,
            families: readiness_families(
                &report.states,
                crate::ConfirmationReadinessStatus::Candidate,
            ),
        },
        ReadinessRow {
            status: "promoted".into(),
            count: report.summary.promoted,
            families: readiness_families(
                &report.states,
                crate::ConfirmationReadinessStatus::Promoted,
            ),
        },
        ReadinessRow {
            status: "frozen".into(),
            count: report.summary.frozen,
            families: readiness_families(
                &report.states,
                crate::ConfirmationReadinessStatus::Frozen,
            ),
        },
    ]
}

fn readiness_families(
    states: &[crate::ConfirmationReadinessState],
    target: crate::ConfirmationReadinessStatus,
) -> Vec<String> {
    states
        .iter()
        .filter(|state| state.readiness_status == target)
        .map(|state| state.signal_name.clone())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PolicyStatusCounts {
    promoted: usize,
    review: usize,
    frozen: usize,
}

fn count_policy_statuses(policy: &ConfirmationPolicy) -> PolicyStatusCounts {
    let mut counts = PolicyStatusCounts {
        promoted: 0,
        review: 0,
        frozen: 0,
    };
    for rule in &policy.rules {
        match rule.status {
            SignalPolicyStatus::Promoted => counts.promoted += 1,
            SignalPolicyStatus::Review => counts.review += 1,
            SignalPolicyStatus::Frozen => counts.frozen += 1,
        }
    }
    counts
}

fn status_badge(status: &str) -> String {
    let class = match status.to_ascii_lowercase().as_str() {
        "promoted" => "good",
        "candidate" => "neutral",
        "frozen" => "warn",
        "experimental" => "soft",
        _ => "muted",
    };
    format!(
        "<span class=\"pill {class}\">{}</span>",
        escape_html(status)
    )
}

fn display_families(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values
            .iter()
            .map(|value| format!("<span class=\"pill soft\">{}</span>", escape_html(value)))
            .collect::<Vec<_>>()
            .join(" ")
    }
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

fn read_optional_operational_summary_report(
    path: &Path,
) -> Result<Option<crate::OperationalSummary>, DashboardError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn operational_summary_path(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(|parent| parent.join("operations/latest_summary.json"))
        .unwrap_or_else(|| PathBuf::from(crate::runtime::DEFAULT_OPERATIONAL_SUMMARY_JSON_PATH))
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

fn recent_orders(
    summary: Option<&crate::OperationalSummary>,
    ledger: &PaperLedgerProjection,
) -> Vec<crate::PaperOrderView> {
    if let Some(summary) = summary {
        let recent_ids: std::collections::HashSet<_> = summary
            .recent_activity
            .orders
            .iter()
            .map(|order| order.order_id.as_str())
            .collect();
        let mut rows = ledger
            .orders
            .iter()
            .filter(|order| recent_ids.contains(order.order_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            order_visible_timestamp(right)
                .cmp(&order_visible_timestamp(left))
                .then_with(|| right.order_id.cmp(&left.order_id))
        });
        return rows;
    }

    let mut rows = ledger.orders.clone();
    rows.sort_by(|left, right| {
        order_visible_timestamp(right)
            .cmp(&order_visible_timestamp(left))
            .then_with(|| right.order_id.cmp(&left.order_id))
    });
    rows.into_iter().take(10).collect()
}

fn recent_fills(
    summary: Option<&crate::OperationalSummary>,
    ledger: &PaperLedgerProjection,
) -> Vec<crate::PaperFillView> {
    if let Some(summary) = summary {
        let recent_ids: std::collections::HashSet<_> = summary
            .recent_activity
            .fills
            .iter()
            .map(|fill| fill.fill_id.as_str())
            .collect();
        let mut rows = ledger
            .fills
            .iter()
            .filter(|fill| recent_ids.contains(fill.fill_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            right
                .executed_at
                .cmp(&left.executed_at)
                .then_with(|| left.fill_id.cmp(&right.fill_id))
        });
        return rows;
    }

    let mut rows = ledger.fills.clone();
    rows.sort_by(|left, right| {
        right
            .executed_at
            .cmp(&left.executed_at)
            .then_with(|| left.fill_id.cmp(&right.fill_id))
    });
    rows.into_iter().take(10).collect()
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
        assert!(data.summary.is_none());
        let _ = std::fs::remove_file(store_path);
    }

    #[test]
    fn rendering_contains_core_sections() {
        let data = super::DashboardData {
            pipeline: None,
            summary: None,
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

        assert!(html.contains("Pipeline Recap"));
        assert!(html.contains("Operational summary"));
        assert!(html.contains("Positions"));
        assert!(html.contains("Open Positions"));
        assert!(html.contains("Closed Positions Summary"));
        assert!(html.contains("Governance"));
        assert!(html.contains("Recent Activity"));
        assert!(html.contains("No readiness artifact found yet"));
        assert!(html.contains("No active confirmation policy file was supplied"));
        assert!(html.contains("No pipeline artifact found yet"));
    }
}
