#!/usr/bin/env python3
"""External candidate scorecard + governance evaluator (shadow/research only)."""

from __future__ import annotations

import argparse
import statistics
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    from core.event_envelope import append_event_jsonl, build_event, build_provenance
    from core.event_store import load_jsonl
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover
    from agents.core.event_envelope import append_event_jsonl, build_event, build_provenance
    from agents.core.event_store import load_jsonl
    from agents.core.logging import emit_json


AGENT_ID = "external-candidate-scorecard-v1"
SOURCE = "external_candidate_scorecard"
DEFAULT_INPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")


@dataclass(frozen=True)
class Thresholds:
    min_signals: int
    min_resolved_rounds: int
    max_drawdown_usdc: float
    min_net_expectancy_bps: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Evaluate governance status for external candidate strategies.")
    parser.add_argument("--input-jsonl", default=str(DEFAULT_INPUT))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--strategy-version", default="oracle_lag_v1")
    parser.add_argument("--min-signals", type=int, default=200)
    parser.add_argument("--min-resolved-rounds", type=int, default=100)
    parser.add_argument("--max-drawdown-usdc", type=float, default=50.0)
    parser.add_argument("--min-net-expectancy-bps", type=float, default=3.0)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def _safe_float(value: Any) -> float | None:
    if isinstance(value, (int, float)):
        return float(value)
    return None


def _compute_drawdown(values: list[float]) -> float:
    if not values:
        return 0.0
    peak = values[0]
    max_dd = 0.0
    for value in values:
        if value > peak:
            peak = value
        dd = peak - value
        if dd > max_dd:
            max_dd = dd
    return max_dd


