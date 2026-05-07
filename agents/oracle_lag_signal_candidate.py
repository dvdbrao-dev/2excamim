#!/usr/bin/env python3
"""Oracle lag signal candidate (shadow/research only)."""

from __future__ import annotations

import argparse
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
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


AGENT_ID = "oracle-lag-signal-candidate-v1"
SOURCE = "oracle_lag_signal_candidate"
DEFAULT_INPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")

DEFAULT_MIN_SPOT_MOVE_BPS = 8.0
DEFAULT_MAX_MARKET_PRICE = 0.92
DEFAULT_MIN_EDGE_BPS = 5.0
DEFAULT_MAX_SPREAD_BPS = 300.0
DEFAULT_MAX_STALE_SECONDS = 180
DEFAULT_MIN_SECONDS_TO_SLOT_END = 30
DEFAULT_MIN_SECONDS_FROM_SLOT_START = 0


@dataclass(frozen=True)
class SnapshotRow:
    ts: datetime
    asset: str
    window: str
    slot_start: str
    slot_end: str
    market_slug: str
    spot_price: float
    oracle_price: float | None
    mid_price: float | None
    best_bid: float | None
    best_ask: float | None
    spread_bps: float | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Score oracle-lag candidate signals from research snapshots.")
    parser.add_argument("--input-jsonl", default=str(DEFAULT_INPUT))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--asset", default="BTC")
    parser.add_argument("--window", default="5m")
    parser.add_argument("--strategy-version", default="oracle_lag_v1")
    parser.add_argument("--min-spot-move-bps", type=float, default=DEFAULT_MIN_SPOT_MOVE_BPS)
    parser.add_argument("--max-market-price", type=float, default=DEFAULT_MAX_MARKET_PRICE)
    parser.add_argument("--min-edge-bps", type=float, default=DEFAULT_MIN_EDGE_BPS)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def _parse_iso(ts: str) -> datetime:
    return datetime.fromisoformat(ts.replace("Z", "+00:00")).astimezone(timezone.utc)


def _now_utc() -> datetime:
    return datetime.now(timezone.utc)


def _to_iso(ts: datetime) -> str:
    return ts.isoformat().replace("+00:00", "Z")


def bps_delta(old: float, new: float) -> float | None:
    if old <= 0:
        return None
    return ((new - old) / old) * 10_000.0


def estimated_direction(move_bps: float) -> str:
    return "UP" if move_bps >= 0 else "DOWN"


def confidence_score(lag_gap_bps: float, spread_bps: float | None, min_edge_bps: float) -> float:
    edge_strength = min(1.0, abs(lag_gap_bps) / max(min_edge_bps * 2.0, 1.0))
    spread_penalty = 1.0
    if spread_bps is not None:
        spread_penalty = max(0.2, min(1.0, 200.0 / max(spread_bps, 1.0)))
    return round(max(0.0, min(1.0, edge_strength * spread_penalty)), 6)


def parse_snapshots(path: Path, asset: str, window: str) -> list[SnapshotRow]:
    rows: list[SnapshotRow] = []
    for event in load_jsonl(path):
        if event.get("event_type") != "market_snapshot.observed":
            continue
        payload = event.get("payload") if isinstance(event.get("payload"), dict) else {}
        if payload.get("asset") != asset or payload.get("window") != window:
            continue

        try:
            ts = _parse_iso(str(event.get("timestamp") or ""))
        except Exception:
            continue

        ob = payload.get("orderbook") if isinstance(payload.get("orderbook"), dict) else {}
        rows.append(
            SnapshotRow(
                ts=ts,
                asset=asset,
                window=window,
                slot_start=str(payload.get("slot_start")),
                slot_end=str(payload.get("slot_end")),
                market_slug=str(payload.get("market_slug")),
                spot_price=float(payload.get("spot_price")),
                oracle_price=(float(payload["oracle_price"]) if isinstance(payload.get("oracle_price"), (int, float)) else None),
                mid_price=(float(ob["mid_price"]) if isinstance(ob.get("mid_price"), (int, float)) else None),
                best_bid=(float(ob["best_bid"]) if isinstance(ob.get("best_bid"), (int, float)) else None),
                best_ask=(float(ob["best_ask"]) if isinstance(ob.get("best_ask"), (int, float)) else None),
                spread_bps=(float(ob["spread_bps"]) if isinstance(ob.get("spread_bps"), (int, float)) else None),
            )
        )
    rows.sort(key=lambda r: r.ts)
    return rows


