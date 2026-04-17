#!/usr/bin/env python3
"""Crypto market matcher.

Reads crypto signals from the append-only store, finds related open Polymarket
markets from the latest snapshots, and appends `crypto.market.matched` events
with a suggested trade direction.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
import uuid
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


AGENT_ID = "crypto-matcher-v1"
PRODUCED_BY = "runtime.agent.crypto_matcher"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
KEYWORDS_BY_SYMBOL = {
    "BTCUSDT": ["bitcoin", "btc", "btc price", "bitcoin price"],
    "ETHUSDT": ["ethereum", "eth", "eth price", "ether"],
    "SOLUSDT": ["solana", "sol", "sol price"],
}
MIDPOINT_FLOOR = 0.10
MIDPOINT_CEILING = 0.90


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Match crypto signals to open markets.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    return parser.parse_args()


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


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


def parse_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    try:
        return datetime.fromisoformat(trimmed.replace("Z", "+00:00")).astimezone(timezone.utc)
    except ValueError:
        return None


def numeric_value(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def normalized_text(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", " ", value.lower()).strip()


def load_latest_snapshots(watch_dir: Path) -> list[dict[str, Any]]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    latest: dict[str, tuple[datetime, dict[str, Any]]] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = snapshot.get("market_id")
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        status = snapshot.get("status")
        title = snapshot.get("title")
        if (
            not isinstance(market_id, str)
            or not market_id.strip()
            or observed_at is None
            or not isinstance(status, str)
            or status.strip().lower() != "open"
            or not isinstance(title, str)
            or not title.strip()
        ):
            continue

        current = latest.get(market_id)
        if current is None or observed_at > current[0]:
            latest[market_id] = (observed_at, snapshot)

    return [snapshot for _, snapshot in latest.values()]


def market_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = numeric_value(snapshot.get("best_bid"))
    best_ask = numeric_value(snapshot.get("best_ask"))
    last_price = numeric_value(snapshot.get("last_price"))

    if best_bid is not None and best_ask is not None:
        return (best_bid + best_ask) / 2.0
    if last_price is not None:
        return last_price
    if best_bid is not None:
        return best_bid
    if best_ask is not None:
        return best_ask
    return None


def title_matches_keyword(title: str, keyword: str) -> bool:
    normalized_title = normalized_text(title)
    normalized_keyword = normalized_text(keyword)
    if " " in normalized_keyword:
        return normalized_keyword in normalized_title
    return re.search(rf"\b{re.escape(normalized_keyword)}\b", normalized_title) is not None


def market_orientation(title: str) -> str:
    lower_title = f" {title.lower()} "
    bearish_tokens = (
        " below ",
        " under ",
        " less than ",
        " at most ",
        " no more than ",
        " drop below ",
        " falls below ",
        " fall below ",
        " < ",
    )
    bullish_tokens = (
        " above ",
        " over ",
        " more than ",
        " greater than ",
        " at least ",
        " reach ",
        " reaches ",
        " hit ",
        " hits ",
        " > ",
    )
    if any(token in lower_title for token in bearish_tokens):
        return "lower_is_yes"
    if any(token in lower_title for token in bullish_tokens):
        return "higher_is_yes"
    return "higher_is_yes"


def direction_for_signal(trend: str, orientation: str) -> str:
    if (trend == "UP" and orientation == "higher_is_yes") or (
        trend == "DOWN" and orientation == "lower_is_yes"
    ):
        return "long_yes"
    return "long_no"


def load_events(path: Path) -> list[dict[str, Any]]:
    return load_jsonl(path)


def build_signal_index(events: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    signals: dict[str, dict[str, Any]] = {}
    for event in events:
        if event.get("event_type") != "crypto.signal.generated":
            continue
        payload = event.get("payload") or {}
        signal_id = payload.get("signal_id")
        symbol = payload.get("symbol")
        trend = payload.get("trend")
        if (
            not isinstance(signal_id, str)
            or not signal_id.strip()
            or not isinstance(symbol, str)
            or not symbol.strip()
            or trend not in ("UP", "DOWN")
        ):
            continue
        signals[signal_id] = event
    return signals


def matched_signal_ids(events: list[dict[str, Any]]) -> set[str]:
    signal_ids: set[str] = set()
    for event in events:
        if event.get("event_type") != "crypto.market.matched":
            continue
        payload = event.get("payload") or {}
        signal_id = payload.get("signal_id")
        if isinstance(signal_id, str) and signal_id.strip():
            signal_ids.add(signal_id)
    return signal_ids


def build_match_event(
    signal_event: dict[str, Any],
    signal_payload: dict[str, Any],
    matched_markets: list[dict[str, Any]],
    run_id: str,
    watch_dir: Path,
) -> dict[str, Any]:
    symbol = signal_payload["symbol"]
    signal_id = signal_payload["signal_id"]
    direction_counts = Counter(match["suggested_direction"] for match in matched_markets)
    suggested_direction = "long_yes"
    if direction_counts:
        suggested_direction = direction_counts.most_common(1)[0][0]

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "crypto.market.matched",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"crypto.market.matched:v1:{signal_id}",
        "aggregate_key": f"crypto:{symbol}",
        "linkage": {
            "signal_id": signal_id,
            "correlation_id": signal_id,
            "hypothesis_id": None,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": signal_event.get("event_id"),
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": str(watch_dir),
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{signal_id}",
            "notes": f"matched_markets={len(matched_markets)} suggested_direction={suggested_direction}",
        },
        "payload": {
            "signal_id": signal_id,
            "symbol": symbol,
            "trend": signal_payload["trend"],
            "suggested_direction": suggested_direction,
            "matched_markets": matched_markets,
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

    events = load_events(store_path)
    signals = build_signal_index(events)
    already_matched = matched_signal_ids(events)
    snapshots = load_latest_snapshots(watch_dir)

    generated = 0
    skipped_already_matched = 0
    skipped_no_match = 0

    for signal_id, signal_event in signals.items():
        if signal_id in already_matched:
            skipped_already_matched += 1
            continue

        payload = signal_event.get("payload") or {}
        symbol = payload.get("symbol")
        trend = payload.get("trend")
        if not isinstance(symbol, str) or trend not in ("UP", "DOWN"):
            skipped_no_match += 1
            continue

        keywords = KEYWORDS_BY_SYMBOL.get(symbol, [])
        matched_markets: list[dict[str, Any]] = []
        for snapshot in snapshots:
            title = snapshot.get("title")
            market_id = snapshot.get("market_id")
            if not isinstance(title, str) or not isinstance(market_id, str):
                continue

            if not any(title_matches_keyword(title, keyword) for keyword in keywords):
                continue

            midpoint = market_midpoint(snapshot)
            if midpoint is None or not (MIDPOINT_FLOOR <= midpoint <= MIDPOINT_CEILING):
                continue

            orientation = market_orientation(title)
            suggested_direction = direction_for_signal(trend, orientation)
            matched_markets.append(
                {
                    "market_id": market_id,
                    "title": title.strip(),
                    "midpoint": midpoint,
                    "orientation": orientation,
                    "suggested_direction": suggested_direction,
                    "coherent": suggested_direction == "long_yes",
                    "status": snapshot.get("status"),
                    "source": snapshot.get("source"),
                    "observed_at": snapshot.get("observed_at"),
                }
            )

        if not matched_markets:
            skipped_no_match += 1
            continue

        append_event(
            store_path,
            build_match_event(signal_event, payload, matched_markets, run_id, watch_dir),
        )
        already_matched.add(signal_id)
        generated += 1

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "watch_dir": str(watch_dir),
                "signals_seen": len(signals),
                "matches_generated": generated,
                "skipped_already_matched": skipped_already_matched,
                "skipped_no_match": skipped_no_match,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
