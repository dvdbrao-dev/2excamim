#!/usr/bin/env python3
"""Offline backtest harness for external candidate strategies."""

from __future__ import annotations

import argparse
import csv
import json
import statistics
from dataclasses import dataclass
from datetime import datetime, timezone
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

from agents.oracle_lag_signal_candidate import bps_delta, confidence_score, estimated_direction
from agents.shadow_execution_simulator import (
    apply_slippage,
    compute_net_pnl,
    estimate_fill_probability,
    polymarket_taker_fee_usdc,
)

AGENT_ID = "external-candidate-backtest-v1"
SOURCE = "external_candidate_backtest"
DEFAULT_INPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_REPORT = Path("./reports/external_candidates/oracle_lag_v1_backtest.md")
DEFAULT_OUTPUT = Path("./var/events/external_backtest.jsonl")


@dataclass(frozen=True)
class Snapshot:
    ts: datetime
    asset: str
    window: str
    slot_start: str
    slot_end: str
    market_slug: str
    spot_price: float
    oracle_price: float | None
    best_bid: float | None
    best_ask: float | None
    mid_price: float | None
    spread_bps: float | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline backtest for external candidate strategies.")
    parser.add_argument("--input-jsonl", default=str(DEFAULT_INPUT))
    parser.add_argument("--strategy-version", default="oracle_lag_v1")
    parser.add_argument("--asset", default="BTC")
    parser.add_argument("--window", default="5m")
    parser.add_argument("--from", dest="from_ts", default="")
    parser.add_argument("--to", dest="to_ts", default="")
    parser.add_argument("--output-report", default=str(DEFAULT_REPORT))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--fee-rate-bps", type=float, default=25.0)
    parser.add_argument("--slippage-bps", type=float, default=20.0)
    parser.add_argument("--latency-ms", type=int, default=800)
    parser.add_argument("--resolution-jsonl", default="")
    return parser.parse_args()


def _iso(ts: datetime) -> str:
    return ts.isoformat().replace("+00:00", "Z")


