use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    codecs::{CodecError, RehydratedEvent},
    events::{EventTyped, FillSide},
    store::StoredEvent,
    POLYMARKET_PAPER_VENUE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperOrderStatus {
    Registered,
    Submitted,
    Filled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperPositionLifecycle {
    Open,
    PartiallyClosed,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperOrderView {
    pub order_id: String,
    pub decision_id: Option<String>,
    pub signal_id: Option<String>,
    pub instrument: String,
    pub venue: String,
    pub status: PaperOrderStatus,
    pub registered_at: Option<DateTime<Utc>>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub fill_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperFillView {
    pub fill_id: String,
    pub order_id: String,
    pub decision_id: Option<String>,
    pub signal_id: Option<String>,
    pub instrument: String,
    pub outcome: String,
    pub side: FillSide,
    pub quantity: f64,
    pub price: f64,
    pub notional: f64,
    pub venue: String,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperPositionView {
    pub instrument: String,
    pub outcome: String,
    pub lifecycle: PaperPositionLifecycle,
    pub net_shares: f64,
    pub average_entry_price: Option<f64>,
    pub current_mark_price: Option<f64>,
    pub notional_spent: f64,
    pub notional_received: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: Option<f64>,
    pub fill_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperExposureView {
    pub key: String,
    pub net_shares: f64,
    pub notional_spent: f64,
    pub notional_received: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperLedgerSummary {
    pub total_orders: usize,
    pub total_fills: usize,
    pub open_positions: usize,
    pub closed_positions: usize,
    pub total_notional_spent: f64,
    pub total_notional_received: f64,
    pub realized_pnl_total: f64,
    pub unrealized_pnl_total: Option<f64>,
    pub exposure_by_market: Vec<PaperExposureView>,
    pub exposure_by_outcome: Vec<PaperExposureView>,
    pub backend_accounts_seen: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperLedgerProjection {
    pub summary: PaperLedgerSummary,
    pub orders: Vec<PaperOrderView>,
    pub fills: Vec<PaperFillView>,
    pub open_positions: Vec<PaperPositionView>,
    pub closed_positions: Vec<PaperPositionView>,
}

pub fn project_paper_ledger(events: &[StoredEvent]) -> Result<PaperLedgerProjection, CodecError> {
    let mut orders = BTreeMap::<String, PaperOrderView>::new();
    let mut fills = BTreeMap::<String, PaperFillView>::new();
    let mut backend_accounts_seen = BTreeSet::new();

    for stored in events {
        match RehydratedEvent::try_from(stored)? {
            RehydratedEvent::OrderRegistered(event)
                if event.payload.venue == POLYMARKET_PAPER_VENUE =>
            {
                let order = orders
                    .entry(event.payload.order_id.clone())
                    .or_insert_with(|| PaperOrderView {
                        order_id: event.payload.order_id.clone(),
                        decision_id: event
                            .payload
                            .decision_id
                            .clone()
                            .or_else(|| event.linkage.decision_id.clone()),
                        signal_id: event.linkage.signal_id.clone(),
                        instrument: event.payload.instrument.clone(),
                        venue: event.payload.venue.clone(),
                        status: PaperOrderStatus::Registered,
                        registered_at: Some(event.occurred_at),
                        submitted_at: None,
                        fill_count: 0,
                    });
                order.registered_at = Some(
                    order
                        .registered_at
                        .map(|current| current.min(event.occurred_at))
                        .unwrap_or(event.occurred_at),
                );
            }
            RehydratedEvent::OrderSubmitted(event)
                if event.payload.venue == POLYMARKET_PAPER_VENUE =>
            {
                let order = orders
                    .entry(event.payload.order_id.clone())
                    .or_insert_with(|| PaperOrderView {
                        order_id: event.payload.order_id.clone(),
                        decision_id: event
                            .payload
                            .decision_id
                            .clone()
                            .or_else(|| event.linkage.decision_id.clone()),
                        signal_id: event.linkage.signal_id.clone(),
                        instrument: event.payload.instrument.clone(),
                        venue: event.payload.venue.clone(),
                        status: PaperOrderStatus::Submitted,
                        registered_at: None,
                        submitted_at: Some(event.occurred_at),
                        fill_count: 0,
                    });
                order.status = PaperOrderStatus::Submitted;
                order.submitted_at = Some(
                    order
                        .submitted_at
                        .map(|current| current.max(event.occurred_at))
                        .unwrap_or(event.occurred_at),
                );
                order.decision_id = order.decision_id.clone().or(event.payload.decision_id);
                order.signal_id = order.signal_id.clone().or(event.linkage.signal_id);
            }
            RehydratedEvent::FillReceived(event)
                if event.payload.venue == POLYMARKET_PAPER_VENUE =>
            {
                let fill_key = event.payload.idempotency_key();
                if fills.contains_key(&fill_key) {
                    continue;
                }
                let outcome =
                    outcome_for_fill(&event.payload.order_id, event.linkage.signal_id.as_deref());
                backend_accounts_seen.extend(backend_account_from_fill_id(&event.payload.fill_id));
                fills.insert(
                    fill_key,
                    PaperFillView {
                        fill_id: event.payload.fill_id.clone(),
                        order_id: event.payload.order_id.clone(),
                        decision_id: event
                            .payload
                            .decision_id
                            .clone()
                            .or_else(|| event.linkage.decision_id.clone()),
                        signal_id: event.linkage.signal_id.clone(),
                        instrument: event.payload.instrument.clone(),
                        outcome,
                        side: event.payload.side,
                        quantity: event.payload.quantity,
                        price: event.payload.price,
                        notional: event.payload.quantity * event.payload.price,
                        venue: event.payload.venue.clone(),
                        executed_at: event.payload.executed_at,
                    },
                );
            }
            _ => {}
        }
    }

    for fill in fills.values() {
        let order = orders
            .entry(fill.order_id.clone())
            .or_insert_with(|| PaperOrderView {
                order_id: fill.order_id.clone(),
                decision_id: fill.decision_id.clone(),
                signal_id: fill.signal_id.clone(),
                instrument: fill.instrument.clone(),
                venue: fill.venue.clone(),
                status: PaperOrderStatus::Filled,
                registered_at: None,
                submitted_at: None,
                fill_count: 0,
            });
        order.status = PaperOrderStatus::Filled;
        order.fill_count += 1;
        order.decision_id = order.decision_id.clone().or(fill.decision_id.clone());
        order.signal_id = order.signal_id.clone().or(fill.signal_id.clone());
    }

    let mut fills = fills.into_values().collect::<Vec<_>>();
    fills.sort_by(|left, right| {
        left.executed_at
            .cmp(&right.executed_at)
            .then_with(|| left.order_id.cmp(&right.order_id))
            .then_with(|| left.fill_id.cmp(&right.fill_id))
    });
    let positions = position_views(&fills);
    let (open_positions, closed_positions): (Vec<_>, Vec<_>) = positions
        .into_iter()
        .partition(|position| position.net_shares.abs() > f64::EPSILON);
    let mut orders = orders.into_values().collect::<Vec<_>>();
    orders.sort_by(|left, right| left.order_id.cmp(&right.order_id));

    Ok(PaperLedgerProjection {
        summary: summarize(
            &orders,
            &fills,
            &open_positions,
            &closed_positions,
            &backend_accounts_seen,
        ),
        orders,
        fills,
        open_positions,
        closed_positions,
    })
}

fn position_views(fills: &[PaperFillView]) -> Vec<PaperPositionView> {
    let mut positions = BTreeMap::<(String, String), PositionAccumulator>::new();
    for fill in fills {
        let position = positions
            .entry((fill.instrument.clone(), fill.outcome.clone()))
            .or_insert_with(|| PositionAccumulator {
                instrument: fill.instrument.clone(),
                outcome: fill.outcome.clone(),
                net_shares: 0.0,
                remaining_cost_basis: 0.0,
                sold_shares: 0.0,
                notional_spent: 0.0,
                notional_received: 0.0,
                realized_pnl: 0.0,
                current_mark_price: None,
                fill_count: 0,
            });
        position.apply_fill(fill);
    }
    positions
        .into_values()
        .map(PositionAccumulator::into_view)
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct PositionAccumulator {
    instrument: String,
    outcome: String,
    net_shares: f64,
    remaining_cost_basis: f64,
    sold_shares: f64,
    notional_spent: f64,
    notional_received: f64,
    realized_pnl: f64,
    current_mark_price: Option<f64>,
    fill_count: usize,
}

impl PositionAccumulator {
    fn apply_fill(&mut self, fill: &PaperFillView) {
        self.current_mark_price = Some(fill.price);
        self.fill_count += 1;
        match fill.side {
            FillSide::Buy => {
                self.net_shares += fill.quantity;
                self.remaining_cost_basis += fill.notional;
                self.notional_spent += fill.notional;
            }
            FillSide::Sell => {
                let closed_shares = fill.quantity.min(self.net_shares.max(0.0));
                if closed_shares > 0.0 {
                    let average_cost = self.remaining_cost_basis / self.net_shares;
                    self.realized_pnl += closed_shares * (fill.price - average_cost);
                    self.remaining_cost_basis -= average_cost * closed_shares;
                }
                self.net_shares -= fill.quantity;
                if self.net_shares.abs() <= f64::EPSILON {
                    self.net_shares = 0.0;
                    self.remaining_cost_basis = 0.0;
                }
                self.sold_shares += fill.quantity;
                self.notional_received += fill.notional;
            }
        }
    }

    fn into_view(self) -> PaperPositionView {
        let average_entry_price =
            (self.net_shares > f64::EPSILON).then_some(self.remaining_cost_basis / self.net_shares);
        let unrealized_pnl = match (average_entry_price, self.current_mark_price) {
            (Some(average_entry), Some(mark)) => Some(self.net_shares * (mark - average_entry)),
            _ => None,
        };
        let lifecycle = if self.net_shares.abs() <= f64::EPSILON {
            PaperPositionLifecycle::Closed
        } else if self.sold_shares > 0.0 {
            PaperPositionLifecycle::PartiallyClosed
        } else {
            PaperPositionLifecycle::Open
        };

        PaperPositionView {
            instrument: self.instrument,
            outcome: self.outcome,
            lifecycle,
            net_shares: self.net_shares,
            average_entry_price,
            current_mark_price: self.current_mark_price,
            notional_spent: self.notional_spent,
            notional_received: self.notional_received,
            realized_pnl: self.realized_pnl,
            unrealized_pnl,
            fill_count: self.fill_count,
        }
    }
}

fn summarize(
    orders: &[PaperOrderView],
    fills: &[PaperFillView],
    open_positions: &[PaperPositionView],
    closed_positions: &[PaperPositionView],
    backend_accounts_seen: &BTreeSet<String>,
) -> PaperLedgerSummary {
    let unrealized_pnl_total = open_positions
        .iter()
        .map(|position| position.unrealized_pnl)
        .collect::<Option<Vec<_>>>()
        .map(|values| values.into_iter().sum());

    PaperLedgerSummary {
        total_orders: orders.len(),
        total_fills: fills.len(),
        open_positions: open_positions.len(),
        closed_positions: closed_positions.len(),
        total_notional_spent: fills
            .iter()
            .filter(|fill| fill.side == FillSide::Buy)
            .map(|fill| fill.notional)
            .sum(),
        total_notional_received: fills
            .iter()
            .filter(|fill| fill.side == FillSide::Sell)
            .map(|fill| fill.notional)
            .sum(),
        realized_pnl_total: open_positions
            .iter()
            .chain(closed_positions.iter())
            .map(|position| position.realized_pnl)
            .sum(),
        unrealized_pnl_total,
        exposure_by_market: exposure_by(open_positions, |position| position.instrument.clone()),
        exposure_by_outcome: exposure_by(open_positions, |position| position.outcome.clone()),
        backend_accounts_seen: backend_accounts_seen.iter().cloned().collect(),
    }
}

fn exposure_by<F>(positions: &[PaperPositionView], key_fn: F) -> Vec<PaperExposureView>
where
    F: Fn(&PaperPositionView) -> String,
{
    let mut rows = BTreeMap::<String, PaperExposureView>::new();
    for position in positions {
        let row = rows
            .entry(key_fn(position))
            .or_insert_with(|| PaperExposureView {
                key: key_fn(position),
                net_shares: 0.0,
                notional_spent: 0.0,
                notional_received: 0.0,
            });
        row.net_shares += position.net_shares;
        row.notional_spent += position.notional_spent;
        row.notional_received += position.notional_received;
    }
    rows.into_values().collect()
}

fn outcome_for_fill(order_id: &str, signal_id: Option<&str>) -> String {
    if order_id.starts_with("pm-paper-order-yes-") {
        return "yes".into();
    }
    if order_id.starts_with("pm-paper-order-no-") {
        return "no".into();
    }
    if let Some(signal_id) = signal_id {
        if order_id == format!("pm-paper-order-{signal_id}") {
            return "signal_direction".into();
        }
    }
    "unknown".into()
}

fn backend_account_from_fill_id(fill_id: &str) -> Option<String> {
    fill_id
        .strip_prefix("pm-paper-")
        .and_then(|remaining| remaining.rsplit_once('-'))
        .map(|(account, _trade_id)| account.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{project_paper_ledger, PaperOrderStatus, PaperPositionLifecycle};
    use crate::{
        events::{
            EventEnvelope, FillReceived, FillSide, Linkage, OrderRegistered, OrderSubmitted,
            Provenance, SourceKind,
        },
        store::StoredEvent,
        POLYMARKET_PAPER_VENUE,
    };

    fn provenance() -> Provenance {
        Provenance {
            source_kind: SourceKind::Runtime,
            source_ref: None,
            producer_run_id: Some("test-run".into()),
            actor: Some("test".into()),
            trace_id: None,
            notes: None,
        }
    }

    fn linkage() -> Linkage {
        Linkage {
            signal_id: Some("sig-1".into()),
            decision_id: Some("dec-1".into()),
            order_id: Some("pm-paper-order-sig-1".into()),
            correlation_id: Some("corr-1".into()),
            ..Linkage::default()
        }
    }

    fn stored<T: serde::Serialize>(event: EventEnvelope<T>) -> StoredEvent {
        StoredEvent::try_from(event).unwrap()
    }

    fn assert_close(left: f64, right: f64) {
        assert!(
            (left - right).abs() < 0.00000001,
            "expected {left} to be close to {right}"
        );
    }

    fn registered() -> StoredEvent {
        stored(
            EventEnvelope::new_order_registered(
                "test",
                Some("market-1".into()),
                linkage(),
                provenance(),
                OrderRegistered {
                    order_id: "pm-paper-order-sig-1".into(),
                    decision_id: Some("dec-1".into()),
                    instrument: "market-1".into(),
                    venue: POLYMARKET_PAPER_VENUE.into(),
                },
            )
            .unwrap(),
        )
    }

    fn submitted() -> StoredEvent {
        stored(
            EventEnvelope::new_order_submitted(
                "test",
                Some("market-1".into()),
                linkage(),
                provenance(),
                OrderSubmitted {
                    order_id: "pm-paper-order-sig-1".into(),
                    decision_id: Some("dec-1".into()),
                    instrument: "market-1".into(),
                    venue: POLYMARKET_PAPER_VENUE.into(),
                },
            )
            .unwrap(),
        )
    }

    fn fill(fill_id: &str, side: FillSide, quantity: f64, price: f64) -> StoredEvent {
        stored(
            EventEnvelope::new_fill_received(
                "test",
                Some("market-1".into()),
                linkage(),
                provenance(),
                FillReceived {
                    fill_id: fill_id.into(),
                    decision_id: Some("dec-1".into()),
                    order_id: "pm-paper-order-sig-1".into(),
                    instrument: "market-1".into(),
                    side,
                    quantity,
                    price,
                    venue: POLYMARKET_PAPER_VENUE.into(),
                    executed_at: Utc.with_ymd_and_hms(2026, 4, 14, 12, 0, 0).unwrap(),
                },
            )
            .unwrap(),
        )
    }

    #[test]
    fn projects_orders_fills_and_open_positions() {
        let projection = project_paper_ledger(&[
            registered(),
            submitted(),
            fill("pm-paper-paper-main-trade-1", FillSide::Buy, 10.0, 0.4),
        ])
        .unwrap();

        assert_eq!(projection.summary.total_orders, 1);
        assert_eq!(projection.summary.total_fills, 1);
        assert_eq!(projection.summary.open_positions, 1);
        assert_eq!(projection.summary.closed_positions, 0);
        assert_eq!(projection.summary.total_notional_spent, 4.0);
        assert_eq!(projection.summary.realized_pnl_total, 0.0);
        assert_eq!(projection.summary.unrealized_pnl_total, Some(0.0));
        assert_eq!(projection.orders[0].status, PaperOrderStatus::Filled);
        assert_eq!(
            projection.open_positions[0].lifecycle,
            PaperPositionLifecycle::Open
        );
        assert_eq!(projection.open_positions[0].net_shares, 10.0);
        assert_close(
            projection.open_positions[0].average_entry_price.unwrap(),
            0.4,
        );
        assert_eq!(projection.open_positions[0].current_mark_price, Some(0.4));
    }

    #[test]
    fn repeated_fill_event_does_not_corrupt_summary() {
        let duplicate = fill("pm-paper-paper-main-trade-1", FillSide::Buy, 10.0, 0.4);
        let projection =
            project_paper_ledger(&[registered(), submitted(), duplicate.clone(), duplicate])
                .unwrap();

        assert_eq!(projection.summary.total_fills, 1);
        assert_eq!(projection.summary.total_notional_spent, 4.0);
        assert_eq!(projection.open_positions[0].net_shares, 10.0);
    }

    #[test]
    fn realized_pnl_and_lifecycle_account_for_partial_sells() {
        let projection = project_paper_ledger(&[
            registered(),
            submitted(),
            fill("pm-paper-paper-main-trade-1", FillSide::Buy, 10.0, 0.4),
            fill("pm-paper-paper-main-trade-2", FillSide::Sell, 4.0, 0.6),
        ])
        .unwrap();

        assert_eq!(projection.summary.total_fills, 2);
        assert_eq!(projection.summary.total_notional_spent, 4.0);
        assert_eq!(projection.summary.total_notional_received, 2.4);
        assert_close(projection.summary.realized_pnl_total, 0.8);
        assert_close(projection.summary.unrealized_pnl_total.unwrap(), 1.2);
        assert_eq!(projection.open_positions[0].net_shares, 6.0);
        assert_eq!(
            projection.open_positions[0].lifecycle,
            PaperPositionLifecycle::PartiallyClosed
        );
        assert_close(
            projection.open_positions[0].average_entry_price.unwrap(),
            0.4,
        );
        assert_close(projection.open_positions[0].realized_pnl, 0.8);
        assert_close(projection.open_positions[0].unrealized_pnl.unwrap(), 1.2);
    }

    #[test]
    fn lifecycle_transitions_to_closed_when_position_is_flat() {
        let projection = project_paper_ledger(&[
            registered(),
            submitted(),
            fill("pm-paper-paper-main-trade-1", FillSide::Buy, 10.0, 0.4),
            fill("pm-paper-paper-main-trade-2", FillSide::Sell, 10.0, 0.6),
        ])
        .unwrap();

        assert_eq!(projection.summary.open_positions, 0);
        assert_eq!(projection.summary.closed_positions, 1);
        assert_close(projection.summary.realized_pnl_total, 2.0);
        assert_eq!(projection.summary.unrealized_pnl_total, Some(0.0));
        assert_eq!(projection.closed_positions[0].net_shares, 0.0);
        assert_eq!(
            projection.closed_positions[0].lifecycle,
            PaperPositionLifecycle::Closed
        );
        assert_eq!(projection.closed_positions[0].average_entry_price, None);
        assert_eq!(projection.closed_positions[0].unrealized_pnl, None);
    }
}
