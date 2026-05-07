#!/usr/bin/env python3
"""Research snapshot collector candidate (shadow/research only)."""

from __future__ import annotations

import argparse
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Protocol

try:
    from core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from core.event_store import load_jsonl
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover - fallback for package-style imports in tests
    from agents.core.event_envelope import (
        append_event_jsonl,
        build_aggregate_key,
        build_event,
        build_provenance,
    )
    from agents.core.event_store import load_jsonl
    from agents.core.logging import emit_json


AGENT_ID = "research-collector-candidate-v1"
SOURCE = "research_collector_candidate"
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_INPUT_SLOTS = Path("./var/events/external_candidates.jsonl")
SUPPORTED_WINDOWS = {"5m", "15m"}
SUPPORTED_ASSETS = {"BTC", "ETH", "SOL"}


@dataclass(frozen=True)
class SlotRecord:
    asset: str
    window: str
    slot_start: str
    slot_end: str
    market_slug: str


@dataclass(frozen=True)
class OrderLevel:
    price: float
    size: float


@dataclass(frozen=True)
class OrderBookSnapshot:
    bids: list[OrderLevel]
    asks: list[OrderLevel]


class SpotPriceAdapter(Protocol):
    def observe(self, asset: str) -> float | None: ...


class OraclePriceAdapter(Protocol):
    def observe(self, asset: str) -> float | None: ...


class OrderBookAdapter(Protocol):
    def observe(self, market_slug: str) -> OrderBookSnapshot | None: ...


class MockSpotPriceAdapter:
    BASE = {"BTC": 96000.0, "ETH": 3500.0, "SOL": 180.0}

    def observe(self, asset: str) -> float | None:
        return self.BASE.get(asset)


class MockOraclePriceAdapter:
    BASE = {"BTC": 95990.0, "ETH": 3497.0, "SOL": 179.5}

    def observe(self, asset: str) -> float | None:
        return self.BASE.get(asset)


class MockOrderBookAdapter:
    def observe(self, market_slug: str) -> OrderBookSnapshot | None:
        _ = market_slug
        return OrderBookSnapshot(
            bids=[OrderLevel(price=0.47, size=1200), OrderLevel(price=0.46, size=800)],
            asks=[OrderLevel(price=0.49, size=1100), OrderLevel(price=0.50, size=700)],
        )


class NullSpotPriceAdapter:
    def observe(self, asset: str) -> float | None:
        _ = asset
        return None


class NullOraclePriceAdapter:
    def observe(self, asset: str) -> float | None:
        _ = asset
        return None


class NullOrderBookAdapter:
    def observe(self, market_slug: str) -> OrderBookSnapshot | None:
        _ = market_slug
        return None


# --- feature helpers (pure) ---

def best_bid(book: OrderBookSnapshot | None) -> float | None:
    if book is None or not book.bids:
        return None
    return max(level.price for level in book.bids)


def best_ask(book: OrderBookSnapshot | None) -> float | None:
    if book is None or not book.asks:
        return None
    return min(level.price for level in book.asks)


def mid_price(bid: float | None, ask: float | None) -> float | None:
    if bid is None or ask is None:
        return None
    return (bid + ask) / 2.0


def spread_bps(bid: float | None, ask: float | None) -> float | None:
    mid = mid_price(bid, ask)
    if mid is None or mid <= 0:
        return None
    return ((ask - bid) / mid) * 10_000.0


def depth_top_n(levels: list[OrderLevel], n: int) -> float:
    return sum(level.size for level in levels[: max(0, n)])


def orderbook_imbalance_top_n(book: OrderBookSnapshot | None, n: int) -> float | None:
    if book is None:
        return None
    bid_depth = depth_top_n(book.bids, n)
    ask_depth = depth_top_n(book.asks, n)
    denom = bid_depth + ask_depth
    if denom <= 0:
        return None
    return (bid_depth - ask_depth) / denom


def spot_delta_bps(prices: deque[float]) -> float | None:
    if len(prices) < 2:
        return None
    first = prices[0]
    last = prices[-1]
    if first <= 0:
        return None
    return ((last - first) / first) * 10_000.0


def oracle_spot_delta_bps(oracle_price: float | None, spot_price: float | None) -> float | None:
    if oracle_price is None or spot_price is None or spot_price <= 0:
        return None
    return ((oracle_price - spot_price) / spot_price) * 10_000.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect research snapshots from candidate slots.")
    parser.add_argument("--input-slots-jsonl", default=str(DEFAULT_INPUT_SLOTS))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--assets", default="BTC,ETH,SOL")
    parser.add_argument("--windows", default="5m,15m")
    parser.add_argument("--sample-count", type=int, default=1)
    parser.add_argument("--sample-interval-ms", type=int, default=1000)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--mock", action="store_true")
    return parser.parse_args()


def _split_csv(raw: str) -> list[str]:
    return [part.strip() for part in raw.split(",") if part.strip()]


def _parse_slots(path: Path, assets: set[str], windows: set[str]) -> list[SlotRecord]:
    slots: list[SlotRecord] = []
    for record in load_jsonl(path):
        if record.get("event_type") != "market_slot.discovered":
            continue
        payload = record.get("payload") or {}
        asset = payload.get("asset")
        window = payload.get("window")
        if not isinstance(asset, str) or not isinstance(window, str):
            continue
        if asset not in assets or window not in windows:
            continue
        market_slug = payload.get("candidate_slug")
        slot_start = payload.get("slot_start")
        slot_end = payload.get("slot_end")
        if not all(isinstance(v, str) for v in (market_slug, slot_start, slot_end)):
            continue
        slots.append(
            SlotRecord(
                asset=asset,
                window=window,
                slot_start=slot_start,
                slot_end=slot_end,
                market_slug=market_slug,
            )
        )
    return slots


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _health_event(ok: bool, reason: str, ts: str) -> dict[str, Any]:
    return build_event(
        event_type="feed_health.checked",
        aggregate_key="feed:research-collector",
        payload={"ok": ok, "reason": reason, "source": SOURCE},
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["research-collector", str(ok), reason],
        timestamp=ts,
    )


