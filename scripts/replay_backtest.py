#!/usr/bin/env python3
"""Minimal event-store replay/backtest.

This script replays canonical events from the JSONL store and computes a small,
reproducible set of metrics. It intentionally avoids pretending precision where
historical data does not exist.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT_DIR / "agents"))
from core.event_store import load_jsonl, parse_timestamp  # noqa: E402


DEFAULT_STORE = Path("./var/events.jsonl")
BPS_SCALE = 10_000.0
YES_ORDER_PREFIX = "pm-paper-order-yes-"
NO_ORDER_PREFIX = "pm-paper-order-no-"


@dataclass
class ReplayConfig:
    fee_bps: float
    slippage_bps: float
    min_liquidity_usdc: float
    low_liquidity_penalty_bps: float


@dataclass
class FillRecord:
    event_id: str
    occurred_at: datetime
    decision_id: str
    signal_id: str | None
    instrument: str
    outcome: str
    side: str
    quantity: float
    price: float
    notional: float


@dataclass
class PositionState:
    shares: float = 0.0
    cost_basis: float = 0.0
    realized_gross: float = 0.0
    realized_net: float = 0.0
    costs_total: float = 0.0
    trades: int = 0
    wins: int = 0
    losses: int = 0
    pnl_path: list[float] = field(default_factory=list)


@dataclass
class ReplayResult:
    pnl_gross: float
    pnl_net: float
    costs_total: float
    trades: int
    win_rate: float | None
    exposure_mean: float
    max_drawdown: float
    open_positions: int
    closed_positions: int
    pipeline_counts: dict[str, int]
    details: dict[str, Any]
    limitations: list[str]
    generated_at: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Replay canonical event history with simple costs.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to JSONL event store.")
    parser.add_argument(
        "--fee-bps",
        type=float,
        default=15.0,
        help="Fee cost in basis points applied to each fill notional.",
    )
    parser.add_argument(
        "--slippage-bps",
        type=float,
        default=10.0,
        help="Spread/slippage cost in basis points applied to each fill notional.",
    )
    parser.add_argument(
        "--min-liquidity-usdc",
        type=float,
        default=10_000.0,
        help="When latest market volume is below this value, penalty applies.",
    )
    parser.add_argument(
        "--low-liquidity-penalty-bps",
        type=float,
        default=5.0,
        help="Extra penalty in basis points for fills on low-liquidity markets.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit full JSON report.",
    )
    return parser.parse_args()


def normalize_market_id(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        return trimmed.split(":", 1)[1]
    return trimmed


def parse_notes(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if isinstance(value, str):
        stripped = value.strip()
        if not stripped:
            return {}
        try:
            parsed = json.loads(stripped)
        except json.JSONDecodeError:
            return {}
        return parsed if isinstance(parsed, dict) else {}
    return {}


def derive_outcome(order_id: str, signal_id: str | None) -> str:
    if order_id.startswith(YES_ORDER_PREFIX):
        return "yes"
    if order_id.startswith(NO_ORDER_PREFIX):
        return "no"
    if signal_id and order_id == f"pm-paper-order-{signal_id}":
        return "signal_direction"
    return "unknown"


def safe_float(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def bps_cost(notional: float, bps: float) -> float:
    return notional * (bps / BPS_SCALE)


def max_drawdown(series: list[float]) -> float:
    if not series:
        return 0.0
    peak = series[0]
    max_dd = 0.0
    for value in series:
        if value > peak:
            peak = value
        drawdown = peak - value
        if drawdown > max_dd:
            max_dd = drawdown
    return max_dd


def load_context(
    events: list[dict[str, Any]]
) -> tuple[dict[str, dict[str, Any]], dict[str, dict[str, Any]], dict[str, float | None], list[FillRecord], dict[str, int]]:
    signals: dict[str, dict[str, Any]] = {}
    decisions: dict[str, dict[str, Any]] = {}
    market_volume: dict[str, float | None] = {}
    fills: list[FillRecord] = []
    pipeline_counts: dict[str, int] = {
        "signal.generated": 0,
        "signal.confirmed": 0,
        "veto.raised": 0,
        "decision.formed": 0,
        "fill.received": 0,
    }

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}
        provenance = event.get("provenance") or {}
        occurred_at = parse_timestamp(event.get("occurred_at")) or datetime.now(timezone.utc)

        if event_type in pipeline_counts:
            pipeline_counts[event_type] += 1

        if event_type == "market.scored":
            market_id = normalize_market_id(payload.get("market_id")) or normalize_market_id(event.get("aggregate_key"))
            if market_id is None:
                continue
            market_volume[market_id] = safe_float(payload.get("volume_usdc"))
            continue

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue
            signals[signal_id] = {
                "signal_id": signal_id,
                "signal_type": payload.get("signal_type") or payload.get("strategy") or "unknown",
                "strategy": payload.get("strategy") or "unknown",
                "side": payload.get("side"),
                "generated_by": provenance.get("actor") or event.get("produced_by"),
                "generated_at": occurred_at,
                "aggregate_key": event.get("aggregate_key"),
                "market_id": normalize_market_id(event.get("aggregate_key"))
                or normalize_market_id(payload.get("market_id")),
            }
            continue

        if event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            if isinstance(signal_id, str):
                signal = signals.setdefault(signal_id, {"signal_id": signal_id})
                signal["confirmed"] = True
                signal["confirmed_by"] = payload.get("confirmed_by")
                signal["estimated_probability"] = safe_float(payload.get("estimated_probability"))
                signal["confirmation_score"] = safe_float(payload.get("confirmation_score"))
                signal["confirmation_reasons"] = payload.get("confirmation_reasons")
                signal["rejection_reasons"] = payload.get("rejection_reasons")
            continue

        if event_type == "veto.raised" and payload.get("scope") == "Signal":
            signal_id = payload.get("target_id")
            if isinstance(signal_id, str):
                signal = signals.setdefault(signal_id, {"signal_id": signal_id})
                signal["vetoed"] = True
                signal["signal_veto_reason_code"] = payload.get("reason_code")
            continue

        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            if not isinstance(decision_id, str):
                continue
            decision_signal_id = linkage.get("signal_id") if isinstance(linkage.get("signal_id"), str) else None
            decisions[decision_id] = {
                "decision_id": decision_id,
                "signal_id": decision_signal_id,
                "decision_by": provenance.get("actor") or event.get("produced_by"),
                "action": payload.get("action"),
                "size_hint": safe_float(payload.get("size_hint")),
                "occurred_at": occurred_at,
                "aggregate_key": event.get("aggregate_key"),
                "market_id": normalize_market_id(event.get("aggregate_key")),
            }
            continue

        if event_type == "veto.raised" and payload.get("scope") == "Decision":
            decision_id = payload.get("target_id")
            if isinstance(decision_id, str):
                decision = decisions.setdefault(decision_id, {"decision_id": decision_id})
                decision["vetoed"] = True
                decision["decision_veto_by"] = payload.get("raised_by")
                decision["decision_veto_reason_code"] = payload.get("reason_code")
            continue

        if event_type == "fill.received":
            decision_id = payload.get("decision_id")
            order_id = payload.get("order_id")
            instrument = payload.get("instrument")
            side = payload.get("side")
            quantity = safe_float(payload.get("quantity"))
            price = safe_float(payload.get("price"))
            if (
                not isinstance(decision_id, str)
                or not isinstance(order_id, str)
                or not isinstance(instrument, str)
                or side not in {"Buy", "Sell"}
                or quantity is None
                or price is None
                or quantity <= 0
                or price <= 0
            ):
                continue

            signal_id = linkage.get("signal_id") if isinstance(linkage.get("signal_id"), str) else None
            fills.append(
                FillRecord(
                    event_id=str(event.get("event_id") or ""),
                    occurred_at=occurred_at,
                    decision_id=decision_id,
                    signal_id=signal_id,
                    instrument=instrument,
                    outcome=derive_outcome(order_id, signal_id),
                    side=side,
                    quantity=quantity,
                    price=price,
                    notional=quantity * price,
                )
            )

    fills.sort(key=lambda item: (item.occurred_at, item.decision_id, item.event_id))
    return signals, decisions, market_volume, fills, pipeline_counts


def run_replay(events: list[dict[str, Any]], config: ReplayConfig) -> ReplayResult:
    signals, decisions, market_volume, fills, pipeline_counts = load_context(events)
    positions: dict[tuple[str, str], PositionState] = {}
    exposure_path: list[float] = []
    cumulative_net = 0.0
    pnl_path: list[float] = [0.0]

    for fill in fills:
        key = (fill.decision_id, fill.outcome)
        position = positions.setdefault(key, PositionState())
        decision = decisions.get(fill.decision_id, {})
        market_id = decision.get("market_id")
        volume_usdc = market_volume.get(market_id) if isinstance(market_id, str) else None

        fee_cost = bps_cost(fill.notional, config.fee_bps)
        slippage_cost = bps_cost(fill.notional, config.slippage_bps)
        liquidity_cost = 0.0
        if (
            isinstance(volume_usdc, float)
            and config.min_liquidity_usdc > 0
            and volume_usdc < config.min_liquidity_usdc
        ):
            liquidity_cost = bps_cost(fill.notional, config.low_liquidity_penalty_bps)
        fill_costs = fee_cost + slippage_cost + liquidity_cost

        pnl_change = -fill_costs
        if fill.side == "Buy":
            position.shares += fill.quantity
            position.cost_basis += fill.notional
        else:
            if position.shares > 0:
                closed_qty = min(fill.quantity, position.shares)
                avg_cost = position.cost_basis / position.shares if position.shares > 0 else fill.price
                gross = closed_qty * (fill.price - avg_cost)
                pnl_change += gross
                position.realized_gross += gross
                position.trades += 1
                if pnl_change > 0:
                    position.wins += 1
                elif pnl_change < 0:
                    position.losses += 1
                position.cost_basis -= avg_cost * closed_qty
                position.shares -= closed_qty
            else:
                position.trades += 1
                position.losses += 1

        if position.shares <= 1e-12:
            position.shares = 0.0
            position.cost_basis = 0.0

        position.costs_total += fill_costs
        position.realized_net += pnl_change
        cumulative_net += pnl_change
        pnl_path.append(cumulative_net)
        position.pnl_path.append(position.realized_net)

        open_exposure = 0.0
        for current in positions.values():
            if current.shares > 0:
                open_exposure += current.shares * fill.price
        exposure_path.append(open_exposure)

    pnl_gross = sum(position.realized_gross for position in positions.values())
    pnl_net = sum(position.realized_net for position in positions.values())
    costs_total = sum(position.costs_total for position in positions.values())
    trades = sum(position.trades for position in positions.values())
    wins = sum(position.wins for position in positions.values())
    open_positions = sum(1 for position in positions.values() if position.shares > 0)
    closed_positions = sum(1 for position in positions.values() if position.shares <= 0 and position.trades > 0)
    win_rate = (wins / trades) if trades > 0 else None
    exposure_mean = (sum(exposure_path) / len(exposure_path)) if exposure_path else 0.0
    replay_drawdown = max_drawdown(pnl_path)

    details = {
        "signals_generated": len(signals),
        "signals_confirmed": len([signal for signal in signals.values() if signal.get("confirmed")]),
        "signal_vetoes": len([signal for signal in signals.values() if signal.get("vetoed")]),
        "decisions_formed": len(decisions),
        "decision_vetoes": len([decision for decision in decisions.values() if decision.get("vetoed")]),
        "fills_processed": len(fills),
    }
    limitations = [
        "PnL is reconstructed from fill.received events only; no MTM for open positions.",
        "Outcome is inferred from order_id prefixes when available (yes/no/signal_direction).",
        "Liquidity penalty uses latest market.scored volume_usdc when present; missing values imply no penalty.",
        "Drawdown uses realized net PnL path from executed fills, not a full portfolio equity curve.",
    ]

    return ReplayResult(
        pnl_gross=pnl_gross,
        pnl_net=pnl_net,
        costs_total=costs_total,
        trades=trades,
        win_rate=win_rate,
        exposure_mean=exposure_mean,
        max_drawdown=replay_drawdown,
        open_positions=open_positions,
        closed_positions=closed_positions,
        pipeline_counts=pipeline_counts,
        details=details,
        limitations=limitations,
        generated_at=datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    )


def as_json_dict(result: ReplayResult, store_path: Path, config: ReplayConfig) -> dict[str, Any]:
    return {
        "store": str(store_path),
        "generated_at": result.generated_at,
        "config": {
            "fee_bps": config.fee_bps,
            "slippage_bps": config.slippage_bps,
            "min_liquidity_usdc": config.min_liquidity_usdc,
            "low_liquidity_penalty_bps": config.low_liquidity_penalty_bps,
        },
        "metrics": {
            "pnl_gross": round(result.pnl_gross, 8),
            "pnl_net": round(result.pnl_net, 8),
            "costs_total": round(result.costs_total, 8),
            "trades": result.trades,
            "win_rate": None if result.win_rate is None else round(result.win_rate, 6),
            "exposure_mean": round(result.exposure_mean, 8),
            "max_drawdown": round(result.max_drawdown, 8),
            "open_positions": result.open_positions,
            "closed_positions": result.closed_positions,
        },
        "pipeline_counts": result.pipeline_counts,
        "details": result.details,
        "limitations": result.limitations,
    }


def format_console_report(payload: dict[str, Any]) -> str:
    metrics = payload["metrics"]
    lines = [
        "=== Replay Backtest (Minimal) ===",
        f"store: {payload['store']}",
        "",
        "--- Metrics ---",
        f"pnl_gross:      {metrics['pnl_gross']:.8f}",
        f"pnl_net:        {metrics['pnl_net']:.8f}",
        f"costs_total:    {metrics['costs_total']:.8f}",
        f"trades:         {metrics['trades']}",
        f"win_rate:       {'n/a' if metrics['win_rate'] is None else format(metrics['win_rate'], '.4f')}",
        f"exposure_mean:  {metrics['exposure_mean']:.8f}",
        f"max_drawdown:   {metrics['max_drawdown']:.8f}",
        f"open_positions: {metrics['open_positions']}",
        f"closed_positions: {metrics['closed_positions']}",
        "",
        "--- Pipeline Reconstruction ---",
    ]
    for key in ("signal.generated", "signal.confirmed", "veto.raised", "decision.formed", "fill.received"):
        lines.append(f"{key}: {payload['pipeline_counts'].get(key, 0)}")
    lines.append("")
    lines.append("--- Notes ---")
    for limitation in payload["limitations"]:
        lines.append(f"- {limitation}")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    config = ReplayConfig(
        fee_bps=float(args.fee_bps),
        slippage_bps=float(args.slippage_bps),
        min_liquidity_usdc=float(args.min_liquidity_usdc),
        low_liquidity_penalty_bps=float(args.low_liquidity_penalty_bps),
    )

    if config.fee_bps < 0 or config.slippage_bps < 0 or config.low_liquidity_penalty_bps < 0:
        raise SystemExit("cost bps values must be >= 0")
    if config.min_liquidity_usdc < 0:
        raise SystemExit("--min-liquidity-usdc must be >= 0")

    events = load_jsonl(store_path)
    result = run_replay(events, config)
    payload = as_json_dict(result, store_path, config)

    if args.json:
        print(json.dumps(payload, separators=(",", ":"), sort_keys=True))
    else:
        print(format_console_report(payload))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
