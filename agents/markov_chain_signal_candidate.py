#!/usr/bin/env python3
"""Markov-chain signal candidate for short-duration crypto prediction slots."""

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
    from agents.core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from agents.core.event_store import load_jsonl
    from agents.core.logging import emit_json


AGENT_ID = "markov-chain-signal-candidate-v1"
SOURCE = "markov_chain_signal_candidate"
DEFAULT_INPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_MAX_STALE_SECONDS = 180


@dataclass(frozen=True)
class SnapshotRow:
    ts: datetime
    asset: str
    window: str
    slot_start: str
    slot_end: str
    market_slug: str
    spot_price: float
    best_bid_up: float | None
    best_ask_up: float | None
    best_bid_down: float | None
    best_ask_down: float | None
    spread_bps: float | None
    depth_total: float | None
    imbalance: float | None


@dataclass(frozen=True)
class SlotFeatures:
    slot_start: str
    slot_end: str
    market_slug: str
    open_spot: float
    close_spot: float
    spot_return: float
    volatility: float
    best_bid_up: float | None
    best_ask_up: float | None
    best_bid_down: float | None
    best_ask_down: float | None
    spread_bps: float | None
    depth_total: float | None
    imbalance: float | None
    outcome: str
    outcome_source: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Markov-chain candidate signal scorer for external research")
    parser.add_argument("--input-jsonl", default=str(DEFAULT_INPUT))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--asset", default="BTC")
    parser.add_argument("--window", default="5m")
    parser.add_argument("--strategy-version", default="markov_chain_v1")
    parser.add_argument("--lookback-slots", type=int, default=2016)
    parser.add_argument("--min-state-samples", type=int, default=30)
    parser.add_argument("--min-global-samples", type=int, default=300)
    parser.add_argument("--alpha", type=float, default=5.0)
    parser.add_argument("--beta", type=float, default=5.0)
    parser.add_argument("--min-net-edge", type=float, default=0.02)
    parser.add_argument("--cost-buffer", type=float, default=0.015)
    parser.add_argument("--max-spread-bps", type=float, default=300.0)
    parser.add_argument("--min-depth-usdc", type=float, default=500.0)
    parser.add_argument("--min-seconds-to-expiry", type=int, default=45)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def _now_utc() -> datetime:
    return datetime.now(timezone.utc)


def _to_iso(ts: datetime) -> str:
    return ts.isoformat().replace("+00:00", "Z")


def _parse_iso(ts: str) -> datetime:
    return datetime.fromisoformat(ts.replace("Z", "+00:00")).astimezone(timezone.utc)


def beta_smoothed_probability(count_up: int, count_total: int, alpha: float, beta: float) -> float:
    denom = count_total + alpha + beta
    if denom <= 0:
        return 0.5
    return (count_up + alpha) / denom


def momentum_bucket(spot_return: float) -> str:
    if spot_return <= -0.0020:
        return "strong_down"
    if spot_return <= -0.0006:
        return "down"
    if spot_return < 0.0006:
        return "flat"
    if spot_return < 0.0020:
        return "up"
    return "strong_up"


def volatility_bucket(volatility: float) -> str:
    if volatility < 0.0008:
        return "low"
    if volatility < 0.0020:
        return "medium"
    return "high"


def book_skew_bucket(imbalance: float | None) -> str:
    if imbalance is None:
        return "neutral"
    if imbalance <= -0.15:
        return "down_favored"
    if imbalance >= 0.15:
        return "up_favored"
    return "neutral"


