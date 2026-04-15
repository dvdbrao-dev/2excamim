#!/usr/bin/env python3
"""Scoring Agent v1.

Reads market-watch snapshots and scores active markets that pass a small
elimination filter. Passing markets are written as `market.scored` events into
the local JSONL store.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


AGENT_ID = "scoring-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")

PRICE_GAP_LIMIT = 0.07
MIN_VOLUME_USDC = 50_000.0
MIN_RESOLUTION_HOURS = 4.0
MAX_RESOLUTION_HOURS = 168.0


@dataclass
class MarketCandidate:
    market_id: str
    title: str
    market_price: float
    price_gap_to_half: float
    volume_usdc: float
    hours_to_resolution: float | None
    score: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Score active markets from market-watch snapshots.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def utc_today() -> str:
    return datetime.now(timezone.utc).date().isoformat()


def execution_run_id() -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return f"{AGENT_ID}-{timestamp}"


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []

    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        stripped = line.strip()
        if not stripped:
            continue
        try:
            records.append(json.loads(stripped))
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid JSONL in {path} at line {line_number}: {error}") from error
    return records


def decode_raw_payload(payload: Any) -> str | None:
    if isinstance(payload, str):
        return payload
    if isinstance(payload, list) and all(isinstance(item, int) and 0 <= item <= 255 for item in payload):
        return bytes(payload).decode("utf-8")
    return None


def parse_timestamp(value: Any) -> datetime | None:
    if isinstance(value, str):
        trimmed = value.strip()
        if not trimmed:
            return None
        try:
            return datetime.fromisoformat(trimmed.replace("Z", "+00:00")).astimezone(timezone.utc)
        except ValueError:
            pass
    elif isinstance(value, (int, float)) and math.isfinite(float(value)):
        return datetime.fromtimestamp(float(value), tz=timezone.utc)
    return None


def extract_first_string(data: dict[str, Any], keys: tuple[str, ...]) -> str | None:
    for key in keys:
        value = data.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def iter_market_objects(document: Any) -> list[dict[str, Any]]:
    if isinstance(document, list):
        return [item for item in document if isinstance(item, dict)]
    if isinstance(document, dict):
        for key in ("markets", "data", "results"):
            value = document.get(key)
            if isinstance(value, list):
                return [item for item in value if isinstance(item, dict)]
        return [document]
    return []


def load_resolution_map(watch_dir: Path) -> dict[str, datetime]:
    raw_path = watch_dir / "raw" / "polymarket-discovery.jsonl"
    resolution_by_market: dict[str, datetime] = {}

    for record in load_jsonl(raw_path):
        payload = decode_raw_payload(record.get("payload"))
        if payload is None:
            continue

        try:
            document = json.loads(payload)
        except json.JSONDecodeError:
            continue

        for market in iter_market_objects(document):
            market_id = extract_first_string(market, ("conditionId", "condition_id", "id"))
            resolution_at = parse_timestamp(
                market.get("end_date_iso")
                or market.get("endDateIso")
                or market.get("end_date")
                or market.get("resolutionDate")
                or market.get("resolution_date")
                or market.get("resolutionAt")
                or market.get("resolution_at")
                or market.get("endDate")
                or market.get("end_date")
                or market.get("closeDate")
                or market.get("close_date")
                or market.get("expiresAt")
                or market.get("expires_at")
                or market.get("closeTime")
                or market.get("close_time")
            )
            if market_id and resolution_at is not None:
                resolution_by_market[market_id] = resolution_at

    return resolution_by_market


def load_active_snapshots(watch_dir: Path) -> list[dict[str, Any]]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    snapshots_by_market: dict[str, dict[str, Any]] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = snapshot.get("market_id")
        status = snapshot.get("status")
        if not isinstance(market_id, str) or status != "Open":
            continue
        snapshots_by_market[market_id] = snapshot

    return list(snapshots_by_market.values())


def market_price(snapshot: dict[str, Any]) -> float | None:
    last_price = snapshot.get("last_price")
    if isinstance(last_price, (int, float)):
        return float(last_price)

    best_bid = snapshot.get("best_bid")
    best_ask = snapshot.get("best_ask")
    if isinstance(best_bid, (int, float)) and isinstance(best_ask, (int, float)):
        return (float(best_bid) + float(best_ask)) / 2.0

    if isinstance(best_bid, (int, float)):
        return float(best_bid)
    if isinstance(best_ask, (int, float)):
        return float(best_ask)
    return None


def score_market(
    snapshot: dict[str, Any], resolution_at: datetime | None
) -> tuple[MarketCandidate | None, str | None]:
    market_id = snapshot.get("market_id")
    if not isinstance(market_id, str) or not market_id.strip():
        return None, "missing market_id"

    price = market_price(snapshot)
    if price is None:
        return None, "missing market price"

    volume = snapshot.get("volume")
    volume_usdc = 0.0
    if isinstance(volume, (int, float)) and math.isfinite(float(volume)):
        volume_usdc = float(volume)

    observed_at = parse_timestamp(snapshot.get("observed_at"))
    if observed_at is None:
        return None, "missing observed_at"

    hours_to_resolution = None
    if resolution_at is not None:
        hours_to_resolution = (resolution_at - observed_at).total_seconds() / 3600.0
    price_gap_to_half = abs(price - 0.5)

    if price_gap_to_half < PRICE_GAP_LIMIT:
        return None, "price gap below floor"

    gap_score = min(1.0, max(0.0, (price_gap_to_half - PRICE_GAP_LIMIT) / (0.5 - PRICE_GAP_LIMIT)))
    volume_score = 0.0 if volume_usdc == 0.0 else min(1.0, volume_usdc / MIN_VOLUME_USDC)
    if hours_to_resolution is None:
        hours_score = 0.0
    else:
        hours_score = min(
            1.0,
            max(
                0.0,
                (hours_to_resolution - MIN_RESOLUTION_HOURS)
                / (MAX_RESOLUTION_HOURS - MIN_RESOLUTION_HOURS),
            ),
        )
    score = round((gap_score + volume_score + hours_score) / 3.0, 6)

    return (
        MarketCandidate(
            market_id=market_id,
            title=snapshot.get("title") if isinstance(snapshot.get("title"), str) else market_id,
            market_price=float(price),
            price_gap_to_half=price_gap_to_half,
            volume_usdc=volume_usdc,
            hours_to_resolution=hours_to_resolution,
            score=score,
        ),
        None,
    )


def existing_market_scores(events: list[dict[str, Any]], scored_on: str) -> set[str]:
    scored_markets: set[str] = set()

    for event in events:
        if event.get("event_type") != "market.scored":
            continue

        payload = event.get("payload") or {}
        market_id = payload.get("market_id") or event.get("aggregate_key")
        if not isinstance(market_id, str) or not market_id.strip():
            continue

        payload_day = payload.get("scored_on")
        if isinstance(payload_day, str) and payload_day == scored_on:
            scored_markets.add(market_id)
            continue

        occurred_at = parse_timestamp(event.get("occurred_at"))
        if occurred_at is not None and occurred_at.date().isoformat() == scored_on:
            scored_markets.add(market_id)

    return scored_markets


def build_market_scored_event(
    candidate: MarketCandidate, run_id: str, scored_on: str, watch_dir: Path
) -> dict[str, Any]:
    if candidate.hours_to_resolution is None:
        hours_note = "hours_to_resolution=unknown"
    else:
        hours_note = f"hours_to_resolution={candidate.hours_to_resolution:.2f}"

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "market.scored",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": "runtime.agent.scoring",
        "idempotency_key": f"market.scored:v1:{candidate.market_id}:{scored_on}",
        "aggregate_key": candidate.market_id,
        "linkage": {
            "hypothesis_id": None,
            "signal_id": None,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": None,
            "correlation_id": None,
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": str(watch_dir),
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{candidate.market_id}",
            "notes": (
                "market scoring passed with "
                f"price={candidate.market_price:.6f} "
                f"gap={candidate.price_gap_to_half:.6f} "
                f"volume={candidate.volume_usdc:.2f} "
                f"{hours_note}"
            ),
        },
        "payload": {
            "market_id": candidate.market_id,
            "scored_on": scored_on,
            "score": candidate.score,
            "market_price": candidate.market_price,
            "price_gap_to_half": candidate.price_gap_to_half,
            "volume_usdc": candidate.volume_usdc,
            "hours_to_resolution": candidate.hours_to_resolution,
            "parameters": {
                "price_gap_limit": PRICE_GAP_LIMIT,
                "min_volume_usdc": MIN_VOLUME_USDC,
                "min_resolution_hours": MIN_RESOLUTION_HOURS,
                "max_resolution_hours": MAX_RESOLUTION_HOURS,
            },
        },
    }


def append_event(store_path: Path, event: dict[str, Any]) -> None:
    store_path.parent.mkdir(parents=True, exist_ok=True)
    with store_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)
    run_id = execution_run_id()
    scored_on = utc_today()

    events = load_jsonl(store_path)
    scored_today = existing_market_scores(events, scored_on)
    resolution_by_market = load_resolution_map(watch_dir)
    snapshots = load_active_snapshots(watch_dir)

    scored = 0
    skipped_already_scored = 0
    skipped_unscorable = 0
    skipped_by_filter = 0
    kill_reasons: dict[str, int] = {}

    for snapshot in snapshots:
        market_id = snapshot.get("market_id")
        if not isinstance(market_id, str):
            skipped_unscorable += 1
            continue

        if market_id in scored_today:
            skipped_already_scored += 1
            continue

        candidate, reason = score_market(snapshot, resolution_by_market.get(market_id))
        if candidate is None:
            skipped_by_filter += 1
            if reason is not None:
                kill_reasons[reason] = kill_reasons.get(reason, 0) + 1
            continue

        append_event(store_path, build_market_scored_event(candidate, run_id, scored_on, watch_dir))
        scored += 1

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "watch_dir": str(watch_dir),
                "scored_on": scored_on,
                "active_snapshots": len(snapshots),
                "already_scored_today": skipped_already_scored,
                "unscorable_snapshots": skipped_unscorable,
                "filtered_out": skipped_by_filter,
                "kill_reasons": kill_reasons,
                "market_scored_this_run": scored,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
