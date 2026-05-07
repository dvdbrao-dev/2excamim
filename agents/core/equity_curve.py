"""Equity curve computation from fill events. Stdlib only."""
from __future__ import annotations

import math
from typing import Any, Literal, Sequence

Scope = Literal["paper", "shadow", "live"]

PAPER_PRODUCED_BY = frozenset({
    "runtime.agent.live_gateway",
    "runtime.paper_ledger",
    "paper",
})
SHADOW_AGGREGATE_PREFIX = "shadow:"


def _is_paper_fill(event: dict[str, Any]) -> bool:
    agg = event.get("aggregate_key", "")
    if isinstance(agg, str) and agg.startswith(SHADOW_AGGREGATE_PREFIX):
        return False
    return event.get("event_type") == "fill.received"


def _is_shadow_fill(event: dict[str, Any]) -> bool:
    agg = event.get("aggregate_key", "")
    if isinstance(agg, str) and agg.startswith(SHADOW_AGGREGATE_PREFIX):
        return event.get("event_type") in ("fill.received", "shadow.fill.received")
    return event.get("event_type") == "shadow.fill.received"


def _fill_pnl(event: dict[str, Any]) -> float | None:
    """Extract realized PnL change from a fill event.

    For Polymarket: PnL flows at resolution, not at fill time.
    We approximate: track cash flow (negative on buy, positive on sell).
    """
    payload = event.get("payload") or {}
    side = str(payload.get("side", "")).upper()
    try:
        price = float(payload.get("price", 0))
        quantity = float(payload.get("quantity", 0))
    except (TypeError, ValueError):
        return None

    if not (math.isfinite(price) and math.isfinite(quantity)):
        return None
    if price <= 0 or quantity <= 0:
        return None

    # BUY: cash outflow (negative impact on free cash)
    # SELL: cash inflow (positive impact)
    if side == "BUY":
        return -(price * quantity)
    elif side == "SELL":
        return price * quantity
    return None


def _event_timestamp(event: dict[str, Any]) -> float:
    """Return unix timestamp from event. Returns 0.0 if unparseable."""
    from agents.core.event_store import parse_timestamp
    ts = parse_timestamp(event.get("occurred_at", ""))
    if ts is None:
        return 0.0
    return ts.timestamp()


def compute_equity_curve(
    events: list[dict[str, Any]],
    initial_bankroll: float = 1000.0,
    scope: Scope = "paper",
) -> list[tuple[float, float]]:
    """Compute equity curve as list of (unix_timestamp, equity).

    Args:
        events: All events from the store.
        initial_bankroll: Starting equity.
        scope: 'paper' (fill.received without shadow prefix),
               'shadow' (shadow.fill.received or fill.received with shadow: aggregate_key),
               'live' (same as paper — live fills are also fill.received).
    """
    if scope == "shadow":
        selector = _is_shadow_fill
    else:
        selector = _is_paper_fill

    fills = [e for e in events if selector(e)]
    fills.sort(key=_event_timestamp)

    equity = initial_bankroll
    curve: list[tuple[float, float]] = [(0.0, equity)]

    for fill in fills:
        ts = _event_timestamp(fill)
        delta = _fill_pnl(fill)
        if delta is None:
            continue
        equity += delta
        curve.append((ts, equity))

    return curve


def rolling_drawdown(
    equity_curve: list[tuple[float, float]],
    window_days: float = 7.0,
) -> float:
    """Return maximum drawdown observed within a rolling window.

    Args:
        equity_curve: List of (unix_timestamp, equity) sorted by time.
        window_days: Rolling window size in days.

    Returns:
        Maximum drawdown fraction (0.0 to 1.0) within any window.
    """
    if len(equity_curve) < 2:
        return 0.0

    window_secs = window_days * 86_400.0
    max_dd = 0.0

    for i, (ts_i, eq_i) in enumerate(equity_curve):
        # Find the peak within the window ending at ts_i
        window_start = ts_i - window_secs
        window_equity = [eq for (ts, eq) in equity_curve[:i + 1] if ts >= window_start]
        if not window_equity:
            continue
        peak = max(window_equity)
        if peak <= 0:
            continue
        dd = (peak - eq_i) / peak
        if dd > max_dd:
            max_dd = dd

    return max_dd