def _extract_orderbook_fields(payload: dict[str, Any]) -> tuple[float | None, float | None, float | None, float | None, float | None, float | None, float | None]:
    ob = payload.get("orderbook") if isinstance(payload.get("orderbook"), dict) else {}

    best_bid_up = ob.get("best_bid") if isinstance(ob.get("best_bid"), (int, float)) else None
    best_ask_up = ob.get("best_ask") if isinstance(ob.get("best_ask"), (int, float)) else None
    spread_bps = ob.get("spread_bps") if isinstance(ob.get("spread_bps"), (int, float)) else None

    depth_total = None
    depth = ob.get("depth_top_n") if isinstance(ob.get("depth_top_n"), dict) else {}
    bid_d = depth.get("bid") if isinstance(depth.get("bid"), (int, float)) else None
    ask_d = depth.get("ask") if isinstance(depth.get("ask"), (int, float)) else None
    if bid_d is not None and ask_d is not None:
        depth_total = float(bid_d) + float(ask_d)

    imbalance = None
    raw_imb = ob.get("imbalance_top_n")
    if isinstance(raw_imb, (int, float)):
        imbalance = float(raw_imb)
    elif isinstance(raw_imb, dict) and isinstance(raw_imb.get("value"), (int, float)):
        imbalance = float(raw_imb["value"])

    best_bid_down = None
    best_ask_down = None
    books = ob.get("books") if isinstance(ob.get("books"), list) else []
    for row in books:
        if not isinstance(row, dict):
            continue
        outcome = str(row.get("outcome") or "").upper()
        b = row.get("best_bid") if isinstance(row.get("best_bid"), (int, float)) else None
        a = row.get("best_ask") if isinstance(row.get("best_ask"), (int, float)) else None
        if outcome in {"YES", "UP"}:
            if b is not None:
                best_bid_up = float(b)
            if a is not None:
                best_ask_up = float(a)
        if outcome in {"NO", "DOWN"}:
            if b is not None:
                best_bid_down = float(b)
            if a is not None:
                best_ask_down = float(a)

    # Conservative complement fallback when only UP side exists.
    if best_bid_down is None and best_ask_up is not None:
        best_bid_down = max(0.0, min(1.0, 1.0 - float(best_ask_up)))
    if best_ask_down is None and best_bid_up is not None:
        best_ask_down = max(0.0, min(1.0, 1.0 - float(best_bid_up)))

    return (
        float(best_bid_up) if best_bid_up is not None else None,
        float(best_ask_up) if best_ask_up is not None else None,
        float(best_bid_down) if best_bid_down is not None else None,
        float(best_ask_down) if best_ask_down is not None else None,
        float(spread_bps) if spread_bps is not None else None,
        depth_total,
        imbalance,
    )


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

        spot = payload.get("spot_price")
        if not isinstance(spot, (int, float)):
            continue

        slot_start = payload.get("slot_start")
        slot_end = payload.get("slot_end")
        market_slug = payload.get("market_slug")
        if not all(isinstance(v, str) and v for v in (slot_start, slot_end, market_slug)):
            continue

        best_bid_up, best_ask_up, best_bid_down, best_ask_down, spread, depth_total, imbalance = _extract_orderbook_fields(payload)
        rows.append(
            SnapshotRow(
                ts=ts,
                asset=asset,
                window=window,
                slot_start=slot_start,
                slot_end=slot_end,
                market_slug=market_slug,
                spot_price=float(spot),
                best_bid_up=best_bid_up,
                best_ask_up=best_ask_up,
                best_bid_down=best_bid_down,
                best_ask_down=best_ask_down,
                spread_bps=spread,
                depth_total=depth_total,
                imbalance=imbalance,
            )
        )
    rows.sort(key=lambda r: r.ts)
    return rows