def reject_reason(
    rows: list[SnapshotRow],
    eval_ts: datetime,
    wall_now_ts: datetime,
    min_spot_move_bps: float,
    max_market_price: float,
    min_edge_bps: float,
) -> tuple[str | None, dict[str, Any]]:
    if len(rows) < 2:
        return "insufficient_snapshots", {}

    first, last = rows[0], rows[-1]
    if (wall_now_ts - last.ts).total_seconds() > DEFAULT_MAX_STALE_SECONDS:
        return "stale_feed", {}

    if last.best_bid is None or last.best_ask is None or last.mid_price is None:
        return "missing_best_bid_ask", {}

    if last.spread_bps is None or last.spread_bps > DEFAULT_MAX_SPREAD_BPS:
        return "spread_too_wide", {}

    slot_start_dt = _parse_iso(last.slot_start)
    slot_end_dt = _parse_iso(last.slot_end)
    if eval_ts < slot_start_dt + timedelta(seconds=DEFAULT_MIN_SECONDS_FROM_SLOT_START):
        return "slot_not_active_yet", {}
    if (slot_end_dt - eval_ts).total_seconds() < DEFAULT_MIN_SECONDS_TO_SLOT_END:
        return "market_too_close_to_resolution", {}

    if not (0.0 < last.best_bid <= max_market_price and 0.0 < last.best_ask <= 1.0):
        return "price_outside_safe_band", {}

    spot_move = bps_delta(first.spot_price, last.spot_price)
    mid_move = bps_delta(first.mid_price, last.mid_price) if (first.mid_price and last.mid_price) else None
    oracle_move = (
        bps_delta(first.oracle_price, last.oracle_price)
        if (first.oracle_price is not None and last.oracle_price is not None)
        else None
    )
    if spot_move is None or mid_move is None:
        return "insufficient_price_basis", {}
    if abs(spot_move) < min_spot_move_bps:
        return "spot_move_too_small", {"spot_move_bps": spot_move}

    lag_gap = spot_move - mid_move
    raw_edge = abs(lag_gap)
    if raw_edge < min_edge_bps:
        return "edge_too_small", {
            "spot_move_bps": spot_move,
            "book_mid_delta_bps": mid_move,
            "lag_gap_bps": lag_gap,
            "raw_edge_bps": raw_edge,
            "oracle_delta_bps": oracle_move,
        }

    return None, {
        "spot_move_bps": spot_move,
        "book_mid_delta_bps": mid_move,
        "lag_gap_bps": lag_gap,
        "raw_edge_bps": raw_edge,
        "oracle_delta_bps": oracle_move,
    }


def build_events(args: argparse.Namespace, rows: list[SnapshotRow], now_ts: datetime) -> list[dict[str, Any]]:
    wall_now_ts = _now_utc()
    reason, metrics = reject_reason(
        rows,
        now_ts,
        wall_now_ts,
        args.min_spot_move_bps,
        args.max_market_price,
        args.min_edge_bps,
    )

    latest = rows[-1] if rows else None
    slot_start = latest.slot_start if latest else ""
    slot_end = latest.slot_end if latest else ""
    slug = latest.market_slug if latest else ""
    bid = latest.best_bid if latest else None
    ask = latest.best_ask if latest else None
    spread = latest.spread_bps if latest else None

    if metrics:
        spot_move = float(metrics.get("spot_move_bps", 0.0))
        mid_move = float(metrics.get("book_mid_delta_bps", 0.0))
        lag_gap = float(metrics.get("lag_gap_bps", 0.0))
        raw_edge = float(metrics.get("raw_edge_bps", 0.0))
        oracle_move = metrics.get("oracle_delta_bps")
    else:
        spot_move = 0.0
        mid_move = 0.0
        lag_gap = 0.0
        raw_edge = 0.0
        oracle_move = None

    side = estimated_direction(lag_gap)
    confidence = confidence_score(lag_gap, spread, args.min_edge_bps)
    rejected = reason is not None

    aggregate = build_aggregate_key("polymarket", args.asset.lower(), args.window)
    base_payload = {
        "asset": args.asset,
        "window": args.window,
        "slot_start": slot_start,
        "slot_end": slot_end,
        "market_slug": slug,
        "side": side,
        "spot_delta_bps": spot_move,
        "oracle_delta_bps": oracle_move,
        "book_mid_delta_bps": mid_move,
        "lag_gap_bps": lag_gap,
        "best_bid": bid,
        "best_ask": ask,
        "spread_bps": spread,
        "confidence": confidence,
        "raw_edge_bps": raw_edge,
        "rejected": rejected,
        "reject_reason": reason,
        "strategy_version": args.strategy_version,
        "governance_state": "candidate",
        "promoted": False,
        "executable": False,
        "source": SOURCE,
    }

    observed = build_event(
        event_type="oracle_lag.observed",
        aggregate_key=aggregate,
        payload={
            **base_payload,
            "observation_count": len(rows),
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[args.asset, args.window, slot_start or "none", args.strategy_version, "observed"],
        timestamp=_to_iso(now_ts),
    )

    scored = build_event(
        event_type="candidate_signal.scored",
        aggregate_key=aggregate,
        payload=base_payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[args.asset, args.window, slot_start or "none", args.strategy_version, "scored"],
        timestamp=_to_iso(now_ts),
    )
    return [observed, scored]


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows = parse_snapshots(Path(args.input_jsonl), args.asset.upper(), args.window.lower())
    now_ts = rows[-1].ts if rows else _now_utc()
    events = build_events(args, rows, now_ts)

    persisted = 0
    for event in events:
        if append_event_jsonl(Path(args.output_jsonl), event, dry_run=args.dry_run):
            persisted += 1

    scored_payload = events[1]["payload"]
    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "asset": args.asset.upper(),
        "window": args.window.lower(),
        "strategy_version": args.strategy_version,
        "snapshots_used": len(rows),
        "events_generated": len(events),
        "events_persisted": persisted,
        "dry_run": bool(args.dry_run),
        "rejected": scored_payload["rejected"],
        "reject_reason": scored_payload["reject_reason"],
        "confidence": scored_payload["confidence"],
    }


def main() -> int:
    args = parse_args()
    summary = run(args)
    emit_json(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