def evaluate(events: list[dict[str, Any]], strategy_version: str, thresholds: Thresholds) -> dict[str, Any]:
    signals: list[dict[str, Any]] = []
    fills: list[dict[str, Any]] = []
    rounds: list[dict[str, Any]] = []
    data_gap_count = 0
    stale_feed_count = 0

    for event in events:
        et = event.get("event_type")
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}

        if et == "candidate_signal.scored" and payload.get("strategy_version") == strategy_version:
            signals.append(payload)
        elif et == "shadow_fill.simulated" and payload.get("strategy_version") == strategy_version:
            fills.append(payload)
        elif et == "strategy_round.scored" and payload.get("strategy_version") == strategy_version:
            rounds.append(payload)
        elif et == "data_gap.detected":
            data_gap_count += 1
        elif et == "feed_health.checked":
            reason = payload.get("reason")
            ok = payload.get("ok")
            if ok is False and isinstance(reason, str) and "stale" in reason:
                stale_feed_count += 1

    signal_count = len(signals)
    rejected_signal_count = sum(1 for s in signals if s.get("rejected") is True)
    rejection_rate = (rejected_signal_count / signal_count) if signal_count > 0 else 0.0

    shadow_fill_count = len(fills)
    shadow_fill_rate = (shadow_fill_count / signal_count) if signal_count > 0 else 0.0

    resolved_rounds = [r for r in rounds if r.get("outcome_known") is True]
    resolved_round_count = len(resolved_rounds)
    gross_pnls = [_safe_float(r.get("gross_pnl_usdc")) for r in resolved_rounds]
    net_pnls = [_safe_float(r.get("net_pnl_usdc")) for r in resolved_rounds]
    gross_vals = [v for v in gross_pnls if isinstance(v, float)]
    net_vals = [v for v in net_pnls if isinstance(v, float)]

    gross_pnl_usdc = sum(gross_vals)
    net_pnl_usdc = sum(net_vals)
    expectancy_usdc = (net_pnl_usdc / resolved_round_count) if resolved_round_count > 0 else 0.0

    notional_sum = 0.0
    net_edge_bps_values: list[float] = []
    fee_values: list[float] = []
    slippage_values: list[float] = []
    latency_values: list[float] = []

    fill_by_signal = {
        str(f.get("signal_event_id")): f
        for f in fills
        if isinstance(f.get("signal_event_id"), str)
    }
    for round_row in resolved_rounds:
        signal_id = round_row.get("signal_event_id")
        if not isinstance(signal_id, str):
            continue
        fill = fill_by_signal.get(signal_id)
        if not fill:
            continue
        notional = _safe_float(fill.get("notional_usdc")) or 0.0
        fee = _safe_float(fill.get("fee_usdc")) or 0.0
        slippage = _safe_float(fill.get("slippage_usdc")) or 0.0
        latency = _safe_float(fill.get("latency_ms"))
        net = _safe_float(round_row.get("net_pnl_usdc"))
        if notional > 0 and isinstance(net, float):
            net_edge_bps_values.append((net / notional) * 10_000.0)
            notional_sum += notional
        fee_values.append(fee)
        slippage_values.append(slippage)
        if isinstance(latency, float):
            latency_values.append(latency)

    avg_net_edge_bps = statistics.mean(net_edge_bps_values) if net_edge_bps_values else 0.0
    median_net_edge_bps = statistics.median(net_edge_bps_values) if net_edge_bps_values else 0.0

    cumulative = []
    running = 0.0
    for v in net_vals:
        running += v
        cumulative.append(running)
    max_drawdown_usdc = _compute_drawdown(cumulative)

    wins = sum(1 for v in net_vals if v > 0)
    win_rate = (wins / len(net_vals)) if net_vals else 0.0

    avg_fee_usdc = statistics.mean(fee_values) if fee_values else 0.0
    avg_slippage_usdc = statistics.mean(slippage_values) if slippage_values else 0.0
    avg_latency_ms = statistics.mean(latency_values) if latency_values else 0.0

    # Governance: effective status is conservative and never auto-promotes by default.
    insufficient_sample = signal_count < thresholds.min_signals or resolved_round_count < thresholds.min_resolved_rounds
    severe_data_issues = data_gap_count >= 10 or stale_feed_count >= 5
    negative_expectancy = avg_net_edge_bps < thresholds.min_net_expectancy_bps and resolved_round_count >= thresholds.min_resolved_rounds

    suggested_status = "candidate"
    status = "candidate"
    reason = "insufficient_sample"

    if severe_data_issues:
        status = "frozen"
        suggested_status = "frozen"
        reason = "severe_data_quality_issues"
    elif negative_expectancy:
        status = "rejected"
        suggested_status = "rejected"
        reason = "negative_expectancy_with_sufficient_sample"
    elif insufficient_sample:
        status = "candidate"
        suggested_status = "candidate"
        reason = "insufficient_sample"
    elif avg_net_edge_bps >= thresholds.min_net_expectancy_bps and max_drawdown_usdc <= thresholds.max_drawdown_usdc:
        status = "candidate"
        suggested_status = "promoted"
        reason = "promotion_suggested_not_applied"

    return {
        "strategy_version": strategy_version,
        "signal_count": signal_count,
        "rejected_signal_count": rejected_signal_count,
        "rejection_rate": round(rejection_rate, 6),
        "shadow_fill_count": shadow_fill_count,
        "shadow_fill_rate": round(shadow_fill_rate, 6),
        "resolved_round_count": resolved_round_count,
        "gross_pnl_usdc": round(gross_pnl_usdc, 8),
        "net_pnl_usdc": round(net_pnl_usdc, 8),
        "avg_net_edge_bps": round(avg_net_edge_bps, 8),
        "median_net_edge_bps": round(median_net_edge_bps, 8),
        "max_drawdown_usdc": round(max_drawdown_usdc, 8),
        "win_rate": round(win_rate, 6),
        "expectancy_usdc": round(expectancy_usdc, 8),
        "avg_fee_usdc": round(avg_fee_usdc, 8),
        "avg_slippage_usdc": round(avg_slippage_usdc, 8),
        "avg_latency_ms": round(avg_latency_ms, 6),
        "data_gap_count": data_gap_count,
        "stale_feed_count": stale_feed_count,
        "status": status,
        "suggested_status": suggested_status,
        "auto_promoted": False,
        "reason": reason,
        "thresholds": {
            "min_signals": thresholds.min_signals,
            "min_resolved_rounds": thresholds.min_resolved_rounds,
            "max_drawdown_usdc": thresholds.max_drawdown_usdc,
            "min_net_expectancy_bps": thresholds.min_net_expectancy_bps,
        },
    }


def run(args: argparse.Namespace) -> dict[str, Any]:
    thresholds = Thresholds(
        min_signals=max(1, int(args.min_signals)),
        min_resolved_rounds=max(1, int(args.min_resolved_rounds)),
        max_drawdown_usdc=max(0.0, float(args.max_drawdown_usdc)),
        min_net_expectancy_bps=float(args.min_net_expectancy_bps),
    )
    metrics = evaluate(load_jsonl(Path(args.input_jsonl)), args.strategy_version, thresholds)

    event = build_event(
        event_type="candidate_strategy.evaluated",
        aggregate_key=f"strategy:{args.strategy_version}",
        payload={
            **metrics,
            "source": SOURCE,
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[args.strategy_version, str(metrics["signal_count"]), str(metrics["resolved_round_count"])],
        timestamp=None,
    )

    persisted = append_event_jsonl(Path(args.output_jsonl), event, dry_run=args.dry_run)
    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "strategy_version": args.strategy_version,
        "dry_run": bool(args.dry_run),
        "persisted": bool(persisted),
        "status": metrics["status"],
        "suggested_status": metrics["suggested_status"],
        "reason": metrics["reason"],
    }


def main() -> int:
    args = parse_args()
    emit_json(run(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
