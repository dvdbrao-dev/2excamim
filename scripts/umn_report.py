#!/usr/bin/env python3
"""Operational UMN report (utility/minimality/net) by agent and strategy."""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from replay_backtest import (  # same scripts/ directory
    DEFAULT_STORE,
    ReplayConfig,
    bps_cost,
    load_context,
    load_jsonl,
    max_drawdown,
    parse_notes,
)


@dataclass
class RowMetrics:
    pnl_gross: float = 0.0
    pnl_net: float = 0.0
    costs: float = 0.0
    trades: int = 0
    wins: int = 0
    losses: int = 0
    risk_proxy: float = 0.0
    score: float = 0.0
    class_label: str = "insufficient_data"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="UMN-style operational report by agent/strategy.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to JSONL event store.")
    parser.add_argument("--fee-bps", type=float, default=15.0, help="Fee bps per fill notional.")
    parser.add_argument(
        "--slippage-bps",
        type=float,
        default=10.0,
        help="Slippage bps per fill notional.",
    )
    parser.add_argument(
        "--min-liquidity-usdc",
        type=float,
        default=10_000.0,
        help="Low-liquidity threshold for penalty.",
    )
    parser.add_argument(
        "--low-liquidity-penalty-bps",
        type=float,
        default=5.0,
        help="Penalty bps when market volume is below threshold.",
    )
    parser.add_argument("--json", action="store_true", help="Emit JSON report.")
    return parser.parse_args()


