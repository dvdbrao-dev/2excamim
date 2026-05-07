#!/usr/bin/env python3
"""Conservative shadow execution simulator for candidate signals."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from core.event_store import load_jsonl
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover
    from agents.core.event_envelope import (
        append_event_jsonl,
        build_aggregate_key,
        build_event,
        build_provenance,
    )
    from agents.core.event_store import load_jsonl
    from agents.core.logging import emit_json


AGENT_ID = "shadow-execution-simulator-v1"
SOURCE = "shadow_execution_simulator"
DEFAULT_INPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Simulate conservative shadow fills for candidate signals.")
    parser.add_argument("--input-jsonl", default=str(DEFAULT_INPUT))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--strategy-version", default="oracle_lag_v1")
    parser.add_argument("--fee-rate-bps", type=float, default=25.0)
    parser.add_argument("--slippage-bps", type=float, default=20.0)
    parser.add_argument("--latency-ms", type=int, default=800)
    parser.add_argument("--max-notional-usdc", type=float, default=25.0)
    parser.add_argument("--mock-resolution", default="")
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def polymarket_taker_fee_usdc(size: float, price: float, fee_rate_bps: float) -> float:
    return max(0.0, size * (fee_rate_bps / 10_000.0) * price * (1.0 - price))


def apply_slippage(price: float, side: str, slippage_bps: float) -> float:
    _ = side
    factor = 1.0 + (slippage_bps / 10_000.0)
    return min(1.0, max(0.0, price * factor))


def estimate_fill_probability(spread_bps: float | None, depth: float | None, latency_ms: int) -> float:
    spread = spread_bps if isinstance(spread_bps, (int, float)) else 500.0
    top_depth = depth if isinstance(depth, (int, float)) else 0.0
    spread_component = max(0.0, min(1.0, 1.0 - (spread / 1000.0)))
    depth_component = max(0.0, min(1.0, top_depth / 2000.0))
    latency_component = max(0.1, min(1.0, 1000.0 / max(float(latency_ms), 1.0)))
    return round(max(0.0, min(1.0, spread_component * depth_component * latency_component)), 6)


def compute_net_pnl(outcome: str, fill_price: float, size: float, fees: float, slippage_usdc: float) -> float:
    if outcome == "UP":
        gross = size * (1.0 - fill_price)
    elif outcome == "DOWN":
        gross = -size * fill_price
    else:
        gross = 0.0
    return gross - fees - slippage_usdc


@dataclass(frozen=True)
class SignalRow:
    event_id: str
    aggregate_key: str
    payload: dict[str, Any]


def _load_signals(path: Path, strategy_version: str) -> list[SignalRow]:
    rows: list[SignalRow] = []
    for event in load_jsonl(path):
        if event.get("event_type") != "candidate_signal.scored":
            continue
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        if payload.get("strategy_version") != strategy_version:
            continue
        event_id = event.get("event_id")
        aggregate = event.get("aggregate_key")
        if isinstance(event_id, str) and isinstance(aggregate, str):
            rows.append(SignalRow(event_id=event_id, aggregate_key=aggregate, payload=payload))
    return rows


def simulate_fill(signal: SignalRow, args: argparse.Namespace) -> tuple[dict[str, Any], dict[str, Any]]:
    p = signal.payload
    rejected_signal = bool(p.get("rejected"))
    side = str(p.get("side") or "UP")
    best_bid = p.get("best_bid")
    best_ask = p.get("best_ask")
    spread_bps = p.get("spread_bps")
    depth = None
    orderbook = p.get("orderbook")
    if isinstance(orderbook, dict):
        depth_top_n = orderbook.get("depth_top_n")
        if isinstance(depth_top_n, dict):
            bid_d = depth_top_n.get("bid")
            ask_d = depth_top_n.get("ask")
            if isinstance(bid_d, (int, float)) and isinstance(ask_d, (int, float)):
                depth = float(bid_d) + float(ask_d)

    limit_price = None
    if isinstance(best_bid, (int, float)) and isinstance(best_ask, (int, float)):
        limit_price = (float(best_bid) + float(best_ask)) / 2.0

    fill_prob = estimate_fill_probability(
        float(spread_bps) if isinstance(spread_bps, (int, float)) else None,
        depth,
        args.latency_ms,
    )

    reject_reason = None
    if rejected_signal:
        reject_reason = "upstream_signal_rejected"
    elif limit_price is None:
        reject_reason = "missing_limit_price"
    elif fill_prob < 0.08:
        reject_reason = "fill_probability_too_low"

    notional = min(max(args.max_notional_usdc, 0.0), args.max_notional_usdc)
    fill_assumption = "conservative_partial" if fill_prob < 0.5 else "conservative_full"
    if reject_reason:
        size = 0.0
        sim_fill_price = None
        fees = 0.0
        slippage_usdc = 0.0
    else:
        sim_fill_price = apply_slippage(float(limit_price), side, args.slippage_bps)
        size = (notional / sim_fill_price) * min(1.0, max(0.2, fill_prob)) if sim_fill_price > 0 else 0.0
        fees = polymarket_taker_fee_usdc(size, sim_fill_price, args.fee_rate_bps)
        slippage_usdc = max(0.0, (sim_fill_price - float(limit_price)) * size)

    aggregate = signal.aggregate_key
    shadow_payload = {
        "strategy_version": args.strategy_version,
        "signal_event_id": signal.event_id,
        "asset": p.get("asset"),
        "window": p.get("window"),
        "side": side,
        "limit_price": limit_price,
        "simulated_fill_price": sim_fill_price,
        "notional_usdc": round(notional, 8),
        "size": round(size, 8),
        "fee_usdc": round(fees, 8),
        "slippage_usdc": round(slippage_usdc, 8),
        "latency_ms": int(args.latency_ms),
        "fill_probability_estimate": fill_prob,
        "fill_assumption": fill_assumption,
        "rejected": bool(reject_reason),
        "reject_reason": reject_reason,
        "source": SOURCE,
    }
    shadow_event = build_event(
        event_type="shadow_fill.simulated",
        aggregate_key=aggregate,
        payload=shadow_payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[args.strategy_version, signal.event_id, "shadow_fill"],
        timestamp=_now_iso(),
    )

    resolved_side = None
    if args.mock_resolution in ("UP", "DOWN"):
        resolved_side = args.mock_resolution
    outcome_known = resolved_side is not None
    gross_pnl = None
    net_pnl = None
    if outcome_known and sim_fill_price is not None:
        gross_pnl = compute_net_pnl(resolved_side, sim_fill_price, size, 0.0, 0.0)
        net_pnl = compute_net_pnl(resolved_side, sim_fill_price, size, fees, slippage_usdc)

    round_payload = {
        "strategy_version": args.strategy_version,
        "signal_event_id": signal.event_id,
        "fill_event_id": shadow_event["event_id"],
        "outcome_known": outcome_known,
        "resolved_side": resolved_side,
        "gross_pnl_usdc": None if gross_pnl is None else round(gross_pnl, 8),
        "net_pnl_usdc": None if net_pnl is None else round(net_pnl, 8),
        "max_adverse_excursion": None,
        "notes": "shadow-only conservative simulator",
    }
    round_event = build_event(
        event_type="strategy_round.scored",
        aggregate_key=aggregate,
        payload=round_payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[args.strategy_version, signal.event_id, "strategy_round"],
        timestamp=_now_iso(),
    )
    return shadow_event, round_event


def run(args: argparse.Namespace) -> dict[str, Any]:
    signals = _load_signals(Path(args.input_jsonl), args.strategy_version)
    events: list[dict[str, Any]] = []
    for signal in signals:
        shadow, round_scored = simulate_fill(signal, args)
        events.extend([shadow, round_scored])

    persisted = 0
    out = Path(args.output_jsonl)
    for event in events:
        if append_event_jsonl(out, event, dry_run=args.dry_run):
            persisted += 1

    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "strategy_version": args.strategy_version,
        "signals_seen": len(signals),
        "events_generated": len(events),
        "events_persisted": persisted,
        "dry_run": bool(args.dry_run),
    }


def main() -> int:
    args = parse_args()
    emit_json(run(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
