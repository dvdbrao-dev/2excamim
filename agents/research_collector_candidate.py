#!/usr/bin/env python3
"""Research snapshot collector candidate (shadow/research only)."""

from __future__ import annotations

import argparse
import json
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from adapters import BinanceSpotAdapter, PolymarketMetadataAdapter, PolymarketOrderBookAdapter
    from core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from core.event_store import load_jsonl
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover
    from agents.adapters import BinanceSpotAdapter, PolymarketMetadataAdapter, PolymarketOrderBookAdapter
    from agents.core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
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
class MockMetadata:
    market_id: str | None
    condition_id: str | None
    outcome_tokens: list[dict[str, str]]
    title: str | None
    active: bool | None
    closed: bool | None
    resolved: bool | None
    end_date: str | None
    resolution_date: str | None
    raw_source_summary: str
    match_confidence: float
    match_reason: str


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
    parser.add_argument("--mock", action="store_true", help="Backward-compatible alias for --data-mode mock")
    parser.add_argument("--data-mode", choices=["mock", "read_only"], default="mock")
    parser.add_argument("--network-timeout-sec", type=float, default=5.0)
    parser.add_argument("--max-retries", type=int, default=2)
    parser.add_argument("--fail-soft", action="store_true")
    parser.add_argument("--polymarket-metadata-enabled", action="store_true")
    parser.add_argument("--polymarket-orderbook-enabled", action="store_true")
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
            SlotRecord(asset=asset, window=window, slot_start=slot_start, slot_end=slot_end, market_slug=market_slug)
        )
    return slots


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _health_event(
    ok: bool,
    partial: bool,
    reason: str,
    ts: str,
    data_mode: str,
    adapter_errors: dict[str, Any],
    successful_assets: list[str],
    failed_assets: list[str],
    metadata_found_count: int,
    orderbook_observed_count: int,
    data_gap_count: int,
) -> dict[str, Any]:
    return build_event(
        event_type="feed_health.checked",
        aggregate_key="feed:research-collector",
        payload={
            "ok": ok,
            "partial": partial,
            "reason": reason,
            "source": SOURCE,
            "data_mode": data_mode,
            "successful_assets": successful_assets,
            "failed_assets": failed_assets,
            "metadata_found_count": metadata_found_count,
            "orderbook_observed_count": orderbook_observed_count,
            "data_gap_count": data_gap_count,
            "adapter_errors": adapter_errors,
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[
            "research-collector",
            str(ok),
            str(partial),
            reason,
            data_mode,
            ",".join(successful_assets),
            ",".join(failed_assets),
        ],
        timestamp=ts,
    )


def _gap_event(
    reason: str,
    ts: str,
    data_mode: str,
    adapter_errors: dict[str, Any],
    details: dict[str, Any] | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "gap_type": reason,
        "source": SOURCE,
        "from": ts,
        "to": ts,
        "data_mode": data_mode,
        "adapter_errors": adapter_errors,
    }
    if details:
        payload.update(details)
    return build_event(
        event_type="data_gap.detected",
        aggregate_key="feed:research-collector",
        payload=payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["research-collector", reason, data_mode, str(details or {})],
        timestamp=ts,
    )


def _snapshot_event(
    slot: SlotRecord,
    spot: float,
    oracle: float | None,
    metadata: MockMetadata,
    orderbook: dict[str, Any],
    latency_ms: int,
    rolling_prices: deque[float],
    data_mode: str,
    source_quality: str,
    adapter_versions: dict[str, str],
    adapter_errors: dict[str, Any],
) -> dict[str, Any]:
    payload = {
        "asset": slot.asset,
        "window": slot.window,
        "slot_start": slot.slot_start,
        "slot_end": slot.slot_end,
        "market_slug": slot.market_slug,
        "spot_price": spot,
        "oracle_price": oracle,
        "metadata": {
            "market_id": metadata.market_id,
            "condition_id": metadata.condition_id,
            "outcome_tokens": metadata.outcome_tokens,
            "title": metadata.title,
            "active": metadata.active,
            "closed": metadata.closed,
            "resolved": metadata.resolved,
            "end_date": metadata.end_date,
            "resolution_date": metadata.resolution_date,
            "match_confidence": metadata.match_confidence,
            "match_reason": metadata.match_reason,
            "raw_source_summary": metadata.raw_source_summary,
        },
        "orderbook": orderbook,
        "features": {
            "spot_delta_bps": spot_delta_bps(rolling_prices),
            "oracle_spot_delta_bps": oracle_spot_delta_bps(oracle, spot),
        },
        "observation_latency_ms": latency_ms,
        "source_quality": source_quality,
        "data_mode": data_mode,
        "adapter_versions": adapter_versions,
        "network_latency_ms": latency_ms,
        "adapter_errors": adapter_errors,
    }
    aggregate = build_aggregate_key("polymarket", slot.asset.lower(), slot.window)
    return build_event(
        event_type="market_snapshot.observed",
        aggregate_key=aggregate,
        payload=payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[slot.asset, slot.window, slot.slot_start, slot.market_slug, data_mode],
        timestamp=_now_iso(),
    )


def _default_metadata(slot: SlotRecord) -> MockMetadata:
    return MockMetadata(None, None, [], f"mock:{slot.market_slug}", True, False, False, None, None, "mock_seeded", 1.0, "mock_seeded")


def _mode(args: argparse.Namespace) -> str:
    if args.mock:
        return "mock"
    return args.data_mode


def _mock_spot(asset: str) -> float | None:
    return {"BTC": 96000.0, "ETH": 3500.0, "SOL": 180.0}.get(asset)


def _record_error(errors: dict[str, dict[str, str]], source: str, key: str, message: str) -> None:
    if source not in errors:
        errors[source] = {}
    errors[source][key] = message


def run(args: argparse.Namespace) -> dict[str, Any]:
    data_mode = _mode(args)
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
    adapter_versions = {
        "spot": "mock_spot_adapter_v1" if data_mode == "mock" else BinanceSpotAdapter.VERSION,
        "metadata": "mock_metadata_adapter_v1" if data_mode == "mock" else PolymarketMetadataAdapter.VERSION,
        "orderbook": "mock_orderbook_adapter_v1" if data_mode == "mock" else PolymarketOrderBookAdapter.VERSION,
    }

    spot_adapter = BinanceSpotAdapter(timeout_sec=args.network_timeout_sec, max_retries=args.max_retries)
    metadata_adapter = PolymarketMetadataAdapter(timeout_sec=args.network_timeout_sec, max_retries=args.max_retries)
    orderbook_adapter = PolymarketOrderBookAdapter(
        timeout_sec=args.network_timeout_sec,
        max_retries=args.max_retries,
        enabled=bool(args.polymarket_orderbook_enabled),
    )

    if not slots:
        ts = _now_iso()
        events.append(_gap_event("missing_market_slots", ts, data_mode, {}, details={"assets": sorted(assets)}))

    rolling: dict[tuple[str, str], deque[float]] = {}
    successful_assets: set[str] = set()
    all_adapter_errors: dict[str, dict[str, str]] = {}
    metadata_found_count = 0
    orderbook_observed_count = 0
    data_gap_count = 0
    failures = 0

    for slot in slots:
        key = (slot.asset, slot.window)
        if key not in rolling:
            rolling[key] = deque(maxlen=max(2, args.sample_count))

        for i in range(max(1, args.sample_count)):
            adapter_errors: dict[str, Any] = {}
            total_latency_ms = 0

            if data_mode == "mock":
                spot_price = _mock_spot(slot.asset)
                metadata = _default_metadata(slot)
                orderbook = {
                    "books": [
                        {
                            "token_id": "mock_yes",
                            "outcome": "YES",
                            "best_bid": 0.47,
                            "best_ask": 0.49,
                            "mid_price": 0.48,
                            "spread_bps": 416.67,
                            "depth_top_n": {"n": 2, "bid": 2000.0, "ask": 1800.0},
                            "imbalance_top_n": 0.05263,
                            "raw_levels_summary": {"bids": 2, "asks": 2},
                            "source_quality": "mock",
                            "adapter_latency_ms": 1,
                        }
                    ],
                    "best_bid": 0.47,
                    "best_ask": 0.49,
                    "mid_price": 0.48,
                    "spread_bps": 416.67,
                    "depth_top_n": {"n": 2, "bid": 2000.0, "ask": 1800.0},
                    "imbalance_top_n": 0.05263,
                }
                source_quality = "mock"
            else:
                spot = spot_adapter.observe(slot.asset)
                total_latency_ms += spot.latency_ms
                spot_price = spot.price
                if spot.error:
                    _record_error(all_adapter_errors, "binance_spot", slot.asset, spot.error)
                    adapter_errors["binance_spot"] = {slot.asset: spot.error}

                if args.polymarket_metadata_enabled:
                    meta = metadata_adapter.observe(slot.market_slug)
                    total_latency_ms += meta.latency_ms
                    metadata = MockMetadata(
                        meta.market_id,
                        meta.condition_id,
                        meta.outcome_tokens,
                        meta.question,
                        meta.active,
                        meta.closed,
                        meta.resolved,
                        meta.end_date,
                        meta.resolution_date,
                        meta.raw_source_summary,
                        meta.match_confidence,
                        meta.match_reason,
                    )
                    if meta.error:
                        serialized = json.dumps(meta.adapter_errors or {"summary": meta.error}, separators=(",", ":"))
                        _record_error(all_adapter_errors, "polymarket_metadata", slot.market_slug, serialized)
                        adapter_errors["polymarket_metadata"] = meta.adapter_errors or {slot.market_slug: meta.error}
                        events.append(
                            _gap_event(
                                "missing_polymarket_metadata",
                                _now_iso(),
                                data_mode,
                                adapter_errors,
                                details={"asset": slot.asset, "window": slot.window, "market_slug": slot.market_slug},
                            )
                        )
                        data_gap_count += 1
                    else:
                        metadata_found_count += 1
                else:
                    metadata = MockMetadata(None, None, [], None, None, None, None, None, None, "metadata_disabled", 0.0, "metadata_disabled")

                if args.polymarket_orderbook_enabled:
                    ob_rows = orderbook_adapter.observe(metadata.outcome_tokens)
                    total_latency_ms += sum(ob.latency_ms for ob in ob_rows)
                    books: list[dict[str, Any]] = []
                    ob_errors: dict[str, str] = {}
                    for ob in ob_rows:
                        if ob.error:
                            ob_errors[f"polymarket_orderbook.{ob.token_id}"] = ob.error
                        else:
                            orderbook_observed_count += 1
                        books.append(
                            {
                                "token_id": ob.token_id,
                                "outcome": ob.outcome,
                                "best_bid": ob.best_bid,
                                "best_ask": ob.best_ask,
                                "mid_price": ob.mid_price,
                                "spread_bps": ob.spread_bps,
                                "depth_top_n": ob.depth_top_n,
                                "imbalance_top_n": ob.imbalance_top_n,
                                "raw_levels_summary": ob.raw_levels_summary,
                                "adapter_latency_ms": ob.latency_ms,
                                "source_quality": ob.source_quality,
                            }
                        )
                    orderbook = {"books": books}
                    primary = next((b for b in books if b.get("best_bid") is not None and b.get("best_ask") is not None), None)
                    if primary is None and books:
                        primary = books[0]
                    orderbook.update(
                        {
                            "best_bid": primary.get("best_bid") if primary else None,
                            "best_ask": primary.get("best_ask") if primary else None,
                            "mid_price": primary.get("mid_price") if primary else None,
                            "spread_bps": primary.get("spread_bps") if primary else None,
                            "depth_top_n": primary.get("depth_top_n") if primary else None,
                            "imbalance_top_n": primary.get("imbalance_top_n") if primary else None,
                        }
                    )
                    if ob_errors:
                        key_id = metadata.market_id or slot.market_slug
                        _record_error(all_adapter_errors, "polymarket_orderbook", key_id, json.dumps(ob_errors, separators=(",", ":")))
                        adapter_errors["polymarket_orderbook"] = ob_errors
                        events.append(
                            _gap_event(
                                "missing_polymarket_orderbook",
                                _now_iso(),
                                data_mode,
                                adapter_errors,
                                details={"asset": slot.asset, "window": slot.window, "market_slug": slot.market_slug},
                            )
                        )
                        data_gap_count += 1
                else:
                    orderbook = {
                        "books": [],
                        "best_bid": None,
                        "best_ask": None,
                        "mid_price": None,
                        "spread_bps": None,
                        "depth_top_n": None,
                        "imbalance_top_n": None,
                    }

                if args.polymarket_orderbook_enabled and "polymarket_orderbook" not in adapter_errors and orderbook.get("books"):
                    source_quality = "read_only_orderbook"
                elif args.polymarket_metadata_enabled and "polymarket_metadata" not in adapter_errors:
                    source_quality = "read_only_metadata"
                elif args.polymarket_metadata_enabled or args.polymarket_orderbook_enabled:
                    source_quality = "partial"
                elif spot_price is not None:
                    source_quality = "read_only_spot_only"
                else:
                    source_quality = "partial"

            if spot_price is None:
                failures += 1
                events.append(
                    _gap_event(
                        "missing_spot_price",
                        _now_iso(),
                        data_mode,
                        adapter_errors,
                        details={"asset": slot.asset, "window": slot.window, "slot_start": slot.slot_start, "slot_end": slot.slot_end},
                    )
                )
                data_gap_count += 1
                continue

            successful_assets.add(slot.asset)
            rolling[key].append(float(spot_price) + (i * 0.01))
            events.append(
                _snapshot_event(
                    slot=slot,
                    spot=float(spot_price) + (i * 0.01),
                    oracle=None,
                    metadata=metadata,
                    orderbook=orderbook,
                    latency_ms=max(total_latency_ms, args.sample_interval_ms),
                    rolling_prices=rolling[key],
                    data_mode=data_mode,
                    source_quality=source_quality,
                    adapter_versions=adapter_versions,
                    adapter_errors=adapter_errors,
                )
            )

    successful_list = sorted(successful_assets)
    failed_list = sorted(a for a in assets if a not in successful_assets)
    ts = _now_iso()

    if data_mode == "mock":
        ok = True
        partial = False
        reason = "mock_adapters_active"
    else:
        ok = len(successful_list) == len(assets)
        partial = len(successful_list) > 0 and not ok
        if ok:
            reason = "all_requested_assets_observed"
        elif partial:
            reason = "partial_spot_coverage"
        else:
            reason = "spot_failures_detected"

    events.insert(
        0,
        _health_event(
            ok=ok,
            partial=partial,
            reason=reason,
            ts=ts,
            data_mode=data_mode,
            adapter_errors=all_adapter_errors,
            successful_assets=successful_list,
            failed_assets=failed_list,
            metadata_found_count=metadata_found_count,
            orderbook_observed_count=orderbook_observed_count,
            data_gap_count=data_gap_count,
        ),
    )

    output = Path(args.output_jsonl)
    persisted = 0
    for event in events:
        if append_event_jsonl(output, event, dry_run=args.dry_run):
            persisted += 1

    if data_mode == "read_only" and not args.fail_soft and not ok:
        raise SystemExit("read_only collection did not observe all requested assets; use --fail-soft to continue")

    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "mock": data_mode == "mock",
        "data_mode": data_mode,
        "dry_run": bool(args.dry_run),
        "input_slots_jsonl": str(args.input_slots_jsonl),
        "output_jsonl": str(output),
        "slots_read": len(slots),
        "events_generated": len(events),
        "events_persisted": persisted,
        "sample_count": args.sample_count,
        "network_timeout_sec": args.network_timeout_sec,
        "max_retries": args.max_retries,
        "successful_assets": successful_list,
        "failed_assets": failed_list,
    }


def main() -> int:
    args = parse_args()
    summary = run(args)
    emit_json(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
