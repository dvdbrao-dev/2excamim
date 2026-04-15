#!/usr/bin/env python3
"""Exit Agent v1.

Reads the local JSONL store, reconstructs active paper-ledger positions from
fill events, evaluates a small ordered set of exit triggers against the most
recent market snapshot, and raises `veto.raised` events for active decisions.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
import uuid
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


AGENT_ID = "exit-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
PRODUCED_BY = "runtime.agent.exit"
TARGET_HIT_FACTOR = 0.85
VOLUME_MULTIPLIER = 3.0
STALE_THESIS_HOURS = 24.0
STALE_PRICE_CHANGE_LIMIT = 0.02
VOLUME_WINDOW_MINUTES = 10

GAP_PATTERN = re.compile(
    r"\b(?:expected_gap|gap_expected|price_gap_to_half)\s*[:=]\s*"
    r"([0-9]+(?:\.[0-9]+)?)\b"
)


@dataclass
class DecisionContext:
    decision_id: str
    instrument: str
    aggregate_key: str | None
    event_id: str | None
    occurred_at: datetime | None
    rationale: str | None
    signal_id: str | None
    correlation_id: str | None


@dataclass
class PositionContext:
    decision_id: str | None
    instrument: str
    outcome: str
    net_shares: float
    average_entry_price: float | None
    entry_at: datetime | None
    last_fill_at: datetime | None
    last_fill_price: float | None
    fill_count: int


@dataclass
class SnapshotContext:
    market_id: str
    observed_at: datetime
    midpoint: float
    volume: float | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Raise exit vetoes for active paper positions.")
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
    if isinstance(value, str):
        trimmed = value.strip()
        if not trimmed:
            return None
        try:
            return datetime.fromisoformat(trimmed.replace("Z", "+00:00")).astimezone(timezone.utc)
        except ValueError:
            return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return datetime.fromtimestamp(float(value), tz=timezone.utc)
    return None


def normalized_market_id(value: str | None) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if not trimmed:
        return None
    if ":" in trimmed:
        return trimmed.split(":", 1)[1]
    return trimmed


def parse_probability_like_gap(value: Any) -> float | None:
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        gap = float(value)
        if gap >= 0:
            return gap
    if isinstance(value, str):
        match = GAP_PATTERN.search(value)
        if match:
            try:
                gap = float(match.group(1))
            except ValueError:
                return None
            if gap >= 0:
                return gap
    if isinstance(value, str):
        try:
            parsed = json.loads(value)
        except json.JSONDecodeError:
            return None
        if isinstance(parsed, dict):
            for key in ("expected_gap", "gap_expected", "price_gap_to_half"):
                candidate = parsed.get(key)
                if isinstance(candidate, (int, float)) and math.isfinite(float(candidate)):
                    gap = float(candidate)
                    if gap >= 0:
                        return gap
    return None


def midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = snapshot.get("best_bid")
    best_ask = snapshot.get("best_ask")
    last_price = snapshot.get("last_price")

    if isinstance(best_bid, (int, float)) and isinstance(best_ask, (int, float)):
        if math.isfinite(float(best_bid)) and math.isfinite(float(best_ask)):
            return (float(best_bid) + float(best_ask)) / 2.0
    if isinstance(last_price, (int, float)) and math.isfinite(float(last_price)):
        return float(last_price)
    if isinstance(best_bid, (int, float)) and math.isfinite(float(best_bid)):
        return float(best_bid)
    if isinstance(best_ask, (int, float)) and math.isfinite(float(best_ask)):
        return float(best_ask)
    return None


def load_snapshot_histories(watch_dir: Path) -> dict[str, list[SnapshotContext]]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    histories: dict[str, list[SnapshotContext]] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = snapshot.get("market_id")
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        current_midpoint = midpoint(snapshot)
        if not isinstance(market_id, str) or observed_at is None or current_midpoint is None:
            continue

        normalized = normalized_market_id(market_id) or market_id
        histories.setdefault(normalized, []).append(
            SnapshotContext(
                market_id=market_id,
                observed_at=observed_at,
                midpoint=current_midpoint,
                volume=float(snapshot["volume"])
                if isinstance(snapshot.get("volume"), (int, float))
                else None,
            )
        )

    for history in histories.values():
        history.sort(key=lambda snapshot: snapshot.observed_at)

    return histories


def parse_fill_outcome(order_id: str, signal_id: str | None) -> str:
    if order_id.startswith("pm-paper-order-yes-"):
        return "yes"
    if order_id.startswith("pm-paper-order-no-"):
        return "no"
    if signal_id and order_id == f"pm-paper-order-{signal_id}":
        return "signal_direction"
    return "unknown"


def collect_decisions(events: list[dict[str, Any]]) -> dict[str, DecisionContext]:
    decisions: dict[str, DecisionContext] = {}

    for event in events:
        if event.get("event_type") != "decision.formed":
            continue

        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}
        decision_id = payload.get("decision_id")
        instrument = payload.get("instrument")
        if not isinstance(decision_id, str) or not isinstance(instrument, str):
            continue

        decisions[decision_id] = DecisionContext(
            decision_id=decision_id,
            instrument=instrument,
            aggregate_key=event.get("aggregate_key"),
            event_id=event.get("event_id"),
            occurred_at=parse_timestamp(event.get("occurred_at")),
            rationale=payload.get("rationale") if isinstance(payload.get("rationale"), str) else None,
            signal_id=linkage.get("signal_id") if isinstance(linkage.get("signal_id"), str) else None,
            correlation_id=linkage.get("correlation_id")
            if isinstance(linkage.get("correlation_id"), str)
            else None,
        )

    return decisions


def collect_positions(events: list[dict[str, Any]]) -> tuple[dict[tuple[str, str], PositionContext], set[str]]:
    positions: dict[tuple[str, str], PositionContext] = {}
    vetoed_decisions: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}

        if event_type == "veto.raised":
            if payload.get("scope") == "Decision":
                target_id = payload.get("target_id")
                if isinstance(target_id, str):
                    vetoed_decisions.add(target_id)
            continue

        if event_type != "fill.received":
            continue

        instrument = payload.get("instrument")
        order_id = payload.get("order_id")
        fill_id = payload.get("fill_id")
        side = payload.get("side")
        price = payload.get("price")
        quantity = payload.get("quantity")
        executed_at = parse_timestamp(payload.get("executed_at"))
        decision_id = payload.get("decision_id")
        linkage = event.get("linkage") or {}
        if isinstance(decision_id, str) is False:
            decision_id = linkage.get("decision_id") if isinstance(linkage.get("decision_id"), str) else None

        if (
            not isinstance(instrument, str)
            or not isinstance(order_id, str)
            or not isinstance(fill_id, str)
            or side not in {"Buy", "Sell"}
            or not isinstance(price, (int, float))
            or not isinstance(quantity, (int, float))
            or executed_at is None
        ):
            continue

        outcome = parse_fill_outcome(order_id, linkage.get("signal_id") if isinstance(linkage.get("signal_id"), str) else None)
        key = (instrument, outcome)
        current = positions.get(key)
        notional = float(price) * float(quantity)
        if current is None:
            current = PositionContext(
                decision_id=decision_id,
                instrument=instrument,
                outcome=outcome,
                net_shares=0.0,
                average_entry_price=None,
                entry_at=None,
                last_fill_at=executed_at,
                last_fill_price=float(price),
                fill_count=0,
            )
            positions[key] = current

        current.decision_id = current.decision_id or decision_id
        current.fill_count += 1
        current.last_fill_at = executed_at if current.last_fill_at is None or executed_at > current.last_fill_at else current.last_fill_at
        current.last_fill_price = float(price)
        current.entry_at = executed_at if current.entry_at is None or executed_at < current.entry_at else current.entry_at

        if side == "Buy":
            previous_shares = current.net_shares
            previous_cost_basis = 0.0 if current.average_entry_price is None else current.average_entry_price * previous_shares
            current.net_shares += float(quantity)
            if current.net_shares > 0:
                current.average_entry_price = (previous_cost_basis + notional) / current.net_shares
        else:
            current.net_shares -= float(quantity)
            if current.net_shares <= 0:
                current.average_entry_price = None

    return positions, vetoed_decisions


def infer_expected_gap(
    decision: DecisionContext | None,
    entry_price: float,
) -> float:
    if decision is not None:
        for candidate in (
            parse_probability_like_gap(decision.rationale),
        ):
            if candidate is not None:
                return candidate
    return abs(0.5 - entry_price)


def deterministic_veto_id(decision_id: str, trigger_name: str) -> str:
    safe_decision_id = decision_id.replace(":", "_")
    return f"exit-veto-{safe_decision_id}-{trigger_name.lower()}"


def build_provenance(run_id: str, watch_dir: Path, market_id: str, notes: str) -> dict[str, Any]:
    return {
        "source_kind": "Runtime",
        "source_ref": str(watch_dir),
        "producer_run_id": run_id,
        "actor": AGENT_ID,
        "trace_id": f"{run_id}:{market_id}",
        "notes": notes,
    }


def build_veto_event(
    decision: DecisionContext,
    market_id: str,
    trigger_name: str,
    reason_text: str,
    run_id: str,
    watch_dir: Path,
) -> dict[str, Any]:
    veto_id = deterministic_veto_id(decision.decision_id, trigger_name)
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "veto.raised",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"veto.raised:v1:{veto_id}",
        "aggregate_key": decision.instrument,
        "linkage": {
            "hypothesis_id": None,
            "signal_id": decision.signal_id,
            "decision_id": decision.decision_id,
            "order_id": None,
            "position_id": None,
            "parent_event_id": decision.event_id,
            "correlation_id": decision.correlation_id,
        },
        "provenance": build_provenance(
            run_id,
            watch_dir,
            market_id,
            f"exit trigger {trigger_name} raised for decision {decision.decision_id}: {reason_text}",
        ),
        "payload": {
            "veto_id": veto_id,
            "scope": "Decision",
            "target_id": decision.decision_id,
            "reason_code": trigger_name,
            "reason_text": reason_text,
            "raised_by": AGENT_ID,
        },
    }


def append_event(store_path: Path, event: dict[str, Any]) -> None:
    store_path.parent.mkdir(parents=True, exist_ok=True)
    with store_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, separators=(",", ":")) + "\n")


def rolling_volume_delta(history: list[SnapshotContext], end_index: int, window: timedelta) -> float | None:
    end_snapshot = history[end_index]
    window_start = end_snapshot.observed_at - window
    start_snapshot: SnapshotContext | None = None
    for candidate in reversed(history[: end_index + 1]):
        if candidate.observed_at <= window_start:
            start_snapshot = candidate
            break
    if start_snapshot is None or end_snapshot.volume is None or start_snapshot.volume is None:
        return None
    delta = end_snapshot.volume - start_snapshot.volume
    if delta < 0:
        return None
    return delta


def volume_exit_trigger(history: list[SnapshotContext]) -> tuple[bool, float | None, float | None]:
    if len(history) < 2:
        return False, None, None

    history = sorted(history, key=lambda snapshot: snapshot.observed_at)
    window = timedelta(minutes=VOLUME_WINDOW_MINUTES)
    recent_index = len(history) - 1
    recent_volume = rolling_volume_delta(history, recent_index, window)
    if recent_volume is None:
        return False, None, None

    baseline_windows: list[float] = []
    for index in range(1, len(history) - 1):
        delta = rolling_volume_delta(history, index, window)
        if delta is not None:
            baseline_windows.append(delta)

    if not baseline_windows:
        return False, recent_volume, None

    average_10min = sum(baseline_windows) / len(baseline_windows)
    if average_10min <= 0:
        return False, recent_volume, average_10min

    return recent_volume > average_10min * VOLUME_MULTIPLIER, recent_volume, average_10min


def evaluate_position(
    position: PositionContext,
    decision: DecisionContext | None,
    snapshot: SnapshotContext,
    history: list[SnapshotContext],
    run_id: str,
    watch_dir: Path,
) -> tuple[dict[str, Any] | None, str | None]:
    if position.decision_id is None or position.average_entry_price is None or position.entry_at is None:
        return None, "missing_position_context"

    entry_price = position.average_entry_price
    current_price = snapshot.midpoint
    gap_expected = infer_expected_gap(decision, entry_price)
    price_change = current_price - entry_price
    hours_since_entry = (snapshot.observed_at - position.entry_at).total_seconds() / 3600.0

    if position.net_shares >= 0:
        target_hit = current_price >= entry_price + (gap_expected * TARGET_HIT_FACTOR)
    else:
        target_hit = current_price <= entry_price - (gap_expected * TARGET_HIT_FACTOR)

    if target_hit:
        reason_text = (
            f"target hit current_price={current_price:.6f} entry_price={entry_price:.6f} "
            f"gap_expected={gap_expected:.6f}"
        )
        return build_veto_event(
            decision,
            snapshot.market_id,
            "TARGET_HIT",
            reason_text,
            run_id,
            watch_dir,
        ), "TARGET_HIT"

    volume_trigger, recent_volume, average_10min = volume_exit_trigger(history)
    if volume_trigger and recent_volume is not None and average_10min is not None:
        reason_text = (
            f"recent 10m volume={recent_volume:.6f} average_10m={average_10min:.6f} "
            f"multiplier={VOLUME_MULTIPLIER:.2f}"
        )
        return build_veto_event(
            decision,
            snapshot.market_id,
            "VOLUME_EXIT",
            reason_text,
            run_id,
            watch_dir,
        ), "VOLUME_EXIT"

    if hours_since_entry > STALE_THESIS_HOURS and abs(price_change) < STALE_PRICE_CHANGE_LIMIT:
        reason_text = (
            f"hours_since_entry={hours_since_entry:.6f} "
            f"price_change={price_change:.6f}"
        )
        return build_veto_event(
            decision,
            snapshot.market_id,
            "STALE_THESIS",
            reason_text,
            run_id,
            watch_dir,
        ), "STALE_THESIS"

    return None, None


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)
    run_id = execution_run_id()

    events = load_jsonl(store_path)
    decisions = collect_decisions(events)
    positions, vetoed_decisions = collect_positions(events)
    snapshot_histories = load_snapshot_histories(watch_dir)
    snapshots_by_market = {market_id: history[-1] for market_id, history in snapshot_histories.items() if history}

    written = 0
    skipped_missing_snapshot = 0
    skipped_inactive = 0
    skipped_already_vetoed = 0
    triggered_counts: dict[str, int] = {"TARGET_HIT": 0, "VOLUME_EXIT": 0, "STALE_THESIS": 0}

    for position in positions.values():
        if position.decision_id is None or position.decision_id in vetoed_decisions:
            skipped_already_vetoed += 1
            continue

        decision = decisions.get(position.decision_id)
        if decision is None:
            skipped_inactive += 1
            continue

        market_id = normalized_market_id(position.instrument) or position.instrument
        snapshot = snapshots_by_market.get(market_id)
        if snapshot is None:
            skipped_missing_snapshot += 1
            continue

        position_history = snapshot_histories.get(market_id, [snapshot])

        event, trigger_name = evaluate_position(
            position,
            decision,
            snapshot,
            position_history,
            run_id,
            watch_dir,
        )
        if event is None or trigger_name is None:
            continue

        append_event(store_path, event)
        written += 1
        triggered_counts[trigger_name] = triggered_counts.get(trigger_name, 0) + 1
        vetoed_decisions.add(position.decision_id)

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "watch_dir": str(watch_dir),
                "positions_found": len(positions),
                "vetoes_written": written,
                "triggered": triggered_counts,
                "skipped_missing_snapshot": skipped_missing_snapshot,
                "skipped_inactive": skipped_inactive,
                "skipped_already_vetoed": skipped_already_vetoed,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
