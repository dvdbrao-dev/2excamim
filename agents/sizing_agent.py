#!/usr/bin/env python3
"""Sizing Agent v1.

Reads eligible signals from the append-only JSONL store, applies Kelly sizing
against the most recent snapshot midpoint for the corresponding market, and
persists either `decision.formed` or `veto.raised`.
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


AGENT_ID = "sizing-agent-v1"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
DEFAULT_BANKROLL = 1000.0


@dataclass
class SignalCandidate:
    signal_id: str
    market_id: str
    instrument: str
    hypothesis_id: str | None
    correlation_id: str | None
    parent_event_id: str | None
    confirmation_score: float
    side: str | None
    market_price: float = 0.0
    bankroll: float = 0.0


@dataclass
class SnapshotCandidate:
    market_id: str
    observed_at: datetime
    midpoint: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Apply Kelly sizing to eligible signals.")
    parser.add_argument("--store", default=str(DEFAULT_STORE), help="Path to the JSONL store.")
    parser.add_argument(
        "--watch-dir",
        default=str(DEFAULT_WATCH_DIR),
        help="Path to the market-watch state directory.",
    )
    parser.add_argument(
        "--bankroll",
        type=float,
        default=DEFAULT_BANKROLL,
        help="Kelly bankroll in quote currency.",
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


def market_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = snapshot.get("best_bid")
    best_ask = snapshot.get("best_ask")
    if isinstance(best_bid, (int, float)) and isinstance(best_ask, (int, float)):
        if math.isfinite(float(best_bid)) and math.isfinite(float(best_ask)):
            return (float(best_bid) + float(best_ask)) / 2.0
    return None


def latest_snapshots(watch_dir: Path) -> dict[str, SnapshotCandidate]:
    snapshots_path = watch_dir / "snapshots.jsonl"
    latest: dict[str, SnapshotCandidate] = {}

    for snapshot in load_jsonl(snapshots_path):
        market_id = snapshot.get("market_id")
        observed_at = parse_timestamp(snapshot.get("observed_at"))
        midpoint = market_midpoint(snapshot)
        if not isinstance(market_id, str) or observed_at is None or midpoint is None:
            continue

        current = latest.get(market_id)
        candidate = SnapshotCandidate(
            market_id=market_id,
            observed_at=observed_at,
            midpoint=midpoint,
        )
        if current is None or candidate.observed_at > current.observed_at:
            latest[market_id] = candidate

    return latest


def collect_candidates(
    events: list[dict[str, Any]]
) -> tuple[dict[str, SignalCandidate], dict[str, float], set[str]]:
    generated: dict[str, SignalCandidate] = {}
    confirmed: dict[str, float] = {}
    vetoed: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}

        if event_type == "signal.generated":
            signal_id = payload.get("signal_id")
            if not isinstance(signal_id, str):
                continue

            market_id = normalized_market_id(event.get("aggregate_key")) or normalized_market_id(
                payload.get("instrument")
            )
            if market_id is None:
                continue

            side = payload.get("side")
            if not isinstance(side, str):
                side = None

            generated[signal_id] = SignalCandidate(
                signal_id=signal_id,
                market_id=market_id,
                instrument=payload.get("instrument") if isinstance(payload.get("instrument"), str) else market_id,
                hypothesis_id=linkage.get("hypothesis_id"),
                correlation_id=linkage.get("correlation_id"),
                parent_event_id=linkage.get("parent_event_id"),
                confirmation_score=0.0,
                side=side,
            )
        elif event_type == "signal.confirmed":
            signal_id = payload.get("signal_id")
            confirmation_score = payload.get("confirmation_score")
            if isinstance(signal_id, str) and isinstance(confirmation_score, (int, float)):
                confirmed[signal_id] = float(confirmation_score)
        elif event_type == "veto.raised":
            if payload.get("scope") != "Signal":
                continue
            target_id = payload.get("target_id")
            if isinstance(target_id, str):
                vetoed.add(target_id)

    return generated, confirmed, vetoed


def kelly_size(p_win: float, market_price: float, bankroll: float, max_fraction: float = 0.25) -> float:
    if (
        not math.isfinite(p_win)
        or not math.isfinite(market_price)
        or not math.isfinite(bankroll)
        or not math.isfinite(max_fraction)
        or bankroll <= 0
        or market_price <= 0
        or market_price >= 1
        or max_fraction <= 0
    ):
        return 0.0

    b = (1 / market_price) - 1
    if b <= 0:
        return 0.0
    q = 1 - p_win
    f_star = (p_win * b - q) / b
    if f_star <= 0:
        return 0.0
    return round(bankroll * min(f_star, max_fraction), 2)


def deterministic_decision_id(signal_id: str) -> str:
    return f"decision-{signal_id}"


def deterministic_veto_id(signal_id: str) -> str:
    return f"sizing-veto-{signal_id}"


def existing_event_ids(events: list[dict[str, Any]]) -> tuple[set[str], set[str]]:
    decisions: set[str] = set()
    vetoes: set[str] = set()

    for event in events:
        event_type = event.get("event_type")
        payload = event.get("payload") or {}
        if event_type == "decision.formed":
            decision_id = payload.get("decision_id")
            if isinstance(decision_id, str):
                decisions.add(decision_id)
        elif event_type == "veto.raised":
            veto_id = payload.get("veto_id")
            if isinstance(veto_id, str):
                vetoes.add(veto_id)

    return decisions, vetoes


def build_provenance(run_id: str, watch_dir: Path, market_id: str, notes: str) -> dict[str, Any]:
    return {
        "source_kind": "Runtime",
        "source_ref": str(watch_dir),
        "producer_run_id": run_id,
        "actor": AGENT_ID,
        "trace_id": f"{run_id}:{market_id}",
        "notes": notes,
    }


def build_decision_event(
    candidate: SignalCandidate, run_id: str, watch_dir: Path, size_hint: float
) -> dict[str, Any]:
    decision_id = deterministic_decision_id(candidate.signal_id)
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "decision.formed",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": "runtime.agent.sizing",
        "idempotency_key": f"decision.formed:v1:{decision_id}",
        "aggregate_key": candidate.instrument,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": decision_id,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.parent_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": build_provenance(
            run_id,
            watch_dir,
            candidate.market_id,
            f"kelly sizing approved p_win={candidate.confirmation_score:.6f} size_hint={size_hint:.2f}",
        ),
        "payload": {
            "decision_id": decision_id,
            "instrument": candidate.instrument,
            "action": "Enter",
            "side": candidate.side,
            "size_hint": size_hint,
            "rationale": (
                f"kelly sizing approved p_win={candidate.confirmation_score:.6f} "
                f"market_price={candidate.market_price:.6f} bankroll={candidate.bankroll:.2f}"
            ),
        },
    }


def build_veto_event(
    candidate: SignalCandidate, run_id: str, watch_dir: Path, reason_text: str
) -> dict[str, Any]:
    veto_id = deterministic_veto_id(candidate.signal_id)
    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "veto.raised",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": "runtime.agent.sizing",
        "idempotency_key": f"veto.raised:v1:{veto_id}",
        "aggregate_key": candidate.instrument,
        "linkage": {
            "hypothesis_id": candidate.hypothesis_id,
            "signal_id": candidate.signal_id,
            "decision_id": None,
            "order_id": None,
            "position_id": None,
            "parent_event_id": candidate.parent_event_id,
            "correlation_id": candidate.correlation_id,
        },
        "provenance": build_provenance(
            run_id,
            watch_dir,
            candidate.market_id,
            f"kelly sizing vetoed signal due to negative EV: {reason_text}",
        ),
        "payload": {
            "veto_id": veto_id,
            "scope": "Signal",
            "target_id": candidate.signal_id,
            "reason_code": "negative_ev",
            "reason_text": reason_text,
            "raised_by": AGENT_ID,
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
    generated, confirmed, vetoed = collect_candidates(events)
    snapshots = latest_snapshots(watch_dir)
    existing_decisions, existing_vetoes = existing_event_ids(events)

    eligible_candidates = [
        SignalCandidate(
            signal_id=candidate.signal_id,
            market_id=candidate.market_id,
            instrument=candidate.instrument,
            hypothesis_id=candidate.hypothesis_id,
            correlation_id=candidate.correlation_id,
            parent_event_id=candidate.parent_event_id,
            confirmation_score=confirmed[signal_id],
            side=candidate.side,
        )
        for signal_id, candidate in generated.items()
        if signal_id in confirmed and signal_id not in vetoed
    ]

    decisions_written = 0
    vetoes_written = 0
    skipped_missing_snapshot = 0
    skipped_already_written = 0
    skipped_not_eligible = len(generated) - len(eligible_candidates)

    for candidate in eligible_candidates:
        snapshot = snapshots.get(candidate.market_id)
        if snapshot is None:
            skipped_missing_snapshot += 1
            continue

        candidate.market_price = snapshot.midpoint
        candidate.bankroll = float(args.bankroll)
        size = kelly_size(candidate.confirmation_score, snapshot.midpoint, float(args.bankroll))

        if size <= 0:
            veto_id = deterministic_veto_id(candidate.signal_id)
            if veto_id in existing_vetoes:
                skipped_already_written += 1
                continue
            reason_text = (
                f"kelly size is zero for market_price={snapshot.midpoint:.6f} "
                f"p_win={candidate.confirmation_score:.6f} bankroll={float(args.bankroll):.2f}"
            )
            append_event(store_path, build_veto_event(candidate, run_id, watch_dir, reason_text))
            vetoes_written += 1
            existing_vetoes.add(veto_id)
            continue

        decision_id = deterministic_decision_id(candidate.signal_id)
        if decision_id in existing_decisions:
            skipped_already_written += 1
            continue

        append_event(store_path, build_decision_event(candidate, run_id, watch_dir, size))
        decisions_written += 1
        existing_decisions.add(decision_id)

    print(
        json.dumps(
            {
                "actor": AGENT_ID,
                "producer_run_id": run_id,
                "store": str(store_path),
                "watch_dir": str(watch_dir),
                "bankroll": float(args.bankroll),
                "generated_signals": len(generated),
                "eligible_signals": len(eligible_candidates),
                "decisions_written": decisions_written,
                "vetoes_written": vetoes_written,
                "skipped_missing_snapshot": skipped_missing_snapshot,
                "skipped_already_written": skipped_already_written,
                "skipped_not_eligible": skipped_not_eligible,
            },
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