def build_slot_features(rows: list[SnapshotRow]) -> list[SlotFeatures]:
    by_slot: dict[str, list[SnapshotRow]] = {}
    for row in rows:
        by_slot.setdefault(row.slot_start, []).append(row)

    slots: list[SlotFeatures] = []
    for slot_start, bucket in by_slot.items():
        bucket.sort(key=lambda r: r.ts)
        first = bucket[0]
        last = bucket[-1]
        if first.spot_price <= 0:
            continue
        open_spot = first.spot_price
        close_spot = last.spot_price
        spot_return = (close_spot - open_spot) / open_spot
        highs = max(r.spot_price for r in bucket)
        lows = min(r.spot_price for r in bucket)
        volatility = ((highs - lows) / open_spot) if open_spot > 0 else 0.0
        outcome = "UP" if close_spot > open_spot else "DOWN"
        slots.append(
            SlotFeatures(
                slot_start=slot_start,
                slot_end=last.slot_end,
                market_slug=last.market_slug,
                open_spot=open_spot,
                close_spot=close_spot,
                spot_return=spot_return,
                volatility=volatility,
                best_bid_up=last.best_bid_up,
                best_ask_up=last.best_ask_up,
                best_bid_down=last.best_bid_down,
                best_ask_down=last.best_ask_down,
                spread_bps=last.spread_bps,
                depth_total=last.depth_total,
                imbalance=last.imbalance,
                outcome=outcome,
                outcome_source="binance_reconstructed_v1",
            )
        )

    slots.sort(key=lambda s: _parse_iso(s.slot_start))
    return slots


def _state_key(previous_outcome: str, slot: SlotFeatures) -> tuple[str, str, str, str]:
    return (
        previous_outcome,
        momentum_bucket(slot.spot_return),
        volatility_bucket(slot.volatility),
        book_skew_bucket(slot.imbalance),
    )


def _count_outcomes(states: list[tuple[tuple[str, ...], str]]) -> dict[tuple[str, ...], dict[str, int]]:
    counts: dict[tuple[str, ...], dict[str, int]] = {}
    for key, outcome in states:
        if key not in counts:
            counts[key] = {"UP": 0, "DOWN": 0}
        counts[key][outcome] += 1
    return counts


def _estimate_backoff(state: tuple[str, str, str, str], train: list[tuple[tuple[str, ...], str]], alpha: float, beta: float) -> tuple[float, int, str]:
    levels = [
        ("full_state", lambda k: (k[0], k[1], k[2], k[3])),
        ("prev_momentum_vol", lambda k: (k[0], k[1], k[2])),
        ("prev_momentum", lambda k: (k[0], k[1])),
        ("previous_outcome", lambda k: (k[0],)),
        ("global_prior", lambda k: tuple()),
    ]

    for name, mapper in levels:
        key = mapper(state)
        filtered = [out for raw, out in train if mapper(raw) == key]
        total = len(filtered)
        if total == 0 and name != "global_prior":
            continue
        up = sum(1 for x in filtered if x == "UP")
        p_up = beta_smoothed_probability(up, total, alpha, beta)
        return p_up, total, name
    return 0.5, 0, "global_prior"


def _reject_reason(
    eval_ts: datetime,
    latest_snapshot_ts: datetime,
    slot: SlotFeatures,
    global_samples: int,
    state_samples: int,
    min_global_samples: int,
    min_state_samples: int,
    max_spread_bps: float,
    min_depth_usdc: float,
    min_seconds_to_expiry: int,
    net_edge: float,
    min_net_edge: float,
    orderbook_missing: bool,
) -> str | None:
    if (eval_ts - latest_snapshot_ts).total_seconds() > DEFAULT_MAX_STALE_SECONDS:
        return "stale_feed"
    if orderbook_missing:
        return "orderbook_missing"
    if global_samples < min_global_samples:
        return "insufficient_global_samples"
    if state_samples < min_state_samples:
        return "insufficient_state_samples"
    if slot.spread_bps is None or slot.spread_bps > max_spread_bps:
        return "spread_too_wide"
    if slot.depth_total is None or slot.depth_total < min_depth_usdc:
        return "depth_too_low"
    seconds_to_expiry = (_parse_iso(slot.slot_end) - eval_ts).total_seconds()
    if seconds_to_expiry < min_seconds_to_expiry:
        return "too_close_to_expiry"
    if net_edge < min_net_edge:
        return "edge_below_threshold"
    return None


