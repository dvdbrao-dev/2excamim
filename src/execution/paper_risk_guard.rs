use serde::{Deserialize, Serialize};

use crate::{PaperExecutionRequest, PaperLedgerProjection};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperRiskGuardConfig {
    pub max_open_positions: Option<usize>,
    pub max_total_notional_exposure: Option<f64>,
    pub max_exposure_per_market: Option<f64>,
    pub max_orders_per_market: Option<usize>,
    pub block_same_market_same_outcome_if_open: bool,
}

impl Default for PaperRiskGuardConfig {
    fn default() -> Self {
        Self {
            max_open_positions: None,
            max_total_notional_exposure: None,
            max_exposure_per_market: None,
            max_orders_per_market: None,
            block_same_market_same_outcome_if_open: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperRiskGuardDecision {
    Allowed,
    BlockedMaxOpenPositions,
    BlockedMaxTotalExposure,
    BlockedMarketExposure,
    BlockedDuplicateMarketOutcome,
    BlockedMarketOrderLimit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperRiskGuardOutcome {
    pub decision: PaperRiskGuardDecision,
    pub reason: Option<String>,
}

pub fn evaluate_paper_risk(
    ledger: &PaperLedgerProjection,
    request: &PaperExecutionRequest,
    config: &PaperRiskGuardConfig,
) -> PaperRiskGuardOutcome {
    let projected_market_exposure =
        market_exposure(ledger, &request.market_slug) + request.amount_usd;
    let projected_total_exposure = total_open_exposure(ledger) + request.amount_usd;
    let existing_market_orders = ledger
        .orders
        .iter()
        .filter(|order| order.instrument == request.market_slug)
        .count();

    if let Some(limit) = config.max_open_positions {
        let opens_new_position = !ledger.open_positions.iter().any(|position| {
            position.instrument == request.market_slug && position.outcome == request.outcome
        });
        if opens_new_position && ledger.open_positions.len() >= limit {
            return blocked(
                PaperRiskGuardDecision::BlockedMaxOpenPositions,
                format!(
                    "open_positions={} max_open_positions={limit}",
                    ledger.open_positions.len()
                ),
            );
        }
    }

    if let Some(limit) = config.max_total_notional_exposure {
        if projected_total_exposure > limit {
            return blocked(
                PaperRiskGuardDecision::BlockedMaxTotalExposure,
                format!(
                    "projected_total_notional_exposure={projected_total_exposure:.8} max_total_notional_exposure={limit:.8}"
                ),
            );
        }
    }

    if let Some(limit) = config.max_exposure_per_market {
        if projected_market_exposure > limit {
            return blocked(
                PaperRiskGuardDecision::BlockedMarketExposure,
                format!(
                    "projected_market_exposure={projected_market_exposure:.8} max_exposure_per_market={limit:.8}"
                ),
            );
        }
    }

    if config.block_same_market_same_outcome_if_open
        && ledger.open_positions.iter().any(|position| {
            position.instrument == request.market_slug && position.outcome == request.outcome
        })
    {
        return blocked(
            PaperRiskGuardDecision::BlockedDuplicateMarketOutcome,
            format!(
                "open position already exists for market={} outcome={}",
                request.market_slug, request.outcome
            ),
        );
    }

    if let Some(limit) = config.max_orders_per_market {
        if existing_market_orders >= limit {
            return blocked(
                PaperRiskGuardDecision::BlockedMarketOrderLimit,
                format!("orders_for_market={existing_market_orders} max_orders_per_market={limit}"),
            );
        }
    }

    PaperRiskGuardOutcome {
        decision: PaperRiskGuardDecision::Allowed,
        reason: None,
    }
}

fn blocked(decision: PaperRiskGuardDecision, reason: String) -> PaperRiskGuardOutcome {
    PaperRiskGuardOutcome {
        decision,
        reason: Some(reason),
    }
}

fn total_open_exposure(ledger: &PaperLedgerProjection) -> f64 {
    ledger
        .open_positions
        .iter()
        .map(|position| position.notional_spent - position.notional_received)
        .sum()
}

fn market_exposure(ledger: &PaperLedgerProjection, market: &str) -> f64 {
    ledger
        .open_positions
        .iter()
        .filter(|position| position.instrument == market)
        .map(|position| position.notional_spent - position.notional_received)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{evaluate_paper_risk, PaperRiskGuardConfig, PaperRiskGuardDecision};
    use crate::{
        PaperExecutionRequest, PaperLedgerProjection, PaperLedgerSummary, PaperPositionView,
    };

    fn request() -> PaperExecutionRequest {
        PaperExecutionRequest {
            order_id: "ord-1".into(),
            decision_id: "dec-1".into(),
            market_slug: "market-1".into(),
            outcome: "yes".into(),
            side: crate::FillSide::Buy,
            amount_usd: 10.0,
            backend_account: "paper-main".into(),
            backend_data_dir: "var/paper".into(),
        }
    }

    fn ledger(open_positions: Vec<PaperPositionView>) -> PaperLedgerProjection {
        PaperLedgerProjection {
            summary: PaperLedgerSummary {
                total_orders: 0,
                total_fills: 0,
                open_positions: open_positions.len(),
                closed_positions: 0,
                total_notional_spent: 0.0,
                total_notional_received: 0.0,
                realized_pnl_total: 0.0,
                unrealized_pnl_total: None,
                exposure_by_market: Vec::new(),
                exposure_by_outcome: Vec::new(),
                backend_accounts_seen: Vec::new(),
            },
            orders: Vec::new(),
            fills: Vec::new(),
            open_positions,
            closed_positions: Vec::new(),
        }
    }

    fn position(market: &str, outcome: &str, exposure: f64) -> PaperPositionView {
        PaperPositionView {
            instrument: market.into(),
            outcome: outcome.into(),
            lifecycle: crate::PaperPositionLifecycle::Open,
            net_shares: 10.0,
            average_entry_price: Some(0.5),
            current_mark_price: None,
            notional_spent: exposure,
            notional_received: 0.0,
            realized_pnl: 0.0,
            unrealized_pnl: None,
            fill_count: 1,
        }
    }

    #[test]
    fn blocks_when_max_open_positions_is_reached() {
        let outcome = evaluate_paper_risk(
            &ledger(vec![position("other-market", "yes", 5.0)]),
            &request(),
            &PaperRiskGuardConfig {
                max_open_positions: Some(1),
                ..PaperRiskGuardConfig::default()
            },
        );

        assert_eq!(
            outcome.decision,
            PaperRiskGuardDecision::BlockedMaxOpenPositions
        );
    }

    #[test]
    fn blocks_when_market_exposure_is_exceeded() {
        let outcome = evaluate_paper_risk(
            &ledger(vec![position("market-1", "no", 15.0)]),
            &request(),
            &PaperRiskGuardConfig {
                max_exposure_per_market: Some(20.0),
                ..PaperRiskGuardConfig::default()
            },
        );

        assert_eq!(
            outcome.decision,
            PaperRiskGuardDecision::BlockedMarketExposure
        );
    }

    #[test]
    fn blocks_duplicate_same_market_same_outcome() {
        let outcome = evaluate_paper_risk(
            &ledger(vec![position("market-1", "yes", 5.0)]),
            &request(),
            &PaperRiskGuardConfig {
                block_same_market_same_outcome_if_open: true,
                ..PaperRiskGuardConfig::default()
            },
        );

        assert_eq!(
            outcome.decision,
            PaperRiskGuardDecision::BlockedDuplicateMarketOutcome
        );
    }

    #[test]
    fn allows_valid_request() {
        let outcome = evaluate_paper_risk(
            &ledger(vec![position("other-market", "yes", 5.0)]),
            &request(),
            &PaperRiskGuardConfig {
                max_open_positions: Some(2),
                max_total_notional_exposure: Some(50.0),
                max_exposure_per_market: Some(20.0),
                max_orders_per_market: Some(2),
                block_same_market_same_outcome_if_open: true,
            },
        );

        assert_eq!(outcome.decision, PaperRiskGuardDecision::Allowed);
    }
}