def safe_float(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def estimated_cost_usd(event: dict[str, Any]) -> float:
    payload = event.get("payload") or {}
    direct = safe_float(payload.get("estimated_cost_usd"))
    if direct is not None:
        return direct
    notes = parse_notes((event.get("provenance") or {}).get("notes"))
    note_cost = safe_float(notes.get("estimated_cost_usd"))
    return note_cost if note_cost is not None else 0.0


def summarize_row(
    pnl_gross: float,
    pnl_net: float,
    costs: float,
    trades: int,
    wins: int,
    losses: int,
    path: list[float],
) -> RowMetrics:
    row = RowMetrics(
        pnl_gross=pnl_gross,
        pnl_net=pnl_net,
        costs=costs,
        trades=trades,
        wins=wins,
        losses=losses,
    )
    row.risk_proxy = max_drawdown(path)
    win_rate = (wins / trades) if trades > 0 else 0.0
    # UMN score: utility (pnl_net) - risk + consistency + participation.
    row.score = pnl_net - (0.5 * row.risk_proxy) + (10.0 * win_rate) + (0.1 * trades)
    if trades < 3:
        row.class_label = "insufficient_data"
    elif row.score > 0 and pnl_net > 0:
        row.class_label = "contributor"
    elif pnl_net < 0:
        row.class_label = "negative"
    else:
        row.class_label = "neutral"
    return row


def compute_report(events: list[dict[str, Any]], config: ReplayConfig) -> dict[str, Any]:
    signals, decisions, market_volume, fills, pipeline_counts = load_context(events)

    # Decision-level replay (for attribution).
    decision_state: dict[str, dict[str, float]] = {}
    decision_path: dict[str, list[float]] = {}
    for fill in fills:
        decision = decisions.get(fill.decision_id, {})
        market_id = decision.get("market_id")
        volume_usdc = market_volume.get(market_id) if isinstance(market_id, str) else None

        state = decision_state.setdefault(
            fill.decision_id,
            {
                "shares": 0.0,
                "cost_basis": 0.0,
                "pnl_gross": 0.0,
                "pnl_net": 0.0,
                "costs": 0.0,
                "trades": 0.0,
                "wins": 0.0,
                "losses": 0.0,
            },
        )
        path = decision_path.setdefault(fill.decision_id, [0.0])

        fee_cost = bps_cost(fill.notional, config.fee_bps)
        slippage_cost = bps_cost(fill.notional, config.slippage_bps)
        liquidity_cost = 0.0
        if (
            isinstance(volume_usdc, float)
            and config.min_liquidity_usdc > 0
            and volume_usdc < config.min_liquidity_usdc
        ):
            liquidity_cost = bps_cost(fill.notional, config.low_liquidity_penalty_bps)
        costs = fee_cost + slippage_cost + liquidity_cost
        pnl_change = -costs

        if fill.side == "Buy":
            state["shares"] += fill.quantity
            state["cost_basis"] += fill.notional
        else:
            shares = state["shares"]
            if shares > 0:
                close_qty = min(fill.quantity, shares)
                avg_cost = state["cost_basis"] / shares if shares > 0 else fill.price
                gross = close_qty * (fill.price - avg_cost)
                pnl_change += gross
                state["pnl_gross"] += gross
                state["shares"] -= close_qty
                state["cost_basis"] -= avg_cost * close_qty
            state["trades"] += 1
            if pnl_change > 0:
                state["wins"] += 1
            elif pnl_change < 0:
                state["losses"] += 1

        if state["shares"] <= 1e-12:
            state["shares"] = 0.0
            state["cost_basis"] = 0.0

        state["costs"] += costs
        state["pnl_net"] += pnl_change
        path.append(state["pnl_net"])

    # Aggregate by decision agent and by signal_type.
    agent_acc: dict[str, dict[str, Any]] = {}
    strategy_acc: dict[str, dict[str, Any]] = {}

    for decision_id, state in decision_state.items():
        decision = decisions.get(decision_id, {})
        signal = signals.get(decision.get("signal_id"), {})
        decision_agent = str(decision.get("decision_by") or "unknown")
        signal_type = str(signal.get("signal_type") or "unknown")

        for bucket, key in ((agent_acc, decision_agent), (strategy_acc, signal_type)):
            row = bucket.setdefault(
                key,
                {
                    "pnl_gross": 0.0,
                    "pnl_net": 0.0,
                    "costs": 0.0,
                    "trades": 0,
                    "wins": 0,
                    "losses": 0,
                    "path": [0.0],
                },
            )
            row["pnl_gross"] += state["pnl_gross"]
            row["pnl_net"] += state["pnl_net"]
            row["costs"] += state["costs"]
            row["trades"] += int(state["trades"])
            row["wins"] += int(state["wins"])
            row["losses"] += int(state["losses"])
            row["path"].append(row["pnl_net"])

    # Add event-based estimated costs per actor.
    for event in events:
        actor = (event.get("provenance") or {}).get("actor")
        if not isinstance(actor, str) or not actor.strip():
            continue
        cost = estimated_cost_usd(event)
        if cost <= 0:
            continue
        row = agent_acc.setdefault(
            actor,
            {"pnl_gross": 0.0, "pnl_net": 0.0, "costs": 0.0, "trades": 0, "wins": 0, "losses": 0, "path": [0.0]},
        )
        row["costs"] += cost
        row["pnl_net"] -= cost
        row["path"].append(row["pnl_net"])

    def finalize(bucket: dict[str, dict[str, Any]]) -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        for key, raw in bucket.items():
            summary = summarize_row(
                pnl_gross=float(raw["pnl_gross"]),
                pnl_net=float(raw["pnl_net"]),
                costs=float(raw["costs"]),
                trades=int(raw["trades"]),
                wins=int(raw["wins"]),
                losses=int(raw["losses"]),
                path=[float(value) for value in raw["path"]],
            )
            rows.append(
                {
                    "key": key,
                    "pnl_gross": round(summary.pnl_gross, 8),
                    "pnl_net": round(summary.pnl_net, 8),
                    "costs": round(summary.costs, 8),
                    "trades": summary.trades,
                    "win_rate": None if summary.trades == 0 else round(summary.wins / summary.trades, 6),
                    "risk_proxy_drawdown": round(summary.risk_proxy, 8),
                    "umn_score": round(summary.score, 8),
                    "class": summary.class_label,
                }
            )
        rows.sort(key=lambda item: (item["umn_score"], item["pnl_net"]), reverse=True)
        return rows

    return {
        "pipeline_counts": pipeline_counts,
        "agents": finalize(agent_acc),
        "strategies": finalize(strategy_acc),
        "limitations": [
            "Attribution is decision-centric using linkage.decision_id and signal_id when available.",
            "Risk proxy is max drawdown on realized net PnL path, not full mark-to-market volatility.",
            "Estimated costs use payload/provenance estimated_cost_usd when present; missing costs are treated as zero.",
        ],
    }


def render_console(payload: dict[str, Any]) -> str:
    lines = [
        "=== UMN Operational Report (Minimal) ===",
        "",
        "--- By Agent ---",
    ]
    if payload["agents"]:
        for row in payload["agents"]:
            lines.append(
                f"{row['key']}: class={row['class']} score={row['umn_score']:.4f} "
                f"pnl_net={row['pnl_net']:.6f} trades={row['trades']} "
                f"costs={row['costs']:.6f} dd={row['risk_proxy_drawdown']:.6f}"
            )
    else:
        lines.append("no agent-attributed trades")

    lines.append("")
    lines.append("--- By Strategy ---")
    if payload["strategies"]:
        for row in payload["strategies"]:
            lines.append(
                f"{row['key']}: class={row['class']} score={row['umn_score']:.4f} "
                f"pnl_net={row['pnl_net']:.6f} trades={row['trades']} "
                f"costs={row['costs']:.6f} dd={row['risk_proxy_drawdown']:.6f}"
            )
    else:
        lines.append("no strategy-attributed trades")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    config = ReplayConfig(
        fee_bps=float(args.fee_bps),
        slippage_bps=float(args.slippage_bps),
        min_liquidity_usdc=float(args.min_liquidity_usdc),
        low_liquidity_penalty_bps=float(args.low_liquidity_penalty_bps),
    )
    events = load_jsonl(Path(args.store))
    payload = compute_report(events, config)
    payload["store"] = str(args.store)
    payload["config"] = {
        "fee_bps": config.fee_bps,
        "slippage_bps": config.slippage_bps,
        "min_liquidity_usdc": config.min_liquidity_usdc,
        "low_liquidity_penalty_bps": config.low_liquidity_penalty_bps,
    }
    if args.json:
        print(json.dumps(payload, separators=(",", ":"), sort_keys=True))
    else:
        print(render_console(payload))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
