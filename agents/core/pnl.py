from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

YES_ORDER_PREFIX = "pm-paper-order-yes-"
NO_ORDER_PREFIX = "pm-paper-order-no-"
BPS_SCALE = 10_000.0


def bps_cost(notional: float, bps: float) -> float:
    return notional * (bps / BPS_SCALE)


def max_drawdown(equity_curve: list[float]) -> float:
    if not equity_curve:
        return 0.0
    peak = equity_curve[0]
    max_dd = 0.0
    for value in equity_curve:
        if value > peak:
            peak = value
        dd = peak - value
        if dd > max_dd:
            max_dd = dd
    return max_dd


def derive_outcome(order_id: str, signal_id: str | None = None) -> str:
    if order_id.startswith(YES_ORDER_PREFIX):
        return "yes"
    if order_id.startswith(NO_ORDER_PREFIX):
        return "no"
    if signal_id and order_id == f"pm-paper-order-{signal_id}":
        return "signal_direction"
    return "unknown"


@dataclass
class FillInput:
    side: str
    quantity: float
    price: float
    order_id: str = ""
    signal_id: str | None = None


@dataclass
class TradeResult:
    pnl_gross: float
    costs: float
    pnl_net: float
    is_win: bool


@dataclass
class PnlSummary:
    pnl_gross: float = 0.0
    costs: float = 0.0
    pnl_net: float = 0.0
    trades: int = 0
    wins: int = 0
    losses: int = 0
    equity_curve: list[float] = field(default_factory=lambda: [0.0])

    @property
    def winrate(self) -> float | None:
        if self.trades == 0:
            return None
        return self.wins / self.trades

    @property
    def profit_factor(self) -> float:
        gross_wins = sum(
            r for r in self._trade_pnls_net if r > 0
        )
        gross_losses = abs(sum(r for r in self._trade_pnls_net if r < 0))
        if gross_losses == 0.0:
            return 0.0 if gross_wins == 0.0 else float("inf")
        return gross_wins / gross_losses

    @property
    def expectancy(self) -> float | None:
        if self.trades == 0:
            return None
        return self.pnl_net / self.trades

    @property
    def max_drawdown(self) -> float:
        return max_drawdown(self.equity_curve)

    _trade_pnls_net: list[float] = field(default_factory=list)


def calculate_realized_pnl_from_fills(
    fills: list[FillInput],
    fee_bps: float = 0.0,
    slippage_bps: float = 0.0,
) -> PnlSummary:
    summary = PnlSummary()
    shares: float = 0.0
    cost_basis: float = 0.0
    cumulative_net: float = 0.0

    for fill in fills:
        side = fill.side.upper()
        notional = fill.quantity * fill.price
        fill_costs = bps_cost(notional, fee_bps) + bps_cost(notional, slippage_bps)
        summary.costs += fill_costs

        if side == "BUY":
            shares += fill.quantity
            cost_basis += notional
        elif side in ("SELL", "SHORT"):
            if shares > 0:
                closed_qty = min(fill.quantity, shares)
                avg_cost = cost_basis / shares
                gross = closed_qty * (fill.price - avg_cost)
                net = gross - fill_costs

                summary.pnl_gross += gross
                summary.trades += 1
                if net > 0:
                    summary.wins += 1
                else:
                    summary.losses += 1
                summary._trade_pnls_net.append(net)

                cost_basis -= avg_cost * closed_qty
                shares -= closed_qty
                cumulative_net += net
            else:
                summary.trades += 1
                summary.losses += 1
                net = -fill_costs
                summary._trade_pnls_net.append(net)
                cumulative_net += net
        else:
            continue

        if shares <= 1e-12:
            shares = 0.0
            cost_basis = 0.0

        summary.equity_curve.append(cumulative_net)

    summary.pnl_net = cumulative_net
    return summary


def summarize_trades(summary: PnlSummary) -> dict[str, Any]:
    return {
        "pnl_gross": round(summary.pnl_gross, 8),
        "costs": round(summary.costs, 8),
        "pnl_net": round(summary.pnl_net, 8),
        "trades": summary.trades,
        "wins": summary.wins,
        "losses": summary.losses,
        "winrate": (
            None if summary.winrate is None else round(summary.winrate, 6)
        ),
        "profit_factor": (
            None
            if summary.profit_factor == float("inf")
            else round(summary.profit_factor, 6)
        ),
        "expectancy": (
            None if summary.expectancy is None else round(summary.expectancy, 8)
        ),
        "max_drawdown": round(summary.max_drawdown, 8),
    }