def confidence(net_edge: float, samples: int) -> float:
    edge_component = max(0.0, min(1.0, net_edge / 0.08))
    sample_component = max(0.0, min(1.0, samples / 200.0))
    return round(edge_component * sample_component, 6)


def _events_persisted_count(dry_run: bool, persisted: bool) -> int:
    if dry_run:
        return 0
    return 1 if persisted else 0


def run(args: argparse.Namespace) -> dict[str, Any]:
    rows = parse_snapshots(Path(args.input_jsonl), args.asset.upper(), args.window.lower())
    if not rows:
        gap = build_event(
            event_type="data_gap.detected",
            aggregate_key="feed:markov-chain",
            payload={
                "gap_type": "missing_market_snapshots",
                "source": SOURCE,
                "data_mode": "unknown",
                "adapter_errors": {},
                "asset": args.asset.upper(),
                "window": args.window.lower(),
            },
            provenance=build_provenance(AGENT_ID, "shadow-research"),
            unique_components=[args.asset.upper(), args.window.lower(), "missing_market_snapshots"],
            timestamp=None,
        )
        persisted = append_event_jsonl(Path(args.output_jsonl), gap, dry_run=args.dry_run)
        return {
            "actor": AGENT_ID,
            "source": SOURCE,
            "strategy_version": args.strategy_version,
            "events_generated": 1,
            "events_persisted": _events_persisted_count(bool(args.dry_run), persisted),
            "reject_reason": "missing_market_snapshots",
            "dry_run": bool(args.dry_run),
        }

    slots = build_slot_features(rows)
    slots = slots[-max(3, int(args.lookback_slots)) :]
    if len(slots) < 2:
        rejected_payload = {
            "asset": args.asset.upper(),
            "window": args.window.lower(),
            "slot_start": slots[-1].slot_start,
            "slot_end": slots[-1].slot_end,
            "market_slug": slots[-1].market_slug,
            "side": "UP",
            "confidence": 0.0,
            "raw_edge_bps": 0.0,
            "rejected": True,
            "reject_reason": "insufficient_snapshots",
            "strategy_version": args.strategy_version,
            "governance_state": "candidate",
            "promoted": False,
            "executable": False,
            "source": SOURCE,
        }
        event = build_event(
            event_type="candidate_signal.scored",
            aggregate_key=build_aggregate_key("polymarket", args.asset.lower(), args.window.lower()),
            payload=rejected_payload,
            provenance=build_provenance(AGENT_ID, "shadow-research"),
            unique_components=[args.asset.upper(), args.window.lower(), slots[-1].slot_start, args.strategy_version],
            timestamp=_to_iso(rows[-1].ts),
        )
        persisted = append_event_jsonl(Path(args.output_jsonl), event, dry_run=args.dry_run)
        return {
            "actor": AGENT_ID,
            "source": SOURCE,
            "strategy_version": args.strategy_version,
            "events_generated": 1,
            "events_persisted": _events_persisted_count(bool(args.dry_run), persisted),
            "reject_reason": "insufficient_snapshots",
            "dry_run": bool(args.dry_run),
        }

    train_pairs: list[tuple[tuple[str, ...], str]] = []
    for idx in range(1, len(slots) - 1):
        prev = slots[idx - 1]
        cur = slots[idx]
        train_pairs.append((_state_key(prev.outcome, cur), cur.outcome))

    global_samples = len(train_pairs)
    previous_outcome = slots[-2].outcome
    active_slot = slots[-1]
    active_state = _state_key(previous_outcome, active_slot)

    p_up, state_samples, backoff_level = _estimate_backoff(active_state, train_pairs, float(args.alpha), float(args.beta))
    p_down = 1.0 - p_up

    ask_up = active_slot.best_ask_up
    ask_down = active_slot.best_ask_down
    orderbook_missing = ask_up is None or ask_down is None

    edge_up = (p_up - ask_up - float(args.cost_buffer)) if ask_up is not None else -1.0
    edge_down = (p_down - ask_down - float(args.cost_buffer)) if ask_down is not None else -1.0

    side = "UP" if edge_up >= edge_down else "DOWN"
    entry_price = ask_up if side == "UP" else ask_down
    gross_edge = p_up - ask_up if side == "UP" and ask_up is not None else (p_down - ask_down if ask_down is not None else 0.0)
    net_edge = max(edge_up, edge_down)

    reason = _reject_reason(
        eval_ts=rows[-1].ts,
        latest_snapshot_ts=rows[-1].ts,
        slot=active_slot,
        global_samples=global_samples,
        state_samples=state_samples,
        min_global_samples=max(1, int(args.min_global_samples)),
        min_state_samples=max(1, int(args.min_state_samples)),
        max_spread_bps=float(args.max_spread_bps),
        min_depth_usdc=float(args.min_depth_usdc),
        min_seconds_to_expiry=max(0, int(args.min_seconds_to_expiry)),
        net_edge=net_edge,
        min_net_edge=float(args.min_net_edge),
        orderbook_missing=orderbook_missing,
    )

    payload = {
        "asset": args.asset.upper(),
        "window": args.window.lower(),
        "slot_start": active_slot.slot_start,
        "slot_end": active_slot.slot_end,
        "market_slug": active_slot.market_slug,
        "side": side,
        "confidence": confidence(max(0.0, net_edge), state_samples),
        "raw_edge_bps": round(float(gross_edge) * 10_000.0, 6),
        "rejected": reason is not None,
        "reject_reason": reason,
        "strategy_version": args.strategy_version,
        "governance_state": "candidate",
        "promoted": False,
        "executable": False,
        "source": SOURCE,
        "model": {
            "model_type": "markov_chain",
            "order": 1,
            "state": {
                "previous_outcome": active_state[0],
                "spot_momentum_bucket": active_state[1],
                "volatility_bucket": active_state[2],
                "book_skew_bucket": active_state[3],
                "outcome_source": active_slot.outcome_source,
            },
            "state_samples": state_samples,
            "global_samples": global_samples,
            "alpha": float(args.alpha),
            "beta": float(args.beta),
            "p_up": round(p_up, 8),
            "p_down": round(p_down, 8),
            "backoff_level": backoff_level,
        },
        "pricing": {
            "best_bid_up": active_slot.best_bid_up,
            "best_ask_up": active_slot.best_ask_up,
            "best_bid_down": active_slot.best_bid_down,
            "best_ask_down": active_slot.best_ask_down,
            "spread_bps": active_slot.spread_bps,
            "depth_total_usdc": active_slot.depth_total,
        },
        "edge": {
            "fair_value": round(p_up if side == "UP" else p_down, 8),
            "entry_price": entry_price,
            "gross_edge": round(gross_edge, 8),
            "cost_buffer": float(args.cost_buffer),
            "net_edge": round(net_edge, 8),
        },
    }

    event = build_event(
        event_type="candidate_signal.scored",
        aggregate_key=build_aggregate_key("polymarket", args.asset.lower(), args.window.lower()),
        payload=payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[args.asset.upper(), args.window.lower(), active_slot.slot_start, args.strategy_version],
        timestamp=_to_iso(rows[-1].ts),
    )

    persisted = append_event_jsonl(Path(args.output_jsonl), event, dry_run=args.dry_run)
    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "strategy_version": args.strategy_version,
        "events_generated": 1,
        "events_persisted": _events_persisted_count(bool(args.dry_run), persisted),
        "rejected": payload["rejected"],
        "reject_reason": payload["reject_reason"],
        "backoff_level": backoff_level,
        "state_samples": state_samples,
        "global_samples": global_samples,
        "dry_run": bool(args.dry_run),
    }


def main() -> int:
    args = parse_args()
    emit_json(run(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
