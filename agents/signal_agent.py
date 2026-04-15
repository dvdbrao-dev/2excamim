#!/usr/bin/env python3
"""Signal Agent v1.

Reads market-watch snapshots and generates vwap_reversion signals for open
markets with a meaningful midpoint gap.
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


AGENT_ID = "signal-agent-v1"
PRODUCED_BY = "runtime.agent.signal"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
STRATEGY = "vwap_reversion"
TIMEFRAME = "market_watch_snapshot"
GAP_FLOOR = 0.07


@dataclass
class SignalCandidate:
    market_id: str
    observed_at: datetime
    midpoint: float
    gap: float
    side: str
    signal_id: str
    aggregate_key: str
    instrument: str
    strength: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate signals from market-watch snapshots.")
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


def format_signal_timestamp(value: datetime) -> str:
    return value.strftime("%Y%m%dT%H%M%S") + f"_{value.microsecond // 1000:03d}Z"


def load_snapshots(watch_dir: Path) -> list[dict[str, Any]]:
    return load_jsonl(watch_dir / "snapshots.jsonl")


def existing_signal_ids(events: list[dict[str, Any]]) -> set[str]:
    signal_ids: set[str] = set()
    for event in events:
        if event.get("event_type") != "signal.generated":
            continue
        payload = event.get("payload") or {}
        signal_id = payload.get("signal_id")
        if isinstance(signal_id, str) and signal_id.strip():
            signal_ids.add(signal_id)
    return signal_ids


def numeric_value(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def build_candidate(snapshot: dict[str, Any]) -> SignalCandidate | None:
    market_id = snapshot.get("market_id")
    status = snapshot.get("status")
    best_bid = numeric_value(snapshot.get("best_bid"))
    best_ask = numeric_value(snapshot.get("best_ask"))
    observed_at = parse_timestamp(snapshot.get("observed_at"))

    if not isinstance(market_id, str) or not market_id.strip():
        return None
    if status != "Open":
        return None
    if best_bid is None or best_ask is None or observed_at is None:
        return None

    midpoint = (best_bid + best_ask) / 2.0
    gap = abs(midpoint - 0.5)
    if gap < GAP_FLOOR:
        return None

    side = "long_yes" if midpoint < 0.5 else "long_no"
    strength = min(gap * 2.0, 1.0)
    timestamp = format_signal_timestamp(observed_at)
    signal_id = f"research-{market_id}-vwap_reversion-{timestamp}-{side}"
    aggregate_key = f"polymarket:{market_id}"

    return SignalCandidate(
        market_id=market_id,
        observed_at=observed_at,
        midpoint=midpoint,
        gap=gap,
        side=side,
        signal_id=signal_id,
        aggregate_key=aggregate_key,
        instrument=aggregate_key,
        strength=strength,
    )


def build_signal_event(candidate: SignalCandidate, run_id: str, watch_dir: Path) -> dict[str, Any]:
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "signal.generated",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"signal.generated:v1:{candidate.signal_id}",
        "aggregate_key": candidate.aggregate_key,
        "linkage": {
            "hypothesis_id": None,
            "signal_id": candidate.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": None,
            "correlation_id": candidate.signal_id,
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": str(watch_dir),
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{candidate.signal_id}",
            "notes": (
                f"vwap_reversion midpoint={candidate.midpoint:.6f} "
                f"gap={candidate.gap:.6f} side={candidate.side}"
            ),
        },
        "payload": {
            "signal_id": candidate.signal_id,
            "strength": candidate.strength,
            "strategy": STRATEGY,
            "side": candidate.side,
            "instrument": candidate.instrument,
            "timeframe": TIMEFRAME,
            "rationale": (
                f"midpoint={candidate.midpoint:.6f} gap={candidate.gap:.6f} "
                f"strategy={STRATEGY}"
            ),
            "market_id": candidate.market_id,
            "midpoint": candidate.midpoint,
            "gap": candidate.gap,
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

    events = load_jsonl(store_path)
    known_signal_ids = existing_signal_ids(events)
    snapshots = load_snapshots(watch_dir)

    generated = 0
    skipped_existing = 0
    skipped_unscorable = 0

    for snapshot in snapshots:
        candidate = build_candidate(snapshot)
        if candidate is None:
            skipped_unscorable += 1
            continue
        if candidate.signal_id in known_signal_ids:
            skipped_existing += 1
            continue

        append_event(store_path, build_signal_event(candidate, run_id, watch_dir))
        known_signal_ids.add(candidate.signal_id)
        generated += 1

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "watch_dir": str(watch_dir),
                "strategy": STRATEGY,
                "threshold_gap": GAP_FLOOR,
                "snapshots_read": len(snapshots),
                "signals_generated": generated,
                "skipped_existing": skipped_existing,
                "skipped_unscorable": skipped_unscorable,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
