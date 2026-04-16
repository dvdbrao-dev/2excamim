#!/usr/bin/env python3
"""Edge analysis for formed decisions.

Reads the append-only JSONL event log plus the current market snapshots and
prints a compact view of decision edges ordered by final blend probability.
"""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")


@dataclass
class SnapshotRecord:
    observed_at: datetime
    title: str | None
    midpoint: float | None


@dataclass
class SignalMetrics:
    occurred_at: datetime | None
    p_market: float | None
    p_llm: float | None
    p_final: float | None
    direction: str | None


@dataclass
class DecisionRecord:
    decision_id: str
    signal_id: str | None
    market_id: str
    aggregate_key: str | None
    occurred_at: datetime | None
    size_hint: float | None
    title: str
    p_market: float | None
    p_llm: float | None
    p_final: float | None
    direction: str | None
    status: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Inspect edge for formed decisions.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    return parser.parse_args()


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


def market_midpoint(snapshot: dict[str, Any]) -> float | None:
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


def parse_notes(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if isinstance(value, str):
        trimmed = value.strip()
        if not trimmed:
            return {}
        try:
            parsed = json.loads(trimmed)
        except json.JSONDecodeError:
            return {}
        return parsed if isinstance(parsed, dict) else {}
    return {}


def load_snapshot_histories(watch_dir: Path) -> dict[str, list[SnapshotRecord]]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    histories: dict[str, list[SnapshotRecord]] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = normalized_market_id(snapshot.get("market_id"))
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        midpoint = market_midpoint(snapshot)
        title = snapshot.get("title") if isinstance(snapshot.get("title"), str) else None
        if market_id is None or observed_at is None or midpoint is None:
            continue

        histories.setdefault(market_id, []).append(
            SnapshotRecord(observed_at=observed_at, title=title, midpoint=midpoint)
        )

    for history in histories.values():
        history.sort(key=lambda snapshot: snapshot.observed_at)

    return histories


def snapshot_for_decision(
    histories: dict[str, list[SnapshotRecord]],
    market_id: str,
    occurred_at: datetime | None,
) -> SnapshotRecord | None:
    history = histories.get(market_id)
    if not history:
        return None
    if occurred_at is None:
        return history[-1]

    for snapshot in reversed(history):
        if snapshot.observed_at <= occurred_at:
            return snapshot
    return history[0]


def fmt_float(value: float | None, digits: int = 3) -> str:
    if value is None or not math.isfinite(float(value)):
        return "n/a"
    return f"{float(value):.{digits}f}"


def collect_decisions(
    events: list[dict[str, Any]],
    histories: dict[str, list[SnapshotRecord]],
) -> list[DecisionRecord]:
    decision_indexes: dict[str, int] = {}
    decision_market_ids: dict[str, str] = {}
    decision_signal_ids: dict[str, str | None] = {}
    decision_occurred_at: dict[str, datetime | None] = {}
    decision_aggregate_keys: dict[str, str | None] = {}
    decision_size_hints: dict[str, float | None] = {}
    signal_metrics: dict[str, SignalMetrics] = {}
    veto_indexes: dict[str, int] = {}
    resolved_markets: set[str] = set()

    for index, event in enumerate(events):
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}
        aggregate_key = event.get("aggregate_key")
        market_id = normalized_market_id(aggregate_key if isinstance(aggregate_key, str) else None)
        if market_id is None:
            market_id = normalized_market_id(payload.get("instrument"))
        occurred_at = parse_timestamp(event.get("occurred_at"))

        if event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue
            provenance = event.get("provenance") or {}
            notes = parse_notes(provenance.get("notes"))
            metrics = SignalMetrics(
                occurred_at=occurred_at,
                p_market=coerce_probability(
                    payload.get("market_midpoint"),
                    payload.get("p_market"),
                    notes.get("market_midpoint"),
                    notes.get("p_market"),
                ),
                p_llm=coerce_probability(
                    payload.get("estimated_probability"),
                    payload.get("p_llm"),
                    notes.get("estimated_probability"),
                    notes.get("p_llm"),
                ),
                p_final=coerce_probability(
                    payload.get("p_final"),
                    payload.get("confirmation_score"),
                    notes.get("p_final"),
                ),
                direction=normalize_direction(payload.get("direction"))
                or normalize_direction(notes.get("direction")),
            )
            current = signal_metrics.get(signal_id)
            if current is None or (
                metrics.occurred_at is not None
                and (current.occurred_at is None or metrics.occurred_at > current.occurred_at)
            ):
                signal_metrics[signal_id] = metrics
            continue

        if event_type == "market.scored" and market_id is not None:
            resolved_markets.add(market_id)
            continue

        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            if not isinstance(decision_id, str) or market_id is None:
                continue

            decision_indexes[decision_id] = index
            decision_market_ids[decision_id] = market_id
            decision_signal_ids[decision_id] = (
                linkage.get("signal_id") if isinstance(linkage.get("signal_id"), str) else None
            )
            decision_occurred_at[decision_id] = occurred_at
            decision_aggregate_keys[decision_id] = aggregate_key if isinstance(aggregate_key, str) else None
            decision_size_hints[decision_id] = coerce_probability(payload.get("size_hint"))

        elif event_type == "veto.raised" and payload.get("scope") == "Decision":
            target_id = payload.get("target_id")
            if isinstance(target_id, str):
                previous = veto_indexes.get(target_id)
                if previous is None or index > previous:
                    veto_indexes[target_id] = index

    records: list[DecisionRecord] = []
    for decision_id, market_id in decision_market_ids.items():
        signal_id = decision_signal_ids.get(decision_id)
        metrics = signal_metrics.get(signal_id) if signal_id else None
        snapshot = snapshot_for_decision(histories, market_id, decision_occurred_at.get(decision_id))
        title = snapshot.title if snapshot and snapshot.title else market_id

        status = "ACTIVA"
        if market_id in resolved_markets:
            status = "RESUELTA"
        else:
            veto_index = veto_indexes.get(decision_id)
            decision_index = decision_indexes.get(decision_id)
            if (
                veto_index is not None
                and decision_index is not None
                and veto_index > decision_index
            ):
                status = "VETADA"

        records.append(
            DecisionRecord(
                decision_id=decision_id,
                signal_id=signal_id,
                market_id=market_id,
                aggregate_key=decision_aggregate_keys.get(decision_id),
                occurred_at=decision_occurred_at.get(decision_id),
                size_hint=decision_size_hints.get(decision_id),
                title=title,
                p_market=(
                    metrics.p_market
                    if metrics and metrics.p_market is not None
                    else snapshot.midpoint if snapshot else None
                ),
                p_llm=metrics.p_llm if metrics and metrics.p_llm is not None else None,
                p_final=metrics.p_final if metrics and metrics.p_final is not None else None,
                direction=metrics.direction if metrics and metrics.direction is not None else None,
                status=status,
            )
        )

    records.sort(
        key=lambda record: (
            record.p_final is None,
            -(record.p_final if record.p_final is not None else 0.0),
            record.market_id,
            record.decision_id,
        )
    )
    return records


def normalize_direction(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    if trimmed in {"market_too_high", "market_too_low", "fair"}:
        return trimmed
    return None


def coerce_probability(*values: Any) -> float | None:
    for value in values:
        if isinstance(value, (int, float)) and math.isfinite(float(value)):
            return float(value)
    return None


def format_rows(records: list[DecisionRecord]) -> str:
    if not records:
        return "No decision.formed events found."

    header = (
        f"{'STATUS':8} {'DIRECTION':16} {'p_final':>8} {'p_llm':>8} "
        f"{'p_market':>9} {'size_hint':>10}  TITLE"
    )
    lines = [header]
    for record in records:
        title = record.title if len(record.title) <= 80 else record.title[:77] + "..."
        lines.append(
            f"{record.status:8} {record.direction or 'n/a':16} "
            f"{fmt_float(record.p_final):>8} {fmt_float(record.p_llm):>8} "
            f"{fmt_float(record.p_market):>9} {fmt_float(record.size_hint, 2):>10}  "
            f"{title} ({record.market_id})"
        )
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)

    events = load_jsonl(store_path)
    histories = load_snapshot_histories(watch_dir)
    records = collect_decisions(events, histories)
    print(format_rows(records))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