def _parse_iso(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(timezone.utc)


def _filter_range(ts: datetime, start: datetime | None, end: datetime | None) -> bool:
    if start and ts < start:
        return False
    if end and ts > end:
        return False
    return True


def load_snapshots(path: Path, asset: str, window: str, start: datetime | None, end: datetime | None) -> list[Snapshot]:
    rows: list[Snapshot] = []
    for ev in load_jsonl(path):
        if ev.get("event_type") != "market_snapshot.observed":
            continue
        payload = ev.get("payload") if isinstance(ev.get("payload"), dict) else {}
        if payload.get("asset") != asset or payload.get("window") != window:
            continue
        ts_raw = ev.get("timestamp")
        if not isinstance(ts_raw, str):
            continue
        ts = _parse_iso(ts_raw)
        if not _filter_range(ts, start, end):
            continue
        ob = payload.get("orderbook") if isinstance(payload.get("orderbook"), dict) else {}
        rows.append(
            Snapshot(
                ts=ts,
                asset=asset,
                window=window,
                slot_start=str(payload.get("slot_start")),
                slot_end=str(payload.get("slot_end")),
                market_slug=str(payload.get("market_slug")),
                spot_price=float(payload.get("spot_price")),
                oracle_price=float(payload["oracle_price"]) if isinstance(payload.get("oracle_price"), (int, float)) else None,
                best_bid=float(ob["best_bid"]) if isinstance(ob.get("best_bid"), (int, float)) else None,
                best_ask=float(ob["best_ask"]) if isinstance(ob.get("best_ask"), (int, float)) else None,
                mid_price=float(ob["mid_price"]) if isinstance(ob.get("mid_price"), (int, float)) else None,
                spread_bps=float(ob["spread_bps"]) if isinstance(ob.get("spread_bps"), (int, float)) else None,
            )
        )
    rows.sort(key=lambda x: x.ts)
    return rows


def load_resolution_map(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    if path.suffix.lower() == ".csv":
        with path.open("r", encoding="utf-8") as fh:
            reader = csv.DictReader(fh)
            out: dict[str, str] = {}
            for row in reader:
                slug = row.get("market_slug")
                side = row.get("resolved_side")
                if isinstance(slug, str) and isinstance(side, str):
                    out[slug] = side
            return out
    out: dict[str, str] = {}
    for row in load_jsonl(path):
        slug = row.get("market_slug")
        side = row.get("resolved_side")
        if isinstance(slug, str) and isinstance(side, str):
            out[slug] = side
    return out


def rejection_reason(cur: Snapshot, spot_move: float | None, mid_move: float | None) -> str | None:
    if cur.spread_bps is None or cur.spread_bps > 300.0:
        return "spread_too_wide"
    if cur.best_bid is None or cur.best_ask is None or cur.mid_price is None:
        return "missing_best_bid_ask"
    if spot_move is None or mid_move is None:
        return "insufficient_snapshots"
    if abs(spot_move) < 8.0:
        return "spot_move_too_small"
    if cur.best_bid <= 0 or cur.best_bid > 0.92:
        return "price_outside_safe_band"
    slot_end = _parse_iso(cur.slot_end)
    if (slot_end - cur.ts).total_seconds() < 30:
        return "market_too_close_to_resolution"
    return None


def evaluate_replay(snapshots: list[Snapshot], strategy_version: str, fee_rate_bps: float, slippage_bps: float, latency_ms: int, resolution_map: dict[str, str]) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    events: list[dict[str, Any]] = []
    reject_counts: dict[str, int] = {}
    unresolved = 0

    cumulative_net = 0.0
    pnl_path = [0.0]
    net_values: list[float] = []
    gross_values: list[float] = []
    fee_values: list[float] = []
    slippage_values: list[float] = []

    for idx in range(1, len(snapshots)):
        prev = snapshots[idx - 1]
        cur = snapshots[idx]

        # No lookahead: signal at t uses only prev and current snapshot.
        spot_move = bps_delta(prev.spot_price, cur.spot_price)
        mid_move = bps_delta(prev.mid_price, cur.mid_price) if (prev.mid_price and cur.mid_price) else None
        oracle_move = bps_delta(prev.oracle_price, cur.oracle_price) if (prev.oracle_price and cur.oracle_price) else None
        lag_gap = (spot_move - mid_move) if (spot_move is not None and mid_move is not None) else 0.0
        raw_edge = abs(lag_gap)
        side = estimated_direction(lag_gap)
        reason = rejection_reason(cur, spot_move, mid_move)
        rejected = reason is not None
        confidence = confidence_score(lag_gap, cur.spread_bps, 5.0)

        base_payload = {
            "asset": cur.asset,
            "window": cur.window,
            "slot_start": cur.slot_start,
            "slot_end": cur.slot_end,
            "market_slug": cur.market_slug,
            "side": side,
            "spot_delta_bps": 0.0 if spot_move is None else spot_move,
            "oracle_delta_bps": oracle_move,
            "book_mid_delta_bps": 0.0 if mid_move is None else mid_move,
            "lag_gap_bps": lag_gap,
            "best_bid": cur.best_bid,
            "best_ask": cur.best_ask,
            "spread_bps": cur.spread_bps,
            "confidence": confidence,
            "raw_edge_bps": raw_edge,
            "rejected": rejected,
            "reject_reason": reason,
            "strategy_version": strategy_version,
            "source": SOURCE,
        }

        signal_ev = build_event(
            event_type="candidate_signal.scored",
            aggregate_key=f"polymarket:{cur.asset.lower()}:{cur.window}",
            payload=base_payload,
            provenance=build_provenance(AGENT_ID, "offline-replay"),
            unique_components=[strategy_version, cur.market_slug, cur.slot_start, str(idx), "signal"],
            timestamp=_iso(cur.ts),
        )
        events.append(signal_ev)

        fill_prob = estimate_fill_probability(cur.spread_bps, 2000.0, latency_ms)
        fill_reject = rejected or cur.mid_price is None or fill_prob < 0.08
        fill_reason = reason if rejected else ("fill_probability_too_low" if fill_prob < 0.08 else None)
        limit_price = cur.mid_price
        sim_price = None
        size = fee = slippage_usdc = 0.0
        if not fill_reject and isinstance(limit_price, float):
            sim_price = apply_slippage(limit_price, side, slippage_bps)
            notional = 25.0
            size = (notional / sim_price) * min(1.0, max(0.2, fill_prob))
            fee = polymarket_taker_fee_usdc(size, sim_price, fee_rate_bps)
            slippage_usdc = max(0.0, (sim_price - limit_price) * size)
        else:
            notional = 25.0

        fill_payload = {
            "strategy_version": strategy_version,
            "signal_event_id": signal_ev["event_id"],
            "asset": cur.asset,
            "window": cur.window,
            "side": side,
            "limit_price": limit_price,
            "simulated_fill_price": sim_price,
            "notional_usdc": notional,
            "size": size,
            "fee_usdc": fee,
            "slippage_usdc": slippage_usdc,
            "latency_ms": latency_ms,
            "fill_probability_estimate": fill_prob,
            "fill_assumption": "conservative_partial",
            "rejected": fill_reject,
            "reject_reason": fill_reason,
            "source": SOURCE,
        }
        fill_ev = build_event(
            event_type="shadow_fill.simulated",
            aggregate_key=f"polymarket:{cur.asset.lower()}:{cur.window}",
            payload=fill_payload,
            provenance=build_provenance(AGENT_ID, "offline-replay"),
            unique_components=[strategy_version, cur.market_slug, cur.slot_start, str(idx), "fill"],
            timestamp=_iso(cur.ts),
        )
        events.append(fill_ev)

        resolved_side = resolution_map.get(cur.market_slug)
        outcome_known = isinstance(resolved_side, str)
        gross = net = None
        if outcome_known and sim_price is not None:
            gross = compute_net_pnl(resolved_side, sim_price, size, 0.0, 0.0)
            net = compute_net_pnl(resolved_side, sim_price, size, fee, slippage_usdc)
            cumulative_net += net
            pnl_path.append(cumulative_net)
            gross_values.append(gross)
            net_values.append(net)
            fee_values.append(fee)
            slippage_values.append(slippage_usdc)
        else:
            unresolved += 1

        round_payload = {
            "strategy_version": strategy_version,
            "signal_event_id": signal_ev["event_id"],
            "fill_event_id": fill_ev["event_id"],
            "outcome_known": outcome_known,
            "resolved_side": resolved_side if outcome_known else None,
            "gross_pnl_usdc": gross,
            "net_pnl_usdc": net,
            "max_adverse_excursion": None,
            "notes": "offline deterministic replay",
            "signal_time": _iso(cur.ts),
            "simulated_fill_time": _iso(cur.ts),
            "resolution_time": cur.slot_end if outcome_known else None,
        }
        round_ev = build_event(
            event_type="strategy_round.scored",
            aggregate_key=f"polymarket:{cur.asset.lower()}:{cur.window}",
            payload=round_payload,
            provenance=build_provenance(AGENT_ID, "offline-replay"),
            unique_components=[strategy_version, cur.market_slug, cur.slot_start, str(idx), "round"],
            timestamp=_iso(cur.ts),
        )
        events.append(round_ev)

        if reason:
            reject_counts[reason] = reject_counts.get(reason, 0) + 1

    gross_pnl = sum(gross_values)
    net_pnl = sum(net_values)
    fees = sum(fee_values)
    slippages = sum(slippage_values)
    max_dd = 0.0
    peak = pnl_path[0] if pnl_path else 0.0
    for v in pnl_path:
        if v > peak:
            peak = v
        max_dd = max(max_dd, peak - v)

    signal_count = len([e for e in events if e["event_type"] == "candidate_signal.scored"])
    resolved_count = len(net_values)
    status = "candidate"
    suggested = "candidate"
    if resolved_count >= 100 and resolved_count > 0:
        exp_bps = (statistics.mean(net_values) / 25.0) * 10_000.0
        if exp_bps < 0:
            status = suggested = "rejected"
        elif exp_bps > 3 and max_dd <= 50:
            status = "candidate"
            suggested = "promoted"

    eval_payload = {
        "strategy_version": strategy_version,
        "signal_count": signal_count,
        "rejected_signal_count": sum(reject_counts.values()),
        "rejection_rate": (sum(reject_counts.values()) / signal_count) if signal_count else 0.0,
        "shadow_fill_count": len([e for e in events if e["event_type"] == "shadow_fill.simulated"]),
        "shadow_fill_rate": 1.0 if signal_count else 0.0,
        "resolved_round_count": resolved_count,
        "gross_pnl_usdc": gross_pnl,
        "net_pnl_usdc": net_pnl,
        "avg_net_edge_bps": ((statistics.mean(net_values) / 25.0) * 10_000.0) if net_values else 0.0,
        "median_net_edge_bps": ((statistics.median(net_values) / 25.0) * 10_000.0) if net_values else 0.0,
        "max_drawdown_usdc": max_dd,
        "win_rate": (sum(1 for v in net_values if v > 0) / len(net_values)) if net_values else 0.0,
        "expectancy_usdc": statistics.mean(net_values) if net_values else 0.0,
        "avg_fee_usdc": statistics.mean(fee_values) if fee_values else 0.0,
        "avg_slippage_usdc": statistics.mean(slippage_values) if slippage_values else 0.0,
        "avg_latency_ms": float(latency_ms),
        "data_gap_count": 0,
        "stale_feed_count": reject_counts.get("stale_feed", 0),
        "status": status,
        "suggested_status": suggested,
        "auto_promoted": False,
        "reason": "offline_backtest_evaluation",
        "thresholds": {
            "min_signals": 200,
            "min_resolved_rounds": 100,
            "max_drawdown_usdc": 50.0,
            "min_net_expectancy_bps": 3.0,
        },
        "source": SOURCE,
    }
    eval_ev = build_event(
        event_type="candidate_strategy.evaluated",
        aggregate_key=f"strategy:{strategy_version}",
        payload=eval_payload,
        provenance=build_provenance(AGENT_ID, "offline-replay"),
        unique_components=[strategy_version, str(signal_count), str(resolved_count), "eval"],
        timestamp=_iso(snapshots[-1].ts) if snapshots else _iso(datetime.now(timezone.utc)),
    )
    events.append(eval_ev)

    summary = {
        "signal_count": signal_count,
        "resolved_round_count": resolved_count,
        "unresolved_count": unresolved,
        "gross_pnl_usdc": gross_pnl,
        "net_pnl_usdc": net_pnl,
        "fees_usdc": fees,
        "slippage_usdc": slippages,
        "max_drawdown_usdc": max_dd,
        "expectancy_usdc": statistics.mean(net_values) if net_values else 0.0,
        "rejection_reasons": reject_counts,
        "data_gap_count": 0,
        "warnings": ["outcomes missing" if unresolved > 0 else ""],
    }
    return events, summary


def render_report(path: Path, dataset_window: str, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        "# External Candidate Backtest Report",
        "",
        f"- Dataset window: {dataset_window}",
        f"- Sample count: {summary['signal_count']}",
        f"- Unresolved count: {summary['unresolved_count']}",
        f"- Gross PnL: {summary['gross_pnl_usdc']:.6f}",
        f"- Net PnL: {summary['net_pnl_usdc']:.6f}",
        f"- Fees: {summary['fees_usdc']:.6f}",
        f"- Slippage: {summary['slippage_usdc']:.6f}",
        f"- Max drawdown: {summary['max_drawdown_usdc']:.6f}",
        f"- Expectancy: {summary['expectancy_usdc']:.6f}",
        "",
        "## Rejection Reasons",
        json.dumps(summary["rejection_reasons"], indent=2),
        "",
        "## Data Gaps",
        str(summary["data_gap_count"]),
        "",
        "## Warnings",
        "- " + "; ".join([w for w in summary["warnings"] if w]) if any(summary["warnings"]) else "- none",
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()
    start = _parse_iso(args.from_ts) if args.from_ts else None
    end = _parse_iso(args.to_ts) if args.to_ts else None

    started_at = datetime.now(timezone.utc)
    out_events: list[dict[str, Any]] = []
    out_events.append(
        build_event(
            event_type="backtest.run_started",
            aggregate_key=f"backtest:{args.strategy_version}:{args.asset.lower()}:{args.window}",
            payload={
                "strategy_version": args.strategy_version,
                "asset": args.asset,
                "window": args.window,
                "started_at": _iso(started_at),
                "source": SOURCE,
            },
            provenance=build_provenance(AGENT_ID, "offline-replay"),
            unique_components=[args.strategy_version, args.asset, args.window, _iso(started_at)],
            timestamp=_iso(started_at),
        )
    )

    snapshots = load_snapshots(Path(args.input_jsonl), args.asset, args.window, start, end)
    resolutions = load_resolution_map(Path(args.resolution_jsonl)) if args.resolution_jsonl else {}
    replay_events, summary = evaluate_replay(
        snapshots,
        args.strategy_version,
        args.fee_rate_bps,
        args.slippage_bps,
        args.latency_ms,
        resolutions,
    )
    out_events.extend(replay_events)

    finished_at = datetime.now(timezone.utc)
    out_events.append(
        build_event(
            event_type="backtest.run_completed",
            aggregate_key=f"backtest:{args.strategy_version}:{args.asset.lower()}:{args.window}",
            payload={
                "strategy_version": args.strategy_version,
                "asset": args.asset,
                "window": args.window,
                "completed_at": _iso(finished_at),
                "summary": summary,
                "source": SOURCE,
            },
            provenance=build_provenance(AGENT_ID, "offline-replay"),
            unique_components=[args.strategy_version, args.asset, args.window, _iso(started_at), "completed"],
            timestamp=_iso(finished_at),
        )
    )

    out_path = Path(args.output_jsonl)
    persisted = 0
    for ev in out_events:
        if append_event_jsonl(out_path, ev, dry_run=False):
            persisted += 1

    window_text = "n/a"
    if snapshots:
        window_text = f"{_iso(snapshots[0].ts)} -> {_iso(snapshots[-1].ts)}"
    render_report(Path(args.output_report), window_text, summary)

    emit_json(
        {
            "actor": AGENT_ID,
            "strategy_version": args.strategy_version,
            "snapshots": len(snapshots),
            "events_generated": len(out_events),
            "events_persisted": persisted,
            "report": str(args.output_report),
            "output_jsonl": str(args.output_jsonl),
        }
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
