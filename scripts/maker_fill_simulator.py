"""Conservative maker fill simulator for Polymarket NO-side quotes.

Models the probability that a limit order posted at a given NO price gets
filled, and detects adverse selection based on subsequent price movement.

Design principle: fail pessimistically. If uncertain, assume no fill or
maximum adverse selection. An edge that survives this model is credible.
"""
from __future__ import annotations

import math
from typing import Any, Sequence

# Maker fee on Polymarket V2 is 0% but we keep a config knob for safety
DEFAULT_MAKER_FEE_BPS: float = 0.0
DEFAULT_ADVERSE_HORIZON_STEPS: int = 3    # snapshots after fill to check
DEFAULT_ADVERSE_SIGMA_MULTIPLIER: float = 2.0  # adverse if move > 2σ
DEFAULT_QUOTE_SIZE_USD: float = 50.0


def simulate_fill(
    snapshot: dict[str, Any],
    quote_price_no: float,
    quote_size_usd: float = DEFAULT_QUOTE_SIZE_USD,
    future_snapshots: Sequence[dict[str, Any]] = (),
    adverse_sigma_multiplier: float = DEFAULT_ADVERSE_SIGMA_MULTIPLIER,
    adverse_horizon_steps: int = DEFAULT_ADVERSE_HORIZON_STEPS,
    fee_bps: float = DEFAULT_MAKER_FEE_BPS,
) -> dict[str, Any]:
    """Simulate a maker quote on the NO side.

    Args:
        snapshot: Current orderbook state. Expected keys:
            best_bid_no   (float): best bid for NO tokens
            best_ask_no   (float): best ask for NO tokens
            sigma_1h      (float, optional): 1h price std dev for adverse detection
            volume_1h_usd (float, optional): traded volume in last 1h
        quote_price_no: The NO price at which we post our bid.
        future_snapshots: Subsequent snapshots for adverse selection check.
        fee_bps: Maker fee in basis points.

    Returns dict with:
        filled            (bool)
        fill_price        (float | None)
        adverse_selection_bps (float): cost in bps if adverse flow detected
        fill_probability  (float): estimated probability of fill
        reject_reason     (str | None): why we didn't fill, if applicable
    """
    best_bid = float(snapshot.get("best_bid_no", 0.0))
    best_ask = float(snapshot.get("best_ask_no", 1.0))
    sigma_1h = float(snapshot.get("sigma_1h", 0.0))
    volume_1h = float(snapshot.get("volume_1h_usd", 0.0))

    # Validate: quote must be in [0, 1)
    if not (0.0 < quote_price_no < 1.0):
        return _no_fill("quote_price_out_of_range")

    # If our quote is below the best bid, we won't improve the queue
    # (already priced out by existing bids)
    if quote_price_no < best_bid:
        return _no_fill("price_below_best_bid")

    # If our quote is above the best ask, we'd cross the spread as taker
    # (maker orders are not crossing orders)
    if quote_price_no >= best_ask:
        return _no_fill("price_at_or_above_best_ask")

    # We are in the spread: best_bid <= quote_price_no < best_ask
    spread = best_ask - best_bid
    if spread <= 0:
        return _no_fill("zero_spread")

    # --- Conservative fill probability model ---
    # Assume we are at the back of the queue at our price level.
    # Fill requires price to trade through our level (adverse flow from our POV).
    # Volume factor: higher volume → more likely to fill
    # We use a very conservative queue position: 0.20 (we get 20% of volume at our level)
    queue_position_factor = 0.20
    if volume_1h > 0 and quote_size_usd > 0:
        # Fraction of 1h volume we represent (pessimistic: full competition)
        volume_factor = min(1.0, quote_size_usd / max(volume_1h, quote_size_usd))
    else:
        volume_factor = 0.05  # near-zero when no volume data

    # How close are we to the ask? Closer → more likely to fill
    distance_from_ask = best_ask - quote_price_no
    position_in_spread = 1.0 - (distance_from_ask / spread) if spread > 0 else 0.0

    fill_probability = queue_position_factor * volume_factor * position_in_spread
    fill_probability = max(0.0, min(1.0, fill_probability))

    # Conservative decision: we only simulate fill if probability exceeds threshold
    # We use a deterministic threshold (not random) for reproducibility
    FILL_THRESHOLD = 0.05  # very conservative — requires non-trivial probability

    if fill_probability < FILL_THRESHOLD:
        return {
            "filled": False,
            "fill_price": None,
            "adverse_selection_bps": 0.0,
            "fill_probability": round(fill_probability, 6),
            "reject_reason": "fill_probability_below_threshold",
        }

    # --- Adverse selection detection ---
    adverse_bps = _compute_adverse_selection(
        quote_price_no, future_snapshots, adverse_sigma_multiplier, adverse_horizon_steps, sigma_1h
    )

    # Fee cost
    fee_cost_bps = fee_bps

    return {
        "filled": True,
        "fill_price": round(quote_price_no, 6),
        "adverse_selection_bps": round(adverse_bps + fee_cost_bps, 4),
        "fill_probability": round(fill_probability, 6),
        "reject_reason": None,
    }


def _no_fill(reason: str) -> dict[str, Any]:
    return {
        "filled": False,
        "fill_price": None,
        "adverse_selection_bps": 0.0,
        "fill_probability": 0.0,
        "reject_reason": reason,
    }


def _compute_adverse_selection(
    fill_price_no: float,
    future_snapshots: Sequence[dict[str, Any]],
    sigma_multiplier: float,
    horizon_steps: int,
    sigma_1h: float,
) -> float:
    """Return adverse selection cost in bps.

    Adverse if the NO midpoint moves AGAINST our position (DOWN) by > sigma_multiplier * sigma
    within horizon_steps. We are long NO, so a drop in NO price is adverse.
    """
    if not future_snapshots or sigma_1h <= 0:
        # No data → assume worst case: use 50bps as conservative penalty
        return 50.0

    steps = list(future_snapshots)[:horizon_steps]
    if not steps:
        return 50.0

    threshold_drop = sigma_multiplier * sigma_1h

    for snap in steps:
        future_mid = _snapshot_no_midpoint(snap)
        if future_mid is None:
            continue
        drop = fill_price_no - future_mid  # positive = NO price dropped (adverse for long NO)
        if drop > threshold_drop:
            # Toxic flow: express as bps of notional
            return round(drop * 10_000, 2)

    return 0.0


def _snapshot_no_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = snapshot.get("best_bid_no")
    best_ask = snapshot.get("best_ask_no")
    if best_bid is not None and best_ask is not None:
        try:
            return (float(best_bid) + float(best_ask)) / 2.0
        except (ValueError, TypeError):
            pass
    mid = snapshot.get("no_midpoint")
    if mid is not None:
        try:
            return float(mid)
        except (ValueError, TypeError):
            pass
    return None


def build_synthetic_snapshot(
    no_midpoint: float,
    spread: float = 0.02,
    volume_1h_usd: float = 500.0,
    sigma_1h: float = 0.01,
) -> dict[str, Any]:
    """Build a synthetic orderbook snapshot from market-level data."""
    half_spread = spread / 2.0
    return {
        "best_bid_no": round(max(0.0, no_midpoint - half_spread), 6),
        "best_ask_no": round(min(1.0, no_midpoint + half_spread), 6),
        "no_midpoint": round(no_midpoint, 6),
        "volume_1h_usd": volume_1h_usd,
        "sigma_1h": sigma_1h,
    }