def _gap_event(reason: str, ts: str) -> dict[str, Any]:
    return build_event(
        event_type="data_gap.detected",
        aggregate_key="feed:research-collector",
        payload={"gap_type": reason, "source": SOURCE, "from": ts, "to": ts},
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["research-collector", reason],
        timestamp=ts,
    )


def _snapshot_event(
    slot: SlotRecord,
    spot: float,
    oracle: float | None,
    book: OrderBookSnapshot | None,
    latency_ms: int,
    rolling_prices: deque[float],
) -> dict[str, Any]:
    bid = best_bid(book)
    ask = best_ask(book)
    spread = spread_bps(bid, ask)
    mid = mid_price(bid, ask)
    depth_bid = depth_top_n(book.bids, 2) if book else None
    depth_ask = depth_top_n(book.asks, 2) if book else None
    imbalance = orderbook_imbalance_top_n(book, 2)
    spot_d = spot_delta_bps(rolling_prices)
    oracle_spot_d = oracle_spot_delta_bps(oracle, spot)

    source_quality = "high" if (oracle is not None and book is not None) else "partial"
    payload = {
        "asset": slot.asset,
        "window": slot.window,
        "slot_start": slot.slot_start,
        "slot_end": slot.slot_end,
        "market_slug": slot.market_slug,
        "spot_price": spot,
        "oracle_price": oracle,
        "orderbook": {
            "best_bid": bid,
            "best_ask": ask,
            "mid_price": mid,
            "spread_bps": spread,
            "depth_top_n": {"n": 2, "bid": depth_bid, "ask": depth_ask},
            "imbalance_top_n": {"n": 2, "value": imbalance},
        },
        "features": {
            "spot_delta_bps": spot_d,
            "oracle_spot_delta_bps": oracle_spot_d,
        },
        "observation_latency_ms": latency_ms,
        "source_quality": source_quality,
    }
    aggregate = build_aggregate_key("polymarket", slot.asset.lower(), slot.window)
    return build_event(
        event_type="market_snapshot.observed",
        aggregate_key=aggregate,
        payload=payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[slot.asset, slot.window, slot.slot_start, slot.market_slug, str(latency_ms)],
        timestamp=_now_iso(),
    )


def run(args: argparse.Namespace) -> dict[str, Any]:
    assets = {a.upper() for a in _split_csv(args.assets)}
    windows = {w.lower() for w in _split_csv(args.windows)}
    bad_assets = sorted(assets - SUPPORTED_ASSETS)
    bad_windows = sorted(windows - SUPPORTED_WINDOWS)
    if bad_assets:
        raise SystemExit(f"unsupported assets: {','.join(bad_assets)}")
    if bad_windows:
        raise SystemExit(f"unsupported windows: {','.join(bad_windows)}")

    slots = _parse_slots(Path(args.input_slots_jsonl), assets, windows)
    events: list[dict[str, Any]] = []

    if args.mock:
        spot_adapter: SpotPriceAdapter = MockSpotPriceAdapter()
        oracle_adapter: OraclePriceAdapter = MockOraclePriceAdapter()
        orderbook_adapter: OrderBookAdapter = MockOrderBookAdapter()
        events.append(_health_event(True, "mock_adapters_active", _now_iso()))
    else:
        spot_adapter = NullSpotPriceAdapter()
        oracle_adapter = NullOraclePriceAdapter()
        orderbook_adapter = NullOrderBookAdapter()
        ts = _now_iso()
        events.append(_health_event(False, "offline_no_network_adapters_configured", ts))
        events.append(_gap_event("missing_external_adapters", ts))

    if not slots:
        ts = _now_iso()
        events.append(_gap_event("missing_market_slots", ts))

    rolling: dict[tuple[str, str], deque[float]] = {}
    slot_limit = max(0, len(slots) * max(1, args.sample_count))
    processed = 0

    for slot in slots:
        key = (slot.asset, slot.window)
        if key not in rolling:
            rolling[key] = deque(maxlen=max(2, args.sample_count))

        for i in range(max(1, args.sample_count)):
            spot = spot_adapter.observe(slot.asset)
            oracle = oracle_adapter.observe(slot.asset)
            book = orderbook_adapter.observe(slot.market_slug)
            if spot is None:
                events.append(_gap_event(f"missing_spot_price:{slot.asset}", _now_iso()))
                continue
            rolling[key].append(spot + (i * 0.01))
            latency_ms = max(0, args.sample_interval_ms)
            events.append(_snapshot_event(slot, spot + (i * 0.01), oracle, book, latency_ms, rolling[key]))
            processed += 1
            if processed >= slot_limit:
                break

    output = Path(args.output_jsonl)
    persisted = 0
    for event in events:
        if append_event_jsonl(output, event, dry_run=args.dry_run):
            persisted += 1

    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "mock": bool(args.mock),
        "dry_run": bool(args.dry_run),
        "input_slots_jsonl": str(args.input_slots_jsonl),
        "output_jsonl": str(output),
        "slots_read": len(slots),
        "events_generated": len(events),
        "events_persisted": persisted,
        "sample_count": args.sample_count,
    }


def main() -> int:
    args = parse_args()
    summary = run(args)
    emit_json(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
