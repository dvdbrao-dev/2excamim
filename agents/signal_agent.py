#!/usr/bin/env python3
"""Signal Agent v1.

Reads market-watch snapshots and generates explicit signal types.
Default mode is conservative threshold-based signalling with NO-side bias.
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
DEFAULT_SIGNAL_TYPE = "threshold_extremes"
LEGACY_SIGNAL_TYPE = "legacy_gap_to_half"
TIMEFRAME = "market_watch_snapshot"
GAP_FLOOR = 0.07
DEFAULT_NO_THRESHOLD = 0.62
DEFAULT_YES_THRESHOLD = 0.38


@dataclass(frozen=True)
class SignalConfig:
    signal_type: str
    no_threshold: float
    yes_threshold: float
    allow_long_yes: bool
    allowed_categories: set[str]


@dataclass
class SignalCandidate:
    market_id: str
    observed_at: datetime
    midpoint: float
    gap: float
    side: str
    signal_type: str
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
    parser.add_argument(
        "--signal-type",
        choices=[DEFAULT_SIGNAL_TYPE, LEGACY_SIGNAL_TYPE],
        default=DEFAULT_SIGNAL_TYPE,
        help="Signal mode. `legacy_gap_to_half` keeps deprecated midpoint-gap logic.",
    )
    parser.add_argument(
        "--no-threshold",
        type=float,
        default=DEFAULT_NO_THRESHOLD,
        help="Long-NO trigger when midpoint >= threshold (threshold_extremes mode).",
    )
    parser.add_argument(
        "--yes-threshold",
        type=float,
        default=DEFAULT_YES_THRESHOLD,
        help="Long-YES trigger when midpoint <= threshold (threshold_extremes mode).",
    )
    parser.add_argument(
        "--allow-long-yes",
        action="store_true",
        help="Allow long-YES signals in threshold mode. Disabled by default for NO bias.",
    )
    parser.add_argument(
        "--allowed-categories",
        default="",
        help="Optional comma-separated category allow-list if snapshot has category-like field.",
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


def normalized_categories(value: str) -> set[str]:
    categories: set[str] = set()
    for item in value.split(","):
        trimmed = item.strip().lower()
        if trimmed:
            categories.add(trimmed)
    return categories


def category_from_snapshot(snapshot: dict[str, Any]) -> str | None:
    for key in ("category", "market_category", "group", "tag"):
        value = snapshot.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip().lower()
    return None


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


def snapshot_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = numeric_value(snapshot.get("best_bid"))
    best_ask = numeric_value(snapshot.get("best_ask"))
    if best_bid is None or best_ask is None:
        return None
    return (best_bid + best_ask) / 2.0


def build_candidate(
    snapshot: dict[str, Any],
    config: SignalConfig,
) -> tuple[SignalCandidate | None, str | None]:
    market_id = snapshot.get("market_id")
    status = snapshot.get("status")
    observed_at = parse_timestamp(snapshot.get("observed_at"))

    if not isinstance(market_id, str) or not market_id.strip():
        return None, "unscorable"
    if status != "Open":
        return None, "unscorable"
    if observed_at is None:
        return None, "unscorable"

    midpoint = snapshot_midpoint(snapshot)
    if midpoint is None:
        return None, "unscorable"

    snapshot_category = category_from_snapshot(snapshot)
    if config.allowed_categories:
        if snapshot_category is None or snapshot_category not in config.allowed_categories:
            return None, "category_filtered"

    gap = abs(midpoint - 0.5)
    side: str | None = None
    strength = 0.0
    if config.signal_type == LEGACY_SIGNAL_TYPE:
        if gap < GAP_FLOOR:
            return None, "threshold_not_met"
        side = "long_yes" if midpoint < 0.5 else "long_no"
        strength = min(gap * 2.0, 1.0)
    else:
        if midpoint >= config.no_threshold:
            side = "long_no"
            strength = midpoint
        elif midpoint <= config.yes_threshold:
            if not config.allow_long_yes:
                return None, "no_bias_filter"
            side = "long_yes"
            strength = 1.0 - midpoint
        else:
            return None, "threshold_not_met"

    timestamp = format_signal_timestamp(observed_at)
    signal_id = f"research-{market_id}-{config.signal_type}-{timestamp}-{side}"
    aggregate_key = f"polymarket:{market_id}"

    return SignalCandidate(
        market_id=market_id,
        observed_at=observed_at,
        midpoint=midpoint,
        gap=gap,
        side=side,
        signal_type=config.signal_type,
        signal_id=signal_id,
        aggregate_key=aggregate_key,
        instrument=aggregate_key,
        strength=strength,
    ), None


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
                f"{candidate.signal_type} midpoint={candidate.midpoint:.6f} "
                f"gap={candidate.gap:.6f} side={candidate.side}"
            ),
        },
        "payload": {
            "signal_id": candidate.signal_id,
            "strength": candidate.strength,
            "strategy": candidate.signal_type,
            "signal_type": candidate.signal_type,
            "side": candidate.side,
            "instrument": candidate.instrument,
            "timeframe": TIMEFRAME,
            "rationale": (
                f"midpoint={candidate.midpoint:.6f} gap={candidate.gap:.6f} "
                f"signal_type={candidate.signal_type}"
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
    if not 0.0 <= args.no_threshold <= 1.0:
        raise SystemExit("--no-threshold must be within [0,1]")
    if not 0.0 <= args.yes_threshold <= 1.0:
        raise SystemExit("--yes-threshold must be within [0,1]")
    if args.yes_threshold >= args.no_threshold:
        raise SystemExit("--yes-threshold must be lower than --no-threshold")

    config = SignalConfig(
        signal_type=args.signal_type,
        no_threshold=float(args.no_threshold),
        yes_threshold=float(args.yes_threshold),
        allow_long_yes=bool(args.allow_long_yes),
        allowed_categories=normalized_categories(args.allowed_categories),
    )
    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)
    run_id = execution_run_id()

    events = load_jsonl(store_path)
    known_signal_ids = existing_signal_ids(events)
    snapshots = load_snapshots(watch_dir)

    generated = 0
    skipped_existing = 0
    skipped_threshold_not_met = 0
    skipped_no_bias_filter = 0
    skipped_category_filtered = 0
    skipped_unscorable = 0

    for snapshot in snapshots:
        candidate, reason = build_candidate(snapshot, config)
        if candidate is None:
            if reason == "threshold_not_met":
                skipped_threshold_not_met += 1
                continue
            if reason == "no_bias_filter":
                skipped_no_bias_filter += 1
                continue
            if reason == "category_filtered":
                skipped_category_filtered += 1
                continue
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
                "signal_type": config.signal_type,
                "strategy": config.signal_type,
                "threshold_gap": GAP_FLOOR,
                "no_threshold": config.no_threshold,
                "yes_threshold": config.yes_threshold,
                "allow_long_yes": config.allow_long_yes,
                "allowed_categories": sorted(config.allowed_categories),
                "snapshots_read": len(snapshots),
                "signals_generated": generated,
                "skipped_existing": skipped_existing,
                "skipped_threshold_not_met": skipped_threshold_not_met,
                "skipped_no_bias_filter": skipped_no_bias_filter,
                "skipped_category_filtered": skipped_category_filtered,
                "skipped_unscorable": skipped_unscorable,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
